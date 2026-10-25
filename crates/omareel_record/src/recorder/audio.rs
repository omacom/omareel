//! The audio side of a native capture (`Recorder.cpp`'s `startAudioCapture` and
//! `Recorder::finishAudioCapture`): ffmpeg records the desktop's monitor source and the
//! microphone from PulseAudio into `audio.mka` while the screen is captured, and is muxed into
//! `screen.mp4` with the screen video afterwards.

use super::command_output;
use super::files::write_start_timestamp;
use super::monotonic_us;
use crate::capture::RecordOptions;
use omareel_core::compat::qdir::FileInfo;
use omareel_core::compat::qnumber;
use omareel_core::compat::qprocess::{ChannelMode, Process, ProcessState};

/// The PulseAudio sources [`start_audio_capture`] records: the default sink's monitor for the
/// desktop audio (`pactl get-default-sink`), then the microphone (the chosen device, or `pactl
/// get-default-source` for `default_input`). A source pactl does not name is left out.
/// `microphone` tells whether the microphone made it in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioSources {
    pub sources: Vec<String>,
    pub microphone: bool,
}

/// The sources for `options`, asking pactl as `startAudioCapture` does.
pub fn audio_sources(options: &RecordOptions) -> AudioSources {
    let mut result = AudioSources::default();
    if options.desktop_audio {
        let sink = command_output("pactl", &["get-default-sink"]);
        if !sink.is_empty() {
            result.sources.push(format!("{sink}.monitor"));
        }
    }
    if options.microphone_audio {
        let mut source = options.microphone_device.clone();
        if source == "default_input" {
            source = command_output("pactl", &["get-default-source"]);
        }
        if !source.is_empty() {
            result.sources.push(source);
            result.microphone = true;
        }
    }
    result
}

/// ffmpeg's arguments for recording `sources` into `path`: each source from PulseAudio, two
/// mixed with volumedetect on the second (the microphone), a lone microphone through
/// volumedetect, so a muted or dead microphone can be told from a quiet one; AAC in Matroska
/// flushed every half second, so the file stays readable if ffmpeg has to be killed.
pub fn audio_capture_arguments<S: AsRef<str>>(sources: &[S], microphone: bool, path: &str) -> Vec<String> {
    let mut arguments: Vec<String> =
        ["-y", "-hide_banner", "-nostats", "-loglevel", "info"].iter().map(|s| (*s).to_owned()).collect();
    for source in sources {
        arguments.extend(["-thread_queue_size", "1024", "-f", "pulse", "-i"].iter().map(|s| (*s).to_owned()));
        arguments.push(source.as_ref().to_owned());
    }
    if sources.len() > 1 {
        arguments.extend(
            ["-filter_complex", "[1:a]volumedetect[m];[0:a][m]amix=inputs=2:duration=longest[a]", "-map", "[a]"]
                .iter()
                .map(|s| (*s).to_owned()),
        );
    } else {
        arguments.extend(["-map", "0:a"].iter().map(|s| (*s).to_owned()));
    }
    if sources.len() == 1 && microphone {
        arguments.extend(["-af", "volumedetect"].iter().map(|s| (*s).to_owned()));
    }
    arguments.extend(
        ["-c:a", "aac", "-b:a", "192k", "-flush_packets", "1", "-cluster_time_limit", "500"]
            .iter()
            .map(|s| (*s).to_owned()),
    );
    arguments.push(path.to_owned());
    arguments
}

/// What [`start_audio_capture`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioCaptureStart {
    /// ffmpeg started and `<path>.ts` was written.
    pub started: bool,
    /// The monotonic time read just before ffmpeg was started (the C++ sets its `startedUs`
    /// then, even when the start fails); `None` when nothing was started (no sources).
    pub started_us: Option<i64>,
}

/// The file-static `startAudioCapture`: starts ffmpeg on `process` recording the sources of
/// `options` into `path`, its merged output into `<path>.log` (which volumedetect's report
/// ends up in), and writes the start time to `<path>.ts`.
pub fn start_audio_capture(process: &mut Process, options: &RecordOptions, path: &str) -> AudioCaptureStart {
    let AudioSources { sources, microphone } = audio_sources(options);
    if sources.is_empty() {
        return AudioCaptureStart { started: false, started_us: None };
    }
    let arguments = audio_capture_arguments(&sources, microphone, path);
    process.set_process_channel_mode(ChannelMode::Merged);
    process.set_standard_output_file(&format!("{path}.log"));
    let started_us = monotonic_us();
    process.start("ffmpeg", &arguments);
    let started = process.wait_for_started(5000) && write_start_timestamp(&format!("{path}.ts"), started_us);
    AudioCaptureStart { started, started_us: Some(started_us) }
}

/// The mux's arguments: the screen video and the audio shifted by `audio_started_us -
/// screen_started_us`, copied into `output_path`. The screen sets the length (`-t` of
/// `screen_duration_us`): audio past its end is cut and audio that ends early leaves the video
/// whole. Without a duration, `-shortest`.
pub fn mux_arguments(
    screen_path: &str,
    audio_path: &str,
    output_path: &str,
    screen_started_us: i64,
    audio_started_us: i64,
    screen_duration_us: i64,
) -> Vec<String> {
    let offset = (audio_started_us - screen_started_us) as f64 / 1_000_000.0;
    let length: Vec<String> = if screen_duration_us > 0 {
        vec!["-t".into(), qnumber::number_f64(screen_duration_us as f64 / 1_000_000.0, 'f', 6)]
    } else {
        vec!["-shortest".into()]
    };
    let mut arguments: Vec<String> =
        ["-y", "-loglevel", "error", "-i", screen_path, "-itsoffset"].iter().map(|s| (*s).to_owned()).collect();
    arguments.push(qnumber::number_f64(offset, 'f', 6));
    arguments
        .extend(["-i", audio_path, "-map", "0:v:0", "-map", "1:a:0", "-c", "copy"].iter().map(|s| (*s).to_owned()));
    arguments.extend(length);
    arguments.extend(["-movflags", "+faststart", output_path].iter().map(|s| (*s).to_owned()));
    arguments
}

/// `Recorder::finishAudioCapture`: stops the audio recorder (SIGINT; a source that sends
/// nothing leaves ffmpeg deaf to it, so after two seconds it is killed and whatever reached the
/// file is kept), then muxes it with the screen video ([`mux_arguments`]). True when the audio
/// file had something in it and the mux exited 0 within 30 seconds with a non-empty output.
pub fn finish_audio_capture(
    process: &mut Process,
    screen_path: &str,
    audio_path: &str,
    output_path: &str,
    screen_started_us: i64,
    audio_started_us: i64,
    screen_duration_us: i64,
) -> bool {
    if process.state() != ProcessState::NotRunning {
        // SAFETY: kill takes a pid and a signal; the child is running, so the pid is ours.
        unsafe { libc::kill(process.process_id() as libc::pid_t, libc::SIGINT) };
        if !process.wait_for_finished(2000) {
            process.kill();
            process.wait_for_finished(1000);
        }
    }
    if FileInfo::new(audio_path).size() <= 0 {
        return false;
    }
    let arguments =
        mux_arguments(screen_path, audio_path, output_path, screen_started_us, audio_started_us, screen_duration_us);
    let mut mux = Process::new();
    mux.start("ffmpeg", &arguments);
    mux.wait_for_finished(30000) && mux.exit_code() == 0 && FileInfo::new(output_path).size() > 0
}
