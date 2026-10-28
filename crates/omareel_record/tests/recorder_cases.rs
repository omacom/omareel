//! `tests/test_recorder.cpp`'s and `tests/test_regionpicker.cpp`'s cases for the code that moved
//! to Rust with the recorder (Recorder.cpp, ScreenCapture.cpp, RegionPicker.cpp,
//! CaptureExclusion.cpp), as the C++ tests had them. The goldens check the same functions over many more inputs; these keep the
//! scenarios the C++ tests were written for, including the ones that run ffmpeg.

use omareel_core::compat::qprocess::{Process, ProcessState};
use omareel_core::compat::qstandardpaths;
use omareel_record::capture::CaptureRegion;
use omareel_record::capture_exclusion::{self, HardwareCursorHold};
use omareel_record::recorder::audio::{AudioCaptureStart, finish_audio_capture, first_sample_us};
use omareel_record::recorder::{
    DiscardActions, GsrExitClassification, camera_capture_block, classify_gsr_exit, discard_timeline,
    even_capture_region, microphone_missing, microphone_peak_db,
};
use omareel_record::region_picker::{is_click_selection, parse_capture_options};
use omareel_record::screen_capture::{
    CaptureRingBookkeeping, EncoderProbeResults, Rect, Size, SlotState, capture_crop_rows, capture_frame_header,
    capture_stream_header, encoder_arguments,
};
use std::cell::RefCell;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

/// Held by the tests that start programs, so the one that changes `PATH` runs alone.
static PROGRAMS: Mutex<()> = Mutex::new(());

fn programs() -> MutexGuard<'static, ()> {
    PROGRAMS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A scratch directory removed at the end.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!("omareel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn file(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn have_ffmpeg() -> bool {
    !qstandardpaths::find_executable("ffmpeg").is_empty() && !qstandardpaths::find_executable("ffprobe").is_empty()
}

#[test]
fn classifies_gsr_exit() {
    use GsrExitClassification::*;
    let rows = [
        ("requested clean stop", 0, true, 1024, 1.5, UserStop),
        ("external clean stop", 0, false, 1024, 1.5, ExternalStop),
        ("non-zero exit", 1, false, 1024, 1.5, Failure),
        ("empty file", 0, false, 0, 1.5, Failure),
        ("zero duration", 0, false, 1024, 0.0, Failure),
        ("invalid duration", 0, false, 1024, f64::NAN, Failure),
    ];
    for (name, exit_code, stop_requested, file_size, duration, expected) in rows {
        assert_eq!(classify_gsr_exit(exit_code, stop_requested, file_size, duration), expected, "{name}");
    }
}

#[test]
fn trims_capture_region_to_even_size() {
    let region = CaptureRegion {
        x: 10.0,
        y: 20.0,
        width: 1521.0,
        height: 835.0,
        physical_width: 1521,
        physical_height: 835,
        ..CaptureRegion::default()
    };
    let trimmed = even_capture_region(region.clone());
    assert_eq!((trimmed.physical_width, trimmed.physical_height), (1520, 834));
    assert_eq!((trimmed.width, trimmed.height, trimmed.x, trimmed.y), (1520.0, 834.0, 10.0, 20.0));

    let region = CaptureRegion { scale: 2.0, width: 760.5, height: 418.0, physical_width: 1521, physical_height: 836, ..region };
    let scaled = even_capture_region(region.clone());
    assert_eq!((scaled.physical_width, scaled.physical_height), (1520, 836));
    assert_eq!((scaled.width, scaled.height), (760.0, 418.0));

    let region = CaptureRegion { scale: 1.0, width: 1520.0, height: 836.0, physical_width: 1520, physical_height: 836, ..region };
    let even = even_capture_region(region);
    assert_eq!((even.physical_width, even.physical_height, even.width, even.height), (1520, 836, 1520.0, 836.0));
}

#[test]
fn camera_capture_block_includes_settings() {
    let camera = camera_capture_block("/dev/video2", 1080, 1920, 1080, 30.0, 1234567, "ffmpeg-pipe", 90, true, "round", 16.0);
    assert_eq!(camera.value("backend").to_string_or(""), "ffmpeg-pipe");
    assert_eq!(camera.value("requestedHeight").to_int(0), 1080);
    assert_eq!((camera.value("width").to_int(0), camera.value("height").to_int(0)), (1920, 1080));
    assert_eq!(camera.value("fps").to_double(0.0), 30.0);
    assert_eq!(camera.value("first_frame_us").to_variant_long_long(), 1234567);
    assert_eq!(camera.value("rotation").to_int(0), 90);
    assert!(camera.value("flipHorizontal").to_bool(false));
    assert_eq!(camera.value("shape").to_string_or(""), "round");
    assert_eq!(camera.value("radius").to_double(0.0), 16.0);
}

#[test]
fn discard_timeline_never_drains() {
    let calls = RefCell::new(Vec::new());
    let camera_wait_ms = RefCell::new(-1);
    let removed = discard_timeline(&mut DiscardActions {
        abort_capture: Some(Box::new(|| calls.borrow_mut().push("abort-capture"))),
        kill_audio: Some(Box::new(|| calls.borrow_mut().push("kill-audio"))),
        request_camera_stop: Some(Box::new(|| calls.borrow_mut().push("request-camera-stop"))),
        wait_for_camera_stop: Some(Box::new(|timeout_ms| {
            *camera_wait_ms.borrow_mut() = timeout_ms;
            calls.borrow_mut().push("wait-camera");
        })),
        remove_bundle: Some(Box::new(|| {
            calls.borrow_mut().push("remove-bundle");
            true
        })),
        remove_state: Some(Box::new(|| calls.borrow_mut().push("remove-state"))),
    });
    assert!(removed);
    assert_eq!(*camera_wait_ms.borrow(), 1000);
    assert_eq!(
        *calls.borrow(),
        ["abort-capture", "kill-audio", "request-camera-stop", "wait-camera", "remove-bundle", "remove-state"]
    );
}

#[test]
fn capture_ring_tracks_slots_and_drops() {
    let mut ring = CaptureRingBookkeeping::new(3);
    let (first, second, third) = (ring.acquire(), ring.acquire(), ring.acquire());
    assert_eq!((first, second, third), (0, 1, 2));
    assert_eq!(ring.count(SlotState::Capturing), 3);
    assert_eq!(ring.acquire(), -1);
    assert_eq!(ring.drops(), 1);
    assert!(ring.mark_queued(second));
    assert!(ring.mark_queued(first));
    assert_eq!(ring.take_queued(), second);
    assert_eq!(ring.take_queued(), first);
    assert_eq!(ring.state(second), SlotState::Writing);
    assert!(ring.release(second));
    assert_eq!(ring.state(second), SlotState::Available);
    assert_eq!(ring.acquire(), second);
    assert!(!ring.mark_queued(first + 8));
}

#[test]
fn capture_region_row_crop_math() {
    let rows = capture_crop_rows(Size::new(3840, 2160), 3840 * 4, Rect::new(320, 240, 1280, 960));
    assert_eq!(rows.len(), 960);
    assert_eq!(rows[0].source_offset, 240 * 3840 * 4 + 320 * 4);
    assert_eq!((rows[0].destination_offset, rows[0].bytes), (0, 1280 * 4));
    let last = rows.last().unwrap();
    assert_eq!(last.source_offset, (240 + 959) * 3840 * 4 + 320 * 4);
    assert_eq!(last.destination_offset, 959 * 1280 * 4);

    let full = capture_crop_rows(Size::new(3840, 2160), 3840 * 4, Rect::new(0, 0, 3840, 2160));
    assert_eq!(full.len(), 1);
    assert_eq!((full[0].source_offset, full[0].bytes), (0, 3840 * 2160 * 4));
}

#[test]
fn capture_stream_keeps_frame_times_and_colours() {
    let _programs = programs();
    if !have_ffmpeg() {
        eprintln!("ffmpeg/ffprobe unavailable");
        return;
    }
    let scratch = Scratch::new("capture-stream");
    let video = scratch.file("screen.mp4");
    let size = Size::new(64, 32);
    // A capture that stalls between 0.117 s and 0.5 s must not close that gap up.
    let times_us = [0, 16667, 100000, 116667, 500000];
    let mut stream = capture_stream_header(size, 60);
    for &time_us in &times_us {
        let frame: Vec<u8> = (0..size.width * size.height).flat_map(|_| [0xc8, 0x32, 0x0a, 0xff]).collect(); // B, G, R, X
        stream.extend(capture_frame_header(time_us, frame.len() as i64));
        stream.extend(frame);
    }
    // The arguments a recording without NVENC runs ffmpeg with.
    let arguments = encoder_arguments(size, 60, &EncoderProbeResults::default(), None, None, &video).arguments;
    let mut encoder = Process::new();
    encoder.start("ffmpeg", &arguments);
    assert!(encoder.wait_for_started(5000));
    encoder.write(&stream);
    encoder.close_write_channel();
    assert!(encoder.wait_for_finished(30000));
    assert_eq!(encoder.exit_code(), 0, "{}", String::from_utf8_lossy(&encoder.read_all_standard_error()));

    let mut probe = Process::new();
    probe.start(
        "ffprobe",
        &["-v", "error", "-select_streams", "v", "-show_entries", "packet=pts_time:format=duration", "-of", "csv=p=0", &video],
    );
    assert!(probe.wait_for_finished(10000));
    let output = String::from_utf8_lossy(&probe.read_all_standard_output()).into_owned();
    let lines: Vec<&str> = output.split('\n').filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), times_us.len() + 1);
    for (line, &time_us) in lines.iter().zip(&times_us) {
        assert!((line.parse::<f64>().unwrap() - time_us as f64 / 1e6).abs() < 0.001, "{line}");
    }
    assert!((lines.last().unwrap().parse::<f64>().unwrap() - (0.5 + 1.0 / 60.0)).abs() < 0.001);

    let mut decoder = Process::new();
    decoder.start("ffmpeg", &["-v", "error", "-i", &video, "-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"]);
    assert!(decoder.wait_for_finished(10000));
    let rgb = decoder.read_all_standard_output();
    assert!(rgb.len() >= 3);
    let near = |value: u8, expected: i32| (i32::from(value) - expected).abs() <= 4;
    assert!(near(rgb[0], 10) && near(rgb[1], 50) && near(rgb[2], 200), "{},{},{}", rgb[0], rgb[1], rgb[2]);
    // Converted with BT.709, and tagged so (players read untagged HD as BT.709 anyway).
    let mut tags = Process::new();
    tags.start(
        "ffprobe",
        &["-v", "error", "-select_streams", "v", "-show_entries", "stream=color_space,color_range", "-of", "csv=p=0", &video],
    );
    assert!(tags.wait_for_finished(10000));
    assert_eq!(String::from_utf8_lossy(&tags.read_all_standard_output()).trim(), "tv,bt709");
}

#[test]
fn killed_capture_encoder_leaves_a_playable_video() {
    let _programs = programs();
    if !have_ffmpeg() {
        eprintln!("ffmpeg/ffprobe unavailable");
        return;
    }
    let scratch = Scratch::new("killed-encoder");
    let video = scratch.file("screen.mp4");
    let size = Size::new(64, 32);
    // Two seconds of a still screen (small frames), then the encoder is killed without warning.
    let mut stream = capture_stream_header(size, 60);
    for index in 0..120i64 {
        let frame: Vec<u8> = (0..size.width * size.height).flat_map(|_| [0x40, 0x80, 0xc0, 0xff]).collect();
        stream.extend(capture_frame_header(index * 1_000_000 / 60, frame.len() as i64));
        stream.extend(frame);
    }
    let arguments = encoder_arguments(size, 60, &EncoderProbeResults::default(), None, None, &video).arguments;
    let mut encoder = Process::new();
    encoder.start("ffmpeg", &arguments);
    assert!(encoder.wait_for_started(5000));
    encoder.write(&stream);
    while encoder.bytes_to_write() > 0 && encoder.wait_for_bytes_written(5000) {}
    let packets = || {
        let mut probe = Process::new();
        probe.start(
            "ffprobe",
            &["-v", "error", "-count_packets", "-select_streams", "v", "-show_entries", "stream=nb_read_packets", "-of", "csv=p=0", &video],
        );
        probe.wait_for_finished(10000);
        String::from_utf8_lossy(&probe.read_all_standard_output()).trim().parse::<i32>().unwrap_or(0)
    };
    // The first one-second fragment reaches the file once the second keyframe is encoded.
    let started = Instant::now();
    while packets() < 60 && started.elapsed().as_secs() < 10 {
        encoder.wait_for_bytes_written(50);
    }
    encoder.kill();
    assert!(encoder.wait_for_finished(5000));
    let packets = packets();
    assert!(packets >= 60, "{packets} packets survived");
}

#[test]
fn stuck_audio_capture_stops_quickly_and_keeps_its_audio() {
    let _programs = programs();
    if !have_ffmpeg() {
        eprintln!("ffmpeg/ffprobe unavailable");
        return;
    }
    let scratch = Scratch::new("stuck-audio");
    let screen = scratch.file("screen-video.mp4");
    let audio = scratch.file("audio.mka");
    let mut make = Process::new();
    make.start("ffmpeg", &["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30", "-t", "2", "-c:v", "libx264", &screen]);
    assert!(make.wait_for_finished(30000) && make.exit_code() == 0);
    make.start("ffmpeg", &["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000", "-t", "2", "-c:a", "aac", &audio]);
    assert!(make.wait_for_finished(30000) && make.exit_code() == 0);
    // ffmpeg blocked on a PipeWire source that sends nothing never acts on SIGINT.
    let deaf_recorder = |process: &mut Process| {
        process.start("sh", &["-c", "trap '' INT; echo ready; exec sleep 30"]);
        process.wait_for_ready_read(5000)
    };

    let mut stuck = Process::new();
    assert!(deaf_recorder(&mut stuck));
    let output = scratch.file("screen.mp4");
    let start = AudioCaptureStart { started: true, started_us: Some(0), realtime_offset_us: 0 };
    let started = Instant::now();
    assert!(finish_audio_capture(&mut stuck, &screen, &audio, &output, 0, &start, 2000000).muxed);
    assert!(started.elapsed().as_millis() < 5000, "{:?}", started.elapsed());
    assert_eq!(stuck.state(), ProcessState::NotRunning);
    let mut probe = Process::new();
    probe.start("ffprobe", &["-v", "error", "-select_streams", "a", "-show_entries", "stream=codec_type", "-of", "csv=p=0", &output]);
    assert!(probe.wait_for_finished(15000));
    assert_eq!(String::from_utf8_lossy(&probe.read_all_standard_output()).trim(), "audio");

    // Nothing ever reached the file: no audio, and still no long wait.
    let mut silent = Process::new();
    assert!(deaf_recorder(&mut silent));
    let started = Instant::now();
    assert!(!finish_audio_capture(&mut silent, &screen, &scratch.file("none.mka"), &scratch.file("silent.mp4"), 0, &start, 2000000).muxed);
    assert!(started.elapsed().as_millis() < 5000, "{:?}", started.elapsed());
}

#[test]
fn audio_is_placed_by_its_first_sample() {
    // What ffmpeg logs for a PulseAudio input: the wall-clock time of its first sample.
    let log = "Input #0, pulse, from 'alsa_output.monitor':\n  Duration: N/A, start: 1761612345.678900, bitrate: 1536 kb/s\n\
               Input #1, pulse, from 'mic':\n  Duration: N/A, start: 1761612345.900000, bitrate: 1536 kb/s\n";
    let offset = 1_761_612_000_000_000;
    let started_us = 345_600_000;
    assert_eq!(first_sample_us(log, started_us, offset), Some(345_678_900));
    // No start line, or one from a wall clock that jumped: none.
    assert_eq!(first_sample_us("Input #0, pulse, from 'x':\n", started_us, offset), None);
    assert_eq!(first_sample_us(log, started_us, offset - 60_000_000), None);
}

#[test]
fn recording_holds_hardware_cursors() {
    let _programs = programs();
    let scratch = Scratch::new("cursor-hold");
    let log = scratch.file("calls");
    let mock = scratch.file("hyprctl");
    std::fs::write(
        &mock,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\n\
             case \"$*\" in\n \
             '-j getoption cursor:no_hardware_cursors') printf '{{\"option\": \"cursor:no_hardware_cursors\", \
             \"int\": %s, \"set\": false }}' \"$OMAREEL_TEST_CURSOR\";;\n \
             eval*) exit \"${{OMAREEL_TEST_EVAL_EXIT:-0}}\";;\nesac\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&mock, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let old_path = std::env::var_os("PATH");
    // SAFETY: the programs lock keeps the other tests that start programs out meanwhile.
    unsafe { std::env::set_var("PATH", &scratch.0) };
    let calls = || {
        let lines: Vec<String> =
            std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect();
        let _ = std::fs::remove_file(&log);
        lines
    };
    let set = |name: &str, value: &str| {
        // SAFETY: as above.
        unsafe { std::env::set_var(name, value) };
    };

    // Hyprland's automatic choice is held at hardware and put back afterwards.
    set("OMAREEL_TEST_CURSOR", "2");
    drop(HardwareCursorHold::new());
    assert_eq!(
        calls(),
        [
            "-j getoption cursor:no_hardware_cursors",
            "eval hl.config({ cursor = { no_hardware_cursors = 0 } })",
            "eval hl.config({ cursor = { no_hardware_cursors = 2 } })",
        ]
    );
    // Hardware cursors already: nothing to change.
    set("OMAREEL_TEST_CURSOR", "0");
    drop(HardwareCursorHold::new());
    assert_eq!(calls(), ["-j getoption cursor:no_hardware_cursors"]);
    // A hyprlang config takes keyword instead of eval.
    set("OMAREEL_TEST_CURSOR", "1");
    set("OMAREEL_TEST_EVAL_EXIT", "1");
    drop(HardwareCursorHold::new());
    assert_eq!(
        calls(),
        [
            "-j getoption cursor:no_hardware_cursors",
            "eval hl.config({ cursor = { no_hardware_cursors = 0 } })",
            "keyword cursor:no_hardware_cursors 0",
            "eval hl.config({ cursor = { no_hardware_cursors = 1 } })",
            "keyword cursor:no_hardware_cursors 1",
        ]
    );
    // SAFETY: as above.
    unsafe {
        match old_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        std::env::remove_var("OMAREEL_TEST_CURSOR");
        std::env::remove_var("OMAREEL_TEST_EVAL_EXIT");
    }
}

/// Environment variables set while the programs lock is held, put back when dropped.
struct ScopedEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl ScopedEnvironment {
    fn new() -> ScopedEnvironment {
        ScopedEnvironment(Vec::new())
    }

    fn set(&mut self, name: &'static str, value: impl AsRef<std::ffi::OsStr>) {
        if !self.0.iter().any(|(saved, _)| *saved == name) {
            self.0.push((name, std::env::var_os(name)));
        }
        // SAFETY: the programs lock keeps the other tests that start programs out meanwhile.
        unsafe { std::env::set_var(name, value) };
    }
}

impl Drop for ScopedEnvironment {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..).rev() {
            // SAFETY: as in set.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

// test_recorder.cpp's captureExclusionChecksHashBeforeLoading: a plugin whose .hash names another
// Hyprland commit is not loaded, one that matches is, and Hyprland's refusal and a configured path
// that does not exist are reported.
#[test]
fn capture_exclusion_checks_hash_before_loading() {
    let _programs = programs();
    let scratch = Scratch::new("exclusion-hash");
    let plugin = scratch.file("plugin.so");
    std::fs::write(&plugin, "fixture").unwrap();
    let log = scratch.file("calls");
    let mock = scratch.file("hyprctl");
    std::fs::write(
        &mock,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\n\
             case \"$*\" in\n \
             '-j plugin list') printf '[]';;\n \
             '-j version') printf '{{\"commit\":\"{}\"}}';;\n \
             *) printf 'Plugin could not be loaded';;\nesac\n",
            "a".repeat(40)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&mock, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let mut environment = ScopedEnvironment::new();
    environment.set("PATH", &scratch.0);
    environment.set("OMAREEL_PLUGIN_PATH", &plugin);
    let sidecar = format!("{plugin}.hash");
    std::fs::write(&sidecar, "b".repeat(40)).unwrap();
    let mismatch = capture_exclusion::ensure_loaded("");
    assert!(!mismatch.loaded);
    assert!(mismatch.reason.contains("rebuilding"), "{}", mismatch.reason);
    assert!(!std::fs::read_to_string(&log).unwrap().contains("plugin load"));
    std::fs::write(&sidecar, "a".repeat(40)).unwrap();
    let refused = capture_exclusion::ensure_loaded("");
    assert!(!refused.loaded);
    assert!(refused.reason.contains("refused"), "{}", refused.reason);
    assert!(std::fs::read_to_string(&log).unwrap().contains("plugin load"));
    environment.set("OMAREEL_PLUGIN_PATH", scratch.file("absent.so"));
    assert!(capture_exclusion::ensure_loaded("").reason.contains("does not exist"));
}

#[test]
fn missing_microphone_falls_back_to_default() {
    let sources = [
        "alsa_input.platform-mtk_sdw_mc.HiFi__Mic__source",
        "alsa_input.usb-Insta360_Insta360_Link_2-02.mono-fallback",
    ];
    assert!(!microphone_missing("alsa_input.usb-Insta360_Insta360_Link_2-02.mono-fallback", &sources));
    assert!(microphone_missing("alsa_input.usb-Elgato_Cam_Link_4K-02.analog-stereo", &sources));
    assert!(!microphone_missing("default_input", &sources));
    assert!(!microphone_missing("alsa_input.usb-Elgato_Cam_Link_4K-02.analog-stereo", &[] as &[&str]));
}

#[test]
fn reads_microphone_peak_from_ffmpeg_log() {
    let probe = "[Parsed_volumedetect_0 @ 0xbfe75fef2360] n_samples: 0\n";
    let silent = format!(
        "{probe}[Parsed_volumedetect_0 @ 0xf76498003ae0] n_samples: 49152\n\
         [Parsed_volumedetect_0 @ 0xf76498003ae0] mean_volume: -91.0 dB\n\
         [Parsed_volumedetect_0 @ 0xf76498003ae0] max_volume: -91.0 dB\n\
         [Parsed_volumedetect_0 @ 0xf76498003ae0] histogram_91db: 49152\n"
    );
    assert_eq!(microphone_peak_db(&silent), -91.0);
    let speech = format!(
        "{probe}[Parsed_volumedetect_0 @ 0x1] n_samples: 960000\n\
         [Parsed_volumedetect_0 @ 0x1] mean_volume: -38.2 dB\n\
         [Parsed_volumedetect_0 @ 0x1] max_volume: -6.4 dB\n\
         [Parsed_volumedetect_0 @ 0x1] histogram_6db: 12\n"
    );
    assert_eq!(microphone_peak_db(&speech), -6.4);
    assert!(microphone_peak_db(probe).is_infinite() && microphone_peak_db(probe) < 0.0);
    assert!(microphone_peak_db("size=N/A time=00:00:10.00\n").is_nan());
}

#[test]
fn parses_capture_options() {
    let output = "DP-3|3840x2160\nregion\n  DP-5 | 2560x720  \r\ninvalid|size\nDP-3|3840x2160\n";
    assert_eq!(parse_capture_options(output), ["DP-3", "DP-5"]);
}

#[test]
fn ignores_empty_and_malformed_capture_options() {
    assert!(parse_capture_options("").is_empty());
    assert!(parse_capture_options("screen\nregion\nDP-3|3840-by-2160\n").is_empty());
}

#[test]
fn detects_bare_clicks_by_area() {
    assert!(is_click_selection(1.0, 19.0));
    assert!(is_click_selection(3.0, 6.0));
    assert!(!is_click_selection(4.0, 5.0));
    assert!(!is_click_selection(100.0, 100.0));
    assert!(!is_click_selection(0.0, 10.0));
}
