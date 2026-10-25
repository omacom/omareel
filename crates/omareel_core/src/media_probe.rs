//! What a video file holds, from ffprobe (`MediaProbe`): inserted clips' size, length, frame rate
//! and audio, and the frame times of a recording.

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

/// When each frame of the first video stream starts, in seconds and in order; empty when the file
/// can't be read. Native screen captures only hold a frame where the screen changed.
pub fn probe_frame_times(path: &str) -> Vec<f64> {
    if !FileInfo::new(path).is_file() {
        return Vec::new();
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
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    times
}
