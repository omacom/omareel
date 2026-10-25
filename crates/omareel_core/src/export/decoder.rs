//! One ffmpeg child decoding a stretch of a video to raw RGBA frames (`FfmpegDecoder`): CUDA
//! first, and in software when CUDA gives no frame at all.

use crate::compat::cpp;
use crate::compat::qdir::FileInfo;
use crate::compat::qnumber::number_f64;
use crate::compat::qprocess::{Process, ProcessState};
use crate::compat::qstring;

pub struct FfmpegDecoder {
    process: Process,
    path: String,
    start: f64,
    length: f64,
    fps: f64,
    width: i32,
    height: i32,
    scale: bool,
    hardware: bool,
    delivered_frame: bool,
    eof: bool,
    cancelled: bool,
}

impl Default for FfmpegDecoder {
    fn default() -> FfmpegDecoder {
        FfmpegDecoder::new()
    }
}

impl FfmpegDecoder {
    pub fn new() -> FfmpegDecoder {
        FfmpegDecoder {
            process: Process::new(),
            path: String::new(),
            start: 0.0,
            length: 0.0,
            fps: 60.0,
            width: 0,
            height: 0,
            scale: false,
            hardware: true,
            delivered_frame: false,
            eof: false,
            cancelled: false,
        }
    }

    /// Starts decoding `length` seconds of `path` from `start_time` at `fps`, as `width` x
    /// `height` frames (scaled to that size when `scale`). Stops whatever was decoding before.
    /// On failure `error` says why; otherwise it is left alone.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        path: &str,
        start_time: f64,
        length: f64,
        fps: f64,
        width: i32,
        height: i32,
        scale: bool,
        error: &mut String,
    ) -> bool {
        self.cancel();
        self.cancelled = false;
        self.path = path.to_owned();
        self.start = cpp::max(0.0, start_time);
        self.length = cpp::max(0.0, length);
        self.fps = fps;
        self.width = width;
        self.height = height;
        self.scale = scale;
        self.hardware = true;
        self.delivered_frame = false;
        self.eof = false;
        if !FileInfo::new(path).is_file() || width <= 0 || height <= 0 || fps <= 0.0 || length <= 0.0 {
            *error = "Invalid decoder input".into();
            return false;
        }
        self.launch(true, error)
    }

    fn launch(&mut self, hardware: bool, error: &mut String) -> bool {
        let mut args: Vec<String> = vec!["-v".into(), "error".into()];
        if hardware {
            args.extend(["-hwaccel".into(), "cuda".into()]);
        }
        // Screen recordings only have frames where the screen changed, so a clip can start
        // seconds after its last frame, or hold none at all. Accurate seeking drops that frame
        // and the fps filter would then start at the next one, shifting the clip early or leaving
        // it empty. Decoding from the keyframe before the start and anchoring the output grid at
        // the start shows the frame on screen at each instant.
        let video_filter = if self.scale {
            format!(
                "fps={}:start_time=0,format=rgba,scale={}:{}:flags=bilinear",
                number_f64(self.fps, 'g', 12),
                self.width,
                self.height
            )
        } else {
            format!("fps={}:start_time=0", number_f64(self.fps, 'g', 12))
        };
        args.extend([
            "-ss".into(),
            number_f64(self.start, 'f', 6),
            "-noaccurate_seek".into(),
            "-i".into(),
            self.path.clone(),
            "-t".into(),
            number_f64(self.length, 'f', 6),
            "-vf".into(),
            video_filter,
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-".into(),
        ]);
        self.process.start("ffmpeg", &args);
        if !self.process.wait_for_started(5000) {
            *error = self.process.error_string();
            return false;
        }
        self.hardware = hardware;
        true
    }

    /// The next frame, tightly packed RGBA rows, or `None` at the end or on a failure. `error`
    /// is cleared with a frame, set when ffmpeg said why it stopped, and otherwise left alone.
    pub fn read_frame(&mut self, error: &mut String) -> Option<Vec<u8>> {
        if self.cancelled {
            *error = "Export cancelled".into();
            return None;
        }
        let needed = i64::from(self.width) * i64::from(self.height) * 4;
        let mut next = vec![0u8; usize::try_from(needed).unwrap_or(0)];
        let mut received = 0usize;
        while received < next.len() && !self.cancelled {
            if self.process.bytes_available() == 0 && !self.process.wait_for_ready_read(30000) {
                if self.process.state() == ProcessState::NotRunning {
                    break;
                }
                *error = "Timed out reading decoded video".into();
                return None;
            }
            received += self.process.read(&mut next[received..]);
        }
        if received != next.len() {
            let details = qstring::from_utf8(&self.process.read_all_standard_error());
            let details = qstring::trimmed(&details);
            if self.hardware && !self.delivered_frame && !self.cancelled {
                self.process.kill();
                self.process.wait_for_finished(30000);
                if !self.launch(false, error) {
                    return None;
                }
                return self.read_frame(error);
            }
            self.eof = true;
            if !details.is_empty() {
                *error = details.to_owned();
            }
            return None;
        }
        self.delivered_frame = true;
        error.clear();
        Some(next)
    }

    /// Stops decoding: kills ffmpeg and waits up to two seconds for it.
    pub fn cancel(&mut self) {
        self.cancelled = true;
        if self.process.state() != ProcessState::NotRunning {
            self.process.kill();
            self.process.wait_for_finished(2000);
        }
    }

    /// The size of the frames `read_frame` gives.
    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn at_end(&self) -> bool {
        self.eof
    }
}

impl Drop for FfmpegDecoder {
    fn drop(&mut self) {
        self.cancel();
    }
}
