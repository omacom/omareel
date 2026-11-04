//! One ffmpeg child decoding a stretch of a video to raw RGBA frames (`FfmpegDecoder`): on the
//! GPU first (CUDA, or VA-API when asked for: see [`hardware`]), and in software when the GPU
//! gives no frame at all.

use crate::compat::cpp;
use crate::compat::qdir::FileInfo;
use crate::compat::qnumber::number_f64;
use crate::compat::qprocess::{Process, ProcessState};
use crate::compat::qstring;
use crate::export::hardware::{self, HwAccel};

pub struct FfmpegDecoder {
    process: Process,
    path: String,
    start: f64,
    length: f64,
    fps: f64,
    width: i32,
    height: i32,
    scale: bool,
    untagged_bt601: bool,
    /// What to try on the GPU, and how many of those have failed: all of them means software.
    accelerations: Vec<HwAccel>,
    attempt: usize,
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
            untagged_bt601: false,
            accelerations: hardware::decode_accelerations(),
            attempt: 0,
            delivered_frame: false,
            eof: false,
            cancelled: false,
        }
    }

    /// Starts decoding `length` seconds of `path` from `start_time` at `fps`, as `width` x
    /// `height` frames (scaled to that size when `scale`). Stops whatever was decoding before.
    /// On failure `error` says why; otherwise it is left alone.
    ///
    /// The frames are converted to RGB with the matrix the file is tagged with. `untagged_bt601`
    /// says that `path` carries no matrix and was converted from RGB with BT.601, as swscale does
    /// when nothing says otherwise: the recordings before colour tagging. ffmpeg assumes the same
    /// for an untagged file today; saying so keeps it that way.
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
        untagged_bt601: bool,
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
        self.untagged_bt601 = untagged_bt601;
        self.attempt = 0;
        self.delivered_frame = false;
        self.eof = false;
        if !FileInfo::new(path).is_file() || width <= 0 || height <= 0 || fps <= 0.0 || length <= 0.0 {
            *error = "Invalid decoder input".into();
            return false;
        }
        self.launch(error)
    }

    /// Starts ffmpeg with the acceleration `attempt` picks, or in software after the last.
    fn launch(&mut self, error: &mut String) -> bool {
        let mut args: Vec<String> = vec!["-v".into(), "error".into()];
        if let Some(acceleration) = self.accelerations.get(self.attempt) {
            args.extend(acceleration.arguments());
        }
        // Screen recordings only have frames where the screen changed, so a clip can start
        // seconds after its last frame, or hold none at all. Accurate seeking drops that frame
        // and the fps filter would then start at the next one, shifting the clip early or leaving
        // it empty. Decoding from the keyframe before the start and anchoring the output grid at
        // the start shows the frame on screen at each instant.
        let mut video_filter = format!("fps={}:start_time=0", number_f64(self.fps, 'g', 12));
        if self.scale {
            video_filter += &format!(",format=rgba,scale={}:{}:flags=bilinear", self.width, self.height);
        }
        // The conversion to RGBA reads the matrix from the frames; an untagged file's are told
        // theirs. A tagged file keeps its own, whatever it is.
        if self.untagged_bt601 {
            video_filter = format!("setparams=colorspace=bt470bg,{video_filter}");
        }
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
        true
    }

    /// What decodes now: "cuda", "vaapi" or "software".
    pub fn acceleration(&self) -> &'static str {
        self.accelerations.get(self.attempt).map_or("software", HwAccel::name)
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
            if self.attempt < self.accelerations.len() && !self.delivered_frame && !self.cancelled {
                self.process.kill();
                self.process.wait_for_finished(30000);
                self.attempt += 1;
                if !self.launch(error) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qprocess::run;
    use crate::compat::qstandardpaths::find_executable;
    use crate::export::plan;

    /// A losslessly encoded red, converted with `matrix` and tagged with it or not at all.
    fn red(path: &str, matrix: &str, tagged: bool) -> bool {
        let tags = if tagged {
            format!(",setparams=colorspace={}:color_primaries=bt709:color_trc=bt709:range=tv",
                    if matrix == "bt601" { "bt470bg" } else { matrix })
        } else {
            ",setparams=colorspace=unknown:color_primaries=unknown:color_trc=unknown:range=unknown".into()
        };
        let filter = format!("scale=out_color_matrix={matrix}:out_range=tv,format=yuv420p{tags}");
        run(
            "ffmpeg",
            &["-y", "-v", "error", "-f", "lavfi", "-i", "color=c=red:size=64x64:rate=30,format=rgba", "-frames:v", "3",
              "-vf", &filter, "-c:v", "libx264", "-qp", "0", path],
            30000,
        )
        .is_some_and(|finished| finished.succeeded())
    }

    // Old recordings carry no matrix and were made with BT.601; new ones say BT.709. Each has to
    // decode as the red it was.
    #[test]
    fn decodes_tagged_and_untagged_recordings_with_their_matrix() {
        if find_executable("ffmpeg").is_empty() || find_executable("ffprobe").is_empty() {
            eprintln!("ffmpeg/ffprobe unavailable");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-decoder-colour-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, matrix, tagged) in [("untagged601", "bt601", false), ("tagged709", "bt709", true), ("tagged601", "bt601", true)] {
            let path = dir.join(format!("{name}.mp4")).to_string_lossy().into_owned();
            assert!(red(&path, matrix, tagged), "{name}");
            let (media, _) = plan::probe(&path);
            assert_eq!(media.colour_tagged, tagged, "{name}");
            for scale in [false, true] {
                let mut decoder = FfmpegDecoder::new();
                decoder.accelerations.clear();
                let mut error = String::new();
                let (width, height) = if scale { (32, 32) } else { (64, 64) };
                assert!(decoder.start(&path, 0.0, 0.1, 30.0, width, height, scale, !media.colour_tagged, &mut error), "{error}");
                let frame = decoder.read_frame(&mut error).unwrap_or_else(|| panic!("{name}: {error}"));
                let centre = ((height / 2 * width + width / 2) * 4) as usize;
                let rgb = &frame[centre..centre + 3];
                assert!(rgb[0] >= 250 && rgb[1] <= 3 && rgb[2] <= 3, "{name} (scaled {scale}): {rgb:?}");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
