//! A live webcam for the launcher's preview and the recording bar's self-view bubble: ffmpeg
//! reads the V4L2 device and hands over NV12 frames, which go to a `rune::NativeSurface` (Rune
//! converts them to RGB in its video shader), as the preview's decoders do.
//!
//! The Qt app reads cameras through Qt Multimedia (`src/plumbing/CameraCapture.cpp`); its pure
//! decisions (which format, which `/dev/videoN`, the stable by-id path) are in
//! `omareel_record::camera`, and the device facts come from [`devices`] here.
//!
//! Recording the camera to `camera.mp4` while it is shown is the recording bar's job
//! (`recording_bar`), which can start a second ffmpeg output on the same capture: see
//! [`LiveSpec::record_to`].

use rune::NativeSurface;
use rune::native_surface::{ByteFrame, ColorInfo, Format, Matrix, Transfer};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};

/// One V4L2 capture device and the formats it offers (what `omareel_record::camera` decides on).
#[derive(Clone, Debug)]
pub struct Device {
    /// `/dev/videoN`.
    pub node: String,
    /// The stable path settings keep (`/dev/v4l/by-id/...` or by-path), from
    /// `omareel_record::camera::stable_camera_path`.
    pub stable_path: String,
    /// The driver's card name ("ASUS FHD webcam: ASUS FHD webcam").
    pub name: String,
    pub formats: Vec<omareel_record::camera::CameraFormat>,
}

impl Device {
    /// As `omareel_record::camera` takes it.
    pub fn record_device(&self) -> omareel_record::camera::CameraDevice {
        omareel_record::camera::CameraDevice { id: self.node.as_bytes().to_vec(), description: self.name.clone(), formats: self.formats.clone() }
    }

    /// Whether it records (has a 30 fps format): IR sensors beside a laptop webcam don't.
    pub fn can_record(&self) -> bool {
        omareel_record::camera::camera_can_record(&self.record_device())
    }
}

/// The video capture devices, from /sys/class/video4linux and `ffmpeg -list_formats`. Nodes that
/// only carry metadata (no formats) are left out.
pub fn devices() -> Vec<Device> {
    let mut nodes: Vec<String> = std::fs::read_dir("/sys/class/video4linux")
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("video")).collect())
        .unwrap_or_default();
    nodes.sort_by_key(|n| n.trim_start_matches("video").parse::<u32>().unwrap_or(u32::MAX));
    nodes
        .into_iter()
        .filter_map(|n| {
            let node = format!("/dev/{n}");
            let name = std::fs::read_to_string(format!("/sys/class/video4linux/{n}/name")).unwrap_or_default().trim().to_owned();
            let formats = formats(&node);
            (!formats.is_empty()).then(|| Device { stable_path: omareel_record::camera::stable_camera_path(&node), node, name, formats })
        })
        .collect()
}

/// The device's formats as `ffmpeg -f v4l2 -list_formats all` prints them; frame rates are not
/// listed there, so each size counts as 30 fps (what UVC webcams offer for every size they list;
/// `omareel_record::camera::selected_format` needs 30).
fn formats(node: &str) -> Vec<omareel_record::camera::CameraFormat> {
    let Ok(out) = Command::new("ffmpeg").args(["-hide_banner", "-f", "v4l2", "-list_formats", "all", "-i", node]).stdin(Stdio::null()).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stderr);
    let mut result = Vec::new();
    for line in text.lines() {
        // [video4linux2,v4l2 @ 0x...] Compressed:       mjpeg :          Motion-JPEG : 1920x1080 1280x720 ...
        let Some(rest) = line.split(']').nth(1) else { continue };
        let parts: Vec<&str> = rest.split(':').map(str::trim).collect();
        if parts.len() < 4 || !(parts[0].starts_with("Compressed") || parts[0].starts_with("Raw")) {
            continue;
        }
        for size in parts[3].split_whitespace() {
            if let Some((w, h)) = size.split_once('x')
                && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
            {
                result.push(omareel_record::camera::CameraFormat { pixel_format: parts[1].to_owned(), width: w, height: h, min_frame_rate: 30.0, max_frame_rate: 30.0 });
            }
        }
    }
    result
}

/// What to capture.
#[derive(Clone, Debug)]
pub struct LiveSpec {
    /// The device node or a stable path (`/dev/v4l/by-id/...`).
    pub device: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// The camera's pixel format as ffmpeg names it ("mjpeg", "yuyv422"), or "" for its default.
    pub input_format: String,
    /// Extra ffmpeg output arguments after the preview's, for recording the same capture (e.g.
    /// `["-map", "0:v", "-c:v", "libx264", ..., "/path/camera.mp4"]`); empty for a preview only.
    pub record_to: Vec<String>,
}

/// A running capture: frames for the screen, and the ffmpeg behind them.
pub struct Live {
    frames: Receiver<ByteFrame>,
    stop: Arc<AtomicBool>,
    child: Arc<std::sync::Mutex<Option<Child>>>,
    /// Frames handed to the surface so far.
    pub shown: u64,
    /// Set when ffmpeg ended or could not start; its last error line.
    pub error: Option<String>,
    errors: Option<Receiver<String>>,
}

impl Live {
    /// Starts ffmpeg on the device. Frames arrive on a thread; [`Live::pump`] hands the newest to
    /// a surface.
    pub fn start(spec: LiveSpec) -> Live {
        let (send, frames) = sync_channel::<ByteFrame>(2);
        let stop = Arc::new(AtomicBool::new(false));
        let child_slot: Arc<std::sync::Mutex<Option<Child>>> = Arc::new(std::sync::Mutex::new(None));
        let (thread_stop, thread_child) = (stop.clone(), child_slot.clone());
        let (error_send, error_receive) = sync_channel::<String>(1);
        let mut command = Command::new("ffmpeg");
        command.args(["-v", "error", "-nostdin", "-f", "v4l2"]);
        if !spec.input_format.is_empty() {
            command.args(["-input_format", &spec.input_format]);
        }
        command.args(["-video_size", &format!("{}x{}", spec.width, spec.height), "-framerate", &spec.fps.to_string(), "-i", &spec.device]);
        command.args(["-map", "0:v", "-f", "rawvideo", "-pix_fmt", "nv12", "pipe:1"]);
        command.args(&spec.record_to);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let (w, h) = (spec.width, spec.height);
        std::thread::Builder::new()
            .name("camera".into())
            .spawn(move || {
                let mut child = match command.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = error_send.try_send(format!("could not start ffmpeg: {e}"));
                        return;
                    }
                };
                let mut out = child.stdout.take().expect("piped stdout");
                let mut err = child.stderr.take();
                *thread_child.lock().unwrap() = Some(child);
                let color = ColorInfo { matrix: Matrix::Bt601, transfer: Transfer::Bt709 };
                loop {
                    let mut y = vec![0u8; (w * h) as usize];
                    let mut uv = vec![0u8; (w * h / 2) as usize];
                    if out.read_exact(&mut y).is_err() || out.read_exact(&mut uv).is_err() {
                        break;
                    }
                    if thread_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    // The newest frame wins: a full queue drops this one.
                    let _ = send.try_send(ByteFrame { width: w, height: h, format: Format::Nv12Video, color, planes: [y, uv] });
                }
                if !thread_stop.load(Ordering::SeqCst) {
                    let mut message = String::new();
                    if let Some(e) = err.as_mut() {
                        let _ = e.read_to_string(&mut message);
                    }
                    let _ = error_send.try_send(message.lines().last().unwrap_or("camera stopped").to_owned());
                }
            })
            .expect("camera thread");
        Live { frames, stop, child: child_slot, shown: 0, error: None, errors: Some(error_receive) }
    }

    /// Hands the newest frame to `surface`. True when a frame was shown.
    pub fn pump(&mut self, surface: &NativeSurface) -> bool {
        if let Some(errors) = &self.errors
            && let Ok(e) = errors.try_recv()
        {
            self.error = Some(e);
        }
        let mut newest = None;
        loop {
            match self.frames.try_recv() {
                Ok(f) => newest = Some(f),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        let Some(frame) = newest else { return false };
        self.shown += 1;
        surface.push_bytes(frame, self.shown).is_ok()
    }

    /// Stops ffmpeg gracefully (`q` is not available without stdin, so SIGINT: ffmpeg finishes
    /// the files it writes) and waits up to `timeout` for it.
    pub fn stop(&mut self, timeout: std::time::Duration) {
        self.stop.store(true, Ordering::SeqCst);
        let Some(mut child) = self.child.lock().unwrap().take() else { return };
        unsafe {
            libc::kill(child.id() as i32, libc::SIGINT);
        }
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop(std::time::Duration::from_millis(1500));
    }
}
