//! What a video file holds (`MediaProbe`): inserted clips' size, length, frame rate and audio,
//! and the frame times of a recording; from FFmpeg's libraries, else from ffprobe.

use crate::compat::qdir::FileInfo;
use crate::compat::qjson::{self, Object};
use crate::compat::qprocess::{Process, ProcessState};
use crate::compat::qround::q_round;
use crate::compat::{qfile, qnumber, qstring};

/// What an inserted video holds, as ffmpeg will decode it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    /// Display size: rotation and sample aspect ratio applied.
    pub width: i32,
    pub height: i32,
    /// Of the video stream; of the container when the stream has none.
    pub duration: f64,
    pub fps: f64,
    pub audio: bool,
}

impl Default for VideoInfo {
    fn default() -> VideoInfo {
        VideoInfo { width: 0, height: 0, duration: 0.0, fps: 30.0, audio: false }
    }
}

impl VideoInfo {
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.duration > 0.0
    }
}

/// `QString::toDouble()`, 0 when the text is not a number.
fn to_double(text: &str) -> f64 {
    qnumber::to_double(text).unwrap_or(0.0)
}

/// A rate or ratio such as "30000/1001" or "4:3", or a plain number; 0 for a zero denominator
/// and for anything that is not two parts or a number.
fn ratio(text: &str) -> f64 {
    let parts: Vec<&str> = text.split(['/', ':']).collect();
    if parts.len() != 2 {
        return to_double(text);
    }
    let denominator = to_double(parts[1]);
    if denominator != 0.0 { to_double(parts[0]) / denominator } else { 0.0 }
}

/// ffprobe's view of `path`, or why it gave none. A missing file is an error, a file that holds
/// no video another; either way the C++ returned a default `VideoInfo`.
pub fn probe_video(path: &str) -> Result<VideoInfo, String> {
    let name = FileInfo::new(path).file_name();
    if !FileInfo::new(path).is_file() {
        return Err(format!("{path} does not exist"));
    }
    // FFmpeg's libraries answer in process; a file they can't read goes to ffprobe, whose
    // message the error carries.
    if let Ok(probe) = crate::media_av::probe(path) {
        return video_info_from(&name, &probe);
    }
    let mut process = Process::new();
    process.start(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,r_frame_rate,avg_frame_rate,duration,\
             sample_aspect_ratio:stream_side_data=rotation:stream_tags=rotate:format=duration",
            "-of",
            "json",
            path,
        ],
    );
    if !process.wait_for_finished(15000) || process.exit_code() != 0 {
        if process.state() != ProcessState::NotRunning {
            process.kill();
        }
        let details = qstring::from_utf8(&process.read_all_standard_error());
        return Err(format!("Could not read {name}: {}", qstring::trimmed(&details)));
    }
    let root = qjson::object_from_json(&process.read_all_standard_output()).unwrap_or_default();
    let mut info = VideoInfo::default();
    let mut found_video = false;
    for entry in root.value("streams").to_array() {
        let stream = entry.to_object();
        let kind = stream.value("codec_type").to_string_or("");
        if kind == "audio" {
            info.audio = true;
        }
        if kind != "video" || found_video {
            continue;
        }
        found_video = true;
        let mut width = f64::from(stream.value("width").to_int(0));
        let height = f64::from(stream.value("height").to_int(0));
        let sample_aspect = ratio(&stream.value("sample_aspect_ratio").to_string_or(""));
        if sample_aspect > 0.0 {
            width *= sample_aspect;
        }
        // Phones record upright video as landscape frames with a rotation; ffmpeg turns them.
        let mut rotation =
            qnumber::to_int(&stream.value("tags").to_object().value("rotate").to_string_or("")).unwrap_or(0);
        for side in stream.value("side_data_list").to_array() {
            let side: Object = side.to_object();
            if side.contains("rotation") {
                rotation = side.value("rotation").to_int(0);
            }
        }
        let quarter_turn = rotation.wrapping_abs() % 180 == 90;
        info.width = q_round(if quarter_turn { height } else { width });
        info.height = q_round(if quarter_turn { width } else { height });
        let average = ratio(&stream.value("avg_frame_rate").to_string_or(""));
        let nominal = ratio(&stream.value("r_frame_rate").to_string_or(""));
        info.fps = if average > 0.0 && average <= 240.0 {
            average
        } else if nominal > 0.0 {
            nominal
        } else {
            30.0
        };
        info.duration = to_double(&stream.value("duration").to_string_or(""));
    }
    if info.duration <= 0.0 {
        info.duration = to_double(&root.value("format").to_object().value("duration").to_string_or(""));
    }
    if !info.is_valid() {
        return Err(if found_video { format!("{name} has no playable video") } else { format!("{name} is not a video") });
    }
    Ok(info)
}

/// The length in seconds of a file's sound (an audio file, or a video whose sound is used),
/// or why it has none to use: the first audio stream's own length, which export plays and
/// loops (a video's sound can end before its picture), found from its packets when the file
/// gives none in its header (a WebM written to a pipe, as browsers record).
pub fn probe_audio(path: &str) -> Result<f64, String> {
    let name = FileInfo::new(path).file_name();
    if !FileInfo::new(path).is_file() {
        return Err(format!("{path} does not exist"));
    }
    let duration = match crate::media_av::audio_duration(path) {
        Ok(duration) => duration,
        Err(_) => probe_audio_with_ffprobe(path, &name)?,
    };
    match duration {
        None => Err(format!("{name} has no sound")),
        Some(duration) if duration > 0.0 => Ok(duration),
        Some(_) => Err(format!("{name} has no playable sound")),
    }
}

/// ffprobe's output for `arguments` on `path`, or its error as [`probe_audio`] gives it.
fn ffprobe(arguments: &[&str], path: &str, name: &str, timeout_ms: i32) -> Result<Vec<u8>, String> {
    let mut process = Process::new();
    let mut all: Vec<&str> = vec!["-v", "error"];
    all.extend_from_slice(arguments);
    all.push(path);
    process.start("ffprobe", &all);
    if !process.wait_for_finished(timeout_ms) || process.exit_code() != 0 {
        if process.state() != ProcessState::NotRunning {
            process.kill();
        }
        let details = qstring::from_utf8(&process.read_all_standard_error());
        return Err(format!("Could not read {name}: {}", qstring::trimmed(&details)));
    }
    Ok(process.read_all_standard_output())
}

/// [`crate::media_av::audio_duration`] from ffprobe, for when the libraries can't read the
/// file: the stream's length, else its packets' extent.
fn probe_audio_with_ffprobe(path: &str, name: &str) -> Result<Option<f64>, String> {
    let output = ffprobe(&["-select_streams", "a:0", "-show_entries", "stream=duration", "-of", "json"], path, name, 15000)?;
    let root = qjson::object_from_json(&output).unwrap_or_default();
    let streams = root.value("streams").to_array();
    let Some(stream) = streams.first() else { return Ok(None) };
    let duration = to_double(&stream.to_object().value("duration").to_string_or(""));
    if duration > 0.0 {
        return Ok(Some(duration));
    }
    let output = ffprobe(
        &["-select_streams", "a:0", "-show_entries", "packet=pts_time,duration_time", "-of", "csv=p=0"],
        path,
        name,
        60000,
    )?;
    Ok(Some(packets_extent(&output)))
}

/// From the first packet's start to the last one's end, in ffprobe's `pts_time,duration_time`
/// lines; 0 when there are none.
fn packets_extent(csv: &[u8]) -> f64 {
    let (mut first, mut end) = (f64::INFINITY, f64::NEG_INFINITY);
    for line in csv.split(|&b| b == b'\n') {
        let mut fields = line.split(|&b| b == b',');
        let Some(at) = fields.next().and_then(|field| qnumber::byte_array_to_double(qfile::trimmed(field))) else {
            continue;
        };
        let length = fields.next().and_then(|field| qnumber::byte_array_to_double(qfile::trimmed(field))).unwrap_or(0.0);
        first = first.min(at);
        end = end.max(at + length);
    }
    if first < end { end - first } else { 0.0 }
}

/// [`probe_video`]'s reading of what the libraries found, as it reads ffprobe's JSON.
fn video_info_from(name: &str, probe: &crate::media_av::Probe) -> Result<VideoInfo, String> {
    let mut info = VideoInfo { audio: probe.has_audio, ..VideoInfo::default() };
    if probe.has_video {
        let mut width = f64::from(probe.width);
        let height = f64::from(probe.height);
        if probe.sample_aspect > 0.0 {
            width *= probe.sample_aspect;
        }
        let mut rotation = probe.rotate_tag.as_deref().and_then(qnumber::to_int).unwrap_or(0);
        if let Some(turned) = probe.rotation {
            rotation = turned;
        }
        let quarter_turn = rotation.wrapping_abs() % 180 == 90;
        info.width = q_round(if quarter_turn { height } else { width });
        info.height = q_round(if quarter_turn { width } else { height });
        let (average, nominal) = (probe.average_frame_rate, probe.frame_rate);
        info.fps = if average > 0.0 && average <= 240.0 {
            average
        } else if nominal > 0.0 {
            nominal
        } else {
            30.0
        };
        info.duration = probe.stream_duration.unwrap_or(0.0);
    }
    if info.duration <= 0.0 {
        info.duration = probe.format_duration.unwrap_or(0.0);
    }
    if !info.is_valid() {
        return Err(if probe.has_video { format!("{name} has no playable video") } else { format!("{name} is not a video") });
    }
    Ok(info)
}

/// When each frame of the first video stream starts, in seconds and in order; empty when the file
/// can't be read. Native screen captures only hold a frame where the screen changed. Read in
/// process (from an MP4's index, without reading the file), else by ffprobe over every packet.
pub fn probe_frame_times(path: &str) -> Vec<f64> {
    if !FileInfo::new(path).is_file() {
        return Vec::new();
    }
    if let Some(times) = crate::media_av::frame_times(path) {
        return times;
    }
    let mut process = Process::new();
    process.start(
        "ffprobe",
        &["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time", "-of", "csv=p=0", path],
    );
    if !process.wait_for_finished(60000) || process.exit_code() != 0 {
        if process.state() != ProcessState::NotRunning {
            process.kill();
        }
        return Vec::new();
    }
    let mut times: Vec<f64> = process
        .read_all_standard_output()
        .split(|&b| b == b'\n')
        .filter_map(|line| qnumber::byte_array_to_double(qfile::trimmed(line)))
        .collect();
    // std::sort compares with <, so equal times (0 and -0) and NaN, which compares equal to
    // everything, are not moved past each other; libstdc++ sorts lists this short by insertion,
    // which keeps them in order, as a stable sort does.
    // A NaN would make that comparison inconsistent, which Rust's sort may panic on. ffprobe
    // never prints one; should it, it sorts last instead of aborting.
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or_else(|| a.is_nan().cmp(&b.is_nan())));
    times
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qstandardpaths::find_executable;

    #[test]
    fn packets_extent_spans_first_start_to_last_end() {
        assert!(close(packets_extent(b"0.000000,0.020000\n0.020000,0.020000\n2.994000,0.007000,\n"), 3.001, 1e-9));
        assert_eq!(packets_extent(b"0.500000,0.500000\n0.000000,0.500000\n"), 1.0);
        assert_eq!(packets_extent(b""), 0.0);
        assert_eq!(packets_extent(b"N/A,N/A\n"), 0.0);
    }

    /// Runs ffmpeg with `args` (after `-y -v error`), writing to `output` or, for a pipe, to
    /// its standard output saved there.
    fn ffmpeg(args: &[&str], output: &std::path::Path, pipe: bool) {
        let output_text = output.to_string_lossy().into_owned();
        let mut all = vec!["-y", "-v", "error"];
        all.extend_from_slice(args);
        all.push(if pipe { "-" } else { &output_text });
        let finished = crate::compat::qprocess::run("ffmpeg", &all, 60000).unwrap();
        assert!(finished.succeeded(), "{}", String::from_utf8_lossy(&finished.stderr));
        if pipe {
            std::fs::write(output, &finished.stdout).unwrap();
        }
    }

    fn close(actual: f64, expected: f64, within: f64) -> bool {
        (actual - expected).abs() <= within
    }

    #[test]
    fn audio_length_is_the_sound_streams_own_found_without_a_header() {
        if find_executable("ffmpeg").is_empty() || find_executable("ffprobe").is_empty() {
            eprintln!("ffmpeg unavailable");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-probe-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();

        // A 3 s video whose sound lasts 1 s: the sound's 1 s, as export loops it.
        let video = dir.join("short-sound.mp4");
        ffmpeg(
            &[
                "-f", "lavfi", "-i", "testsrc=duration=3:size=160x120:rate=30", "-f", "lavfi", "-i",
                "sine=frequency=440:duration=1", "-c:v", "libx264", "-c:a", "aac",
            ],
            &video,
            false,
        );
        // A WebM written to a pipe, as browsers record: no length in its header.
        let webm = dir.join("piped.webm");
        ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-c:a", "libopus", "-f", "webm"], &webm, true);
        // An MP3 without its Xing header, whose length ffmpeg only guesses from the bitrate:
        // 2 s of near silence and 2 s of noise at a variable bitrate.
        let mp3 = dir.join("vbr.mp3");
        ffmpeg(
            &[
                "-f", "lavfi", "-i", "aevalsrc='if(lt(t,2),0.0001*sin(440*2*PI*t),random(0)-0.5)':s=44100:d=4", "-c:a",
                "libmp3lame", "-q:a", "4", "-write_xing", "0",
            ],
            &mp3,
            false,
        );
        let silent = dir.join("silent.mp4");
        ffmpeg(&["-f", "lavfi", "-i", "testsrc=duration=1:size=160x120:rate=30", "-c:v", "libx264"], &silent, false);

        let length = probe_audio(&path("short-sound.mp4")).unwrap();
        assert!(close(length, 1.0, 0.05), "{length}");
        let length = probe_audio(&path("piped.webm")).unwrap();
        assert!(close(length, 3.0, 0.03), "{length}");
        let length = probe_audio(&path("vbr.mp3")).unwrap();
        assert!(close(length, 4.0, 0.06), "{length}");
        assert_eq!(probe_audio(&path("silent.mp4")), Err("silent.mp4 has no sound".into()));

        // ffprobe, for files the libraries can't read, finds the same.
        let length = probe_audio_with_ffprobe(&path("short-sound.mp4"), "").unwrap().unwrap();
        assert!(close(length, 1.0, 0.05), "{length}");
        let length = probe_audio_with_ffprobe(&path("piped.webm"), "").unwrap().unwrap();
        assert!(close(length, 3.0, 0.03), "{length}");
        assert_eq!(probe_audio_with_ffprobe(&path("silent.mp4"), "").unwrap(), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
