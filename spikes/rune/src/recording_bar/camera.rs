//! The camera the recording bar shows and records: `src/plumbing/CameraCapture.cpp` (and the
//! parts of `RecordingBarBase` that own it) over ffmpeg instead of Qt Multimedia.
//!
//! - Capture: ffmpeg reads the V4L2 device in the format `omareel_record::camera::selected_format`
//!   picks (30 fps, closest to 1080p or 720p, MJPEG first), decodes it and hands NV12 (BT.601,
//!   limited range) frames over a pipe. A `showinfo` filter prints each frame's driver timestamp
//!   (`-copyts`: uvcvideo's `CLOCK_MONOTONIC` time as the frame started to arrive), read from
//!   stderr and matched to the frame by its index.
//! - Times: `CameraFrameClock`. The Qt sink's `startTime()` is relative to the stream's first
//!   frame, so Qt anchors it at the least delay seen; here the driver time is already monotonic,
//!   so a frame takes it as it is (when it lies at most ten seconds before its arrival), else the
//!   Qt clock runs on arrival. Times never exceed arrival and always increase by a millisecond.
//! - Preview: the centre square, turned and mirrored as the bubble shows it (the QML turns and
//!   mirrors the VideoOutput; Rune's `NativeSurface` stays upright under `rotate`), into a
//!   `rune::Pixels` the bubble draws.
//! - Recording: from `begin_recording`, each frame's time goes to the first-frame timestamp file
//!   (`camera.mp4.ts`, `omareel_record::camera::FirstFrameTimestamp`) and the frame to the encoder
//!   thread (`CameraEncoder`): raw frames with their times as uncompressed Matroska piped to
//!   ffmpeg, which writes fragmented, BT.709-tagged, VFR H.264 (NVENC when the screen recorder's
//!   cached probe found it) with `cameraEncoderArguments`' exact arguments.

use omareel_record::camera::{CameraDevice, CameraFormat, FirstFrameTimestamp, camera_node_path, selected_format};
use omareel_record::recorder::{monotonic_us, realtime_us};
use rune::Pixels;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::os::fd::AsRawFd;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// What `CameraCapture` signals: `readyChanged` (the first frame), `errorOccurred`, `stopped`.
#[derive(Debug)]
pub enum Event {
    Ready,
    Error(String),
    Stopped,
}

type Notify = Arc<dyn Fn(Event) + Send + Sync>;

struct Inner {
    device_path: String,
    requested_height: i32,
    video_path: String,
    timestamp: FirstFrameTimestamp,
    capture_size: (i32, i32),
    error: String,
    ready: bool,
    running: bool,
    stopping: bool,
    recording_frames: bool,
    encoder: Option<Arc<Encoder>>,
    encoder_thread: Option<JoinHandle<()>>,
    child: Option<Child>,
}

struct Shared {
    inner: Mutex<Inner>,
    notify: Notify,
    preview: Pixels,
    /// What the preview shows: the bubble's turn (degrees) and mirror, and whether it shows.
    rotation: AtomicI32,
    flip: AtomicBool,
    previewing: AtomicBool,
}

/// `CameraCapture`.
pub struct Camera {
    shared: Arc<Shared>,
}

impl Camera {
    /// `CameraCapture(devicePath, requestedHeight, videoPath, timestampPath)`: nothing runs
    /// until [`start`](Self::start).
    pub fn new(device_path: &str, requested_height: i32, video_path: &str, timestamp_path: &str, preview: Pixels, notify: Notify) -> Camera {
        let inner = Inner {
            device_path: device_path.to_owned(),
            requested_height: if requested_height == 720 { 720 } else { 1080 },
            video_path: video_path.to_owned(),
            timestamp: FirstFrameTimestamp::new(timestamp_path),
            capture_size: (-1, -1),
            error: String::new(),
            ready: false,
            running: false,
            stopping: false,
            recording_frames: false,
            encoder: None,
            encoder_thread: None,
            child: None,
        };
        let shared = Arc::new(Shared {
            inner: Mutex::new(inner),
            notify,
            preview,
            rotation: AtomicI32::new(0),
            flip: AtomicBool::new(false),
            previewing: AtomicBool::new(true),
        });
        Camera { shared }
    }

    pub fn ready(&self) -> bool {
        self.shared.inner.lock().unwrap().ready
    }

    /// `captureSize()`: -1 x -1 until a format was chosen.
    pub fn capture_size(&self) -> (i32, i32) {
        self.shared.inner.lock().unwrap().capture_size
    }

    /// `frameRate()`: the format is chosen for 30 fps.
    pub fn frame_rate(&self) -> f64 {
        30.0
    }

    /// How the preview is turned and mirrored, and whether it is drawn at all.
    pub fn set_preview(&self, rotation: i32, flip: bool, shown: bool) {
        self.shared.rotation.store(rotation, Ordering::Relaxed);
        self.shared.flip.store(flip, Ordering::Relaxed);
        self.shared.previewing.store(shown, Ordering::Relaxed);
    }

    /// `start()`: opens the camera (on a thread: finding its formats takes ffmpeg a moment).
    pub fn start(&self) {
        {
            let mut inner = self.shared.inner.lock().unwrap();
            if inner.running || inner.stopping {
                return;
            }
            inner.running = true;
        }
        // The preview would keep the previous camera's last frame otherwise.
        self.shared.preview.set(1, 1, vec![0, 0, 0, 0]);
        let shared = self.shared.clone();
        std::thread::Builder::new().name("camera".into()).spawn(move || capture(shared)).expect("camera thread");
        // Some webcams take seconds to stream after they open; the Insta360 Link 2 calibrates
        // first. Only a camera that stays dark well past that is treated as broken.
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("camera-start".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                while started.elapsed() < Duration::from_secs(10) {
                    std::thread::sleep(Duration::from_millis(100));
                    let inner = shared.inner.lock().unwrap();
                    if inner.ready || inner.stopping {
                        return;
                    }
                }
                fail(&shared, "Camera did not produce a frame within ten seconds");
            })
            .expect("camera start timer");
    }

    /// `setRecordingOutput`: where recording goes, until it has begun.
    pub fn set_recording_output(&self, video_path: &str, timestamp_path: &str) {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.recording_frames || inner.stopping {
            return;
        }
        inner.video_path = video_path.to_owned();
        inner.timestamp.set_path(timestamp_path);
    }

    /// `beginRecording`: frames from now on are recorded (may come before the first frame).
    pub fn begin_recording(&self) {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.stopping || inner.recording_frames {
            return;
        }
        inner.recording_frames = true;
    }

    /// `stop()`: the camera closes, and the recording finishes (`stopped` once it has).
    pub fn stop(&self) {
        stop(&self.shared);
    }
}

impl Drop for Camera {
    /// `~CameraCapture`: the camera closes and the encoder gets its deadline to finish the file.
    fn drop(&mut self) {
        let (encoder, thread) = {
            let mut inner = self.shared.inner.lock().unwrap();
            inner.stopping = true;
            inner.recording_frames = false;
            if let Some(mut child) = inner.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            (inner.encoder.clone(), inner.encoder_thread.take())
        };
        if let (Some(encoder), Some(thread)) = (encoder, thread) {
            encoder.request_stop();
            let _ = thread.join();
        }
    }
}

fn fail(shared: &Arc<Shared>, message: &str) {
    {
        let mut inner = shared.inner.lock().unwrap();
        if !inner.error.is_empty() {
            return;
        }
        inner.error = if message.is_empty() { "Camera capture failed".into() } else { message.to_owned() };
        eprintln!("{}", inner.error);
    }
    (shared.notify)(Event::Error(message.to_owned()));
    stop(shared);
}

fn stop(shared: &Arc<Shared>) {
    let encoder = {
        let mut inner = shared.inner.lock().unwrap();
        if inner.stopping {
            return;
        }
        inner.stopping = true;
        inner.recording_frames = false;
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.encoder.clone().filter(|e| !e.finished.load(Ordering::SeqCst))
    };
    match encoder {
        // The encoder's thread calls `finish_stop` when it is done.
        Some(encoder) => encoder.request_stop(),
        None => finish_stop(shared),
    }
}

/// `finishStop`: an encoder's error is the camera's, and the camera has stopped.
fn finish_stop(shared: &Arc<Shared>) {
    let error = {
        let mut inner = shared.inner.lock().unwrap();
        let encoder_error = inner.encoder.as_ref().map(|e| e.error()).unwrap_or_default();
        let report = !encoder_error.is_empty() && inner.error.is_empty();
        if report {
            inner.error = encoder_error.clone();
            eprintln!("{encoder_error}");
        }
        inner.stopping = true;
        inner.recording_frames = false;
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.running = false;
        report.then_some(encoder_error)
    };
    if let Some(error) = error {
        (shared.notify)(Event::Error(error));
    }
    (shared.notify)(Event::Stopped);
}

/// The device's formats as `ffmpeg -f v4l2 -list_formats all` lists them (as `crate::camera`
/// reads them: each size counts as 30 fps), compressed ones first, so MJPEG wins a tie with a
/// raw format that only streams a few frames a second at that size.
fn device(node: &str) -> CameraDevice {
    let output = Command::new("ffmpeg").args(["-hide_banner", "-f", "v4l2", "-list_formats", "all", "-i", node]).stdin(Stdio::null()).output();
    let text = output.map(|o| String::from_utf8_lossy(&o.stderr).into_owned()).unwrap_or_default();
    let mut compressed = Vec::new();
    let mut raw = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.split(']').nth(1) else { continue };
        let parts: Vec<&str> = rest.split(':').map(str::trim).collect();
        if parts.len() < 4 || !(parts[0].starts_with("Compressed") || parts[0].starts_with("Raw")) {
            continue;
        }
        let list = if parts[0].starts_with("Compressed") { &mut compressed } else { &mut raw };
        for size in parts[3].split_whitespace() {
            if let Some((w, h)) = size.split_once('x')
                && let (Ok(width), Ok(height)) = (w.parse(), h.parse())
            {
                list.push(CameraFormat { pixel_format: parts[1].to_owned(), width, height, min_frame_rate: 30.0, max_frame_rate: 30.0 });
            }
        }
    }
    compressed.extend(raw);
    CameraDevice { id: node.as_bytes().to_vec(), description: String::new(), formats: compressed }
}

/// One frame: NV12, the Y plane then the interleaved CbCr plane.
struct Frame {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

/// The driver times `showinfo` printed, by frame index.
#[derive(Default)]
struct Times {
    queue: Mutex<(VecDeque<(u64, i64)>, bool)>,
    arrived: Condvar,
}

impl Times {
    /// Frame `n`'s time, waiting a little for its line.
    fn take(&self, n: u64) -> Option<i64> {
        let mut guard = self.queue.lock().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_millis(100);
        loop {
            while let Some(&(index, _)) = guard.0.front()
                && index < n
            {
                guard.0.pop_front();
            }
            if let Some(&(index, time)) = guard.0.front()
                && index == n
            {
                guard.0.pop_front();
                return Some(time);
            }
            let now = std::time::Instant::now();
            if guard.1 || now >= deadline {
                return None;
            }
            guard = self.arrived.wait_timeout(guard, deadline - now).unwrap().0;
        }
    }
}

/// `n:   12 pts:1734 pts_time:1734.512345 ...` from showinfo: the index and the time in
/// microseconds.
fn showinfo_time(line: &str) -> Option<(u64, i64)> {
    if !line.contains("showinfo") {
        return None;
    }
    let after = |key: &str| line.find(key).map(|i| line[i + key.len()..].trim_start());
    let number = |s: &str| s.split(|c: char| c.is_whitespace()).next().unwrap_or("").to_owned();
    let n = number(after(" n:")?).parse().ok()?;
    let seconds: f64 = number(after("pts_time:")?).parse().ok()?;
    Some((n, (seconds * 1e6).round() as i64))
}

/// The capture thread: picks the format, runs ffmpeg and hands each frame on.
fn capture(shared: Arc<Shared>) {
    let (device_path, requested_height) = {
        let inner = shared.inner.lock().unwrap();
        (inner.device_path.clone(), inner.requested_height)
    };
    let node = camera_node_path(&device_path);
    let device = if node.is_empty() { CameraDevice::default() } else { device(&node) };
    if device.formats.is_empty() {
        fail(&shared, "Selected camera is unavailable");
        return;
    }
    let Some(format) = selected_format(&device, requested_height).cloned() else {
        fail(&shared, "Selected camera has no 30 fps capture format");
        return;
    };
    let (w, h) = (format.width as u32, format.height as u32);
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-nostats", "-loglevel", "info", "-nostdin", "-f", "v4l2", "-input_format", &format.pixel_format]);
    command.args(["-video_size", &format!("{w}x{h}"), "-framerate", "30", "-i", &node, "-copyts", "-map", "0:v"]);
    // The decoder says the range (MJPEG is full range); NV12 out is BT.601 limited range.
    command.args(["-vf", "showinfo,scale=out_color_matrix=bt601:out_range=tv,format=nv12", "-fps_mode", "passthrough"]);
    command.args(["-f", "rawvideo", "pipe:1"]);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            fail(&shared, &format!("Could not start the camera: {error}"));
            return;
        }
    };
    let mut out = child.stdout.take().expect("piped stdout");
    let err = child.stderr.take().expect("piped stderr");
    {
        let mut inner = shared.inner.lock().unwrap();
        if inner.stopping {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        inner.capture_size = (w as i32, h as i32);
        inner.child = Some(child);
    }
    let times = Arc::new(Times::default());
    let last_error = Arc::new(Mutex::new(String::new()));
    let stderr_thread = {
        let (times, last_error) = (times.clone(), last_error.clone());
        std::thread::Builder::new()
            .name("camera-log".into())
            .spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if let Some(time) = showinfo_time(&line) {
                        times.queue.lock().unwrap().0.push_back(time);
                        times.arrived.notify_all();
                    } else if !line.trim().is_empty() && !line.starts_with(' ') && !line.contains("@ 0x") {
                        *last_error.lock().unwrap() = line.trim().to_owned();
                    } else if line.contains("error") || line.contains("Error") {
                        *last_error.lock().unwrap() = line.trim().to_owned();
                    }
                }
                times.queue.lock().unwrap().1 = true;
                times.arrived.notify_all();
            })
            .expect("camera log thread")
    };
    let mut clock = FrameClock::default();
    let mut index = 0u64;
    let mut format_logged = false;
    loop {
        let mut bytes = vec![0u8; (w * h + w * h / 2) as usize];
        if out.read_exact(&mut bytes).is_err() {
            break;
        }
        let arrival = monotonic_us();
        let driver = times.take(index);
        index += 1;
        let time = clock.frame_time_us(driver, arrival);
        if !format_logged {
            format_logged = true;
            eprintln!("Camera frame: {} {w}x{h} (as nv12)", format.pixel_format);
        }
        let frame = Arc::new(Frame { width: w, height: h, bytes });
        if shared.previewing.load(Ordering::Relaxed) {
            let rotation = shared.rotation.load(Ordering::Relaxed);
            let flip = shared.flip.load(Ordering::Relaxed);
            let side = PREVIEW_SIDE.min(w.min(h));
            shared.preview.set(side, side, preview(&frame, side, rotation, flip));
        }
        let became_ready;
        let record;
        {
            let mut inner = shared.inner.lock().unwrap();
            if inner.stopping {
                break;
            }
            became_ready = !inner.ready;
            inner.ready = true;
            record = inner.recording_frames;
        }
        if became_ready {
            (shared.notify)(Event::Ready);
        }
        if !record {
            continue;
        }
        let realtime = realtime_us() - (arrival - time);
        let mut inner = shared.inner.lock().unwrap();
        if !inner.timestamp.record_frame_arrival(time, realtime) {
            drop(inner);
            fail(&shared, "Could not write camera timestamp");
            break;
        }
        if inner.encoder.is_none() {
            let encoder = Arc::new(Encoder::new(inner.video_path.clone()));
            encoder.submit(frame, time);
            let (thread_encoder, thread_shared) = (encoder.clone(), shared.clone());
            inner.encoder_thread = Some(
                std::thread::Builder::new()
                    .name("camera-encoder".into())
                    .spawn(move || {
                        thread_encoder.run();
                        thread_encoder.finished.store(true, Ordering::SeqCst);
                        finish_stop(&thread_shared);
                    })
                    .expect("camera encoder thread"),
            );
            inner.encoder = Some(encoder);
        } else if let Some(encoder) = &inner.encoder {
            encoder.submit(frame, time);
        }
    }
    let _ = stderr_thread.join();
    let stopping = shared.inner.lock().unwrap().stopping;
    if !stopping {
        let message = last_error.lock().unwrap().clone();
        fail(&shared, if message.is_empty() { "Camera stopped" } else { &message });
    }
}

/// The preview's side in pixels: the largest bubble (220 points) at scale 2.
const PREVIEW_SIDE: u32 = 448;

/// The frame's centre square at `side` x `side`, mirrored then turned `rotation` degrees
/// clockwise (the QML's `rotation` over its mirroring `Scale`), as straight-alpha sRGB.
fn preview(frame: &Frame, side: u32, rotation: i32, flip: bool) -> Vec<u8> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let square = w.min(h);
    let (ox, oy) = ((w - square) / 2, (h - square) / 2);
    let n = side as usize;
    let (y_plane, uv_plane) = frame.bytes.split_at(w * h);
    let mut rgba = vec![255u8; n * n * 4];
    let last = n - 1;
    for v in 0..n {
        for u in 0..n {
            // Undo the turn, then the mirror.
            let (x, y) = match rotation.rem_euclid(360) {
                90 => (v, last - u),
                180 => (last - u, last - v),
                270 => (last - v, u),
                _ => (u, v),
            };
            let x = if flip { last - x } else { x };
            let sx = ox + x * square / n;
            let sy = oy + y * square / n;
            let luma = i32::from(y_plane[sy * w + sx]) - 16;
            let uv = (sy / 2) * w + (sx / 2) * 2;
            let (cb, cr) = (i32::from(uv_plane[uv]) - 128, i32::from(uv_plane[uv + 1]) - 128);
            let c = 298 * luma + 128;
            let at = (v * n + u) * 4;
            rgba[at] = ((c + 409 * cr) >> 8).clamp(0, 255) as u8;
            rgba[at + 1] = ((c - 100 * cb - 208 * cr) >> 8).clamp(0, 255) as u8;
            rgba[at + 2] = ((c + 516 * cb) >> 8).clamp(0, 255) as u8;
        }
    }
    rgba
}

/// `CameraFrameClock`, taking the driver's time as it is when it is already monotonic.
#[derive(Default)]
struct FrameClock {
    offset_us: i64,
    has_offset: bool,
    last_start_us: Option<i64>,
    last_frame_us: i64,
    window_start_us: i64,
    window_offset_us: i64,
}

impl FrameClock {
    fn frame_time_us(&mut self, driver_us: Option<i64>, arrival_us: i64) -> i64 {
        let mut time = arrival_us;
        match driver_us {
            // uvcvideo's buffer time: CLOCK_MONOTONIC as the frame started to arrive.
            Some(driver) if driver <= arrival_us && arrival_us - driver < 10_000_000 => {
                time = driver;
                self.has_offset = false;
            }
            // Another clock: Qt's anchoring at the least delay seen.
            Some(start) if self.last_start_us.is_none_or(|last| start > last) => {
                // A window of two seconds of driver time; half a second above the anchor for
                // all of it is no stall.
                const WINDOW_US: i64 = 2_000_000;
                const LAG_US: i64 = 500_000;
                let offset = arrival_us - start;
                if !self.has_offset {
                    self.offset_us = offset;
                    self.window_offset_us = offset;
                    self.window_start_us = start;
                } else if start - self.window_start_us >= WINDOW_US {
                    if self.window_offset_us - self.offset_us > LAG_US {
                        self.offset_us = self.window_offset_us;
                    }
                    self.window_start_us = start;
                    self.window_offset_us = offset;
                } else {
                    self.window_offset_us = self.window_offset_us.min(offset);
                }
                self.offset_us = self.offset_us.min(offset);
                self.has_offset = true;
                time = self.offset_us + start;
            }
            // No driver time, or the stream restarted: arrival until it advances again.
            _ => self.has_offset = false,
        }
        if driver_us.is_some() {
            self.last_start_us = driver_us;
        }
        // A millisecond apart at least, so no two frames share a tick of the encoder's time base.
        if self.last_frame_us > 0 {
            time = time.max(self.last_frame_us + 1000);
        }
        self.last_frame_us = time;
        time
    }
}

// ---------------------------------------------------------------------------------------------
// The encoder (`CameraEncoder`).

/// The EBML writer of `cameraStreamHeader`: IDs keep their marker bits, sizes are eight bytes.
fn ebml_id(out: &mut Vec<u8>, id: u32) {
    for shift in [24, 16, 8, 0] {
        if id >> shift != 0 {
            out.push(((id >> shift) & 0xff) as u8);
        }
    }
}

fn ebml_size(out: &mut Vec<u8>, size: u64) {
    out.push(0x01);
    for shift in (0..=48).rev().step_by(8) {
        out.push(((size >> shift) & 0xff) as u8);
    }
}

fn ebml_element(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    ebml_id(&mut out, id);
    ebml_size(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

fn ebml_unsigned(id: u32, value: u64) -> Vec<u8> {
    ebml_element(id, &value.to_be_bytes())
}

/// `cameraStreamHeader` for NV12 frames of `width` x `height`.
fn stream_header(width: u32, height: u32) -> Vec<u8> {
    let cat = |parts: &[Vec<u8>]| parts.concat();
    let ebml = ebml_element(
        0x1A45DFA3,
        &cat(&[
            ebml_unsigned(0x4286, 1),
            ebml_unsigned(0x42F7, 1),
            ebml_unsigned(0x42F2, 4),
            ebml_unsigned(0x42F3, 8),
            ebml_element(0x4282, b"matroska"),
            ebml_unsigned(0x4287, 4),
            ebml_unsigned(0x4285, 2),
        ]),
    );
    // Timestamps count microseconds. The default duration, one frame at 30 fps, is only the
    // last frame's.
    let info = ebml_element(0x1549A966, &cat(&[ebml_unsigned(0x2AD7B1, 1000), ebml_element(0x4D80, b"omareel"), ebml_element(0x5741, b"omareel")]));
    let video = ebml_element(0xE0, &cat(&[ebml_unsigned(0xB0, u64::from(width)), ebml_unsigned(0xBA, u64::from(height)), ebml_element(0x2EB524, b"NV12")]));
    let track = ebml_element(
        0xAE,
        &cat(&[
            ebml_unsigned(0xD7, 1),
            ebml_unsigned(0x73C5, 1),
            ebml_unsigned(0x83, 1),
            ebml_element(0x86, b"V_UNCOMPRESSED"),
            ebml_unsigned(0x9C, 0),
            ebml_unsigned(0x23E383, 33333333),
            video,
        ]),
    );
    let mut segment = Vec::new();
    ebml_id(&mut segment, 0x18538067);
    // Unknown size: the stream ends with the pipe.
    segment.extend_from_slice(&[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    cat(&[ebml, segment, info, ebml_element(0x1654AE6B, &track)])
}

/// `cameraFrameHeader`: a cluster at the frame's time holding one keyframe SimpleBlock of
/// track 1; the frame's bytes follow.
fn frame_header(timestamp_us: i64, frame_bytes: usize) -> Vec<u8> {
    let block_header = [0x81u8, 0x00, 0x00, 0x80];
    let mut block = Vec::new();
    ebml_id(&mut block, 0xA3);
    ebml_size(&mut block, (block_header.len() + frame_bytes) as u64);
    block.extend_from_slice(&block_header);
    let timestamp = ebml_unsigned(0xE7, timestamp_us.max(0) as u64);
    let mut cluster = Vec::new();
    ebml_id(&mut cluster, 0x1F43B675);
    ebml_size(&mut cluster, (timestamp.len() + block.len() + frame_bytes) as u64);
    [cluster, timestamp, block].concat()
}

/// `cameraEncoderArguments` for the NV12 frames (BT.601, limited range) the capture hands over.
pub fn encoder_arguments(nvenc: bool, path: &str) -> Vec<String> {
    // swscale reads untagged YCbCr as BT.601 while players take HD video for BT.709, so convert
    // to BT.709 limited range here, once, and say so in the file.
    let scale = "scale=in_color_matrix=bt601:in_range=tv:out_color_matrix=bt709:out_range=tv,format=yuv420p,\
                 setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv";
    let mut args: Vec<&str> = vec!["-hide_banner", "-loglevel", "error", "-y", "-f", "matroska", "-i", "pipe:0", "-an", "-vf", scale, "-c:v"];
    args.push(if nvenc { "h264_nvenc" } else { "libx264" });
    if nvenc {
        args.extend(["-preset", "p4", "-zerolatency", "1", "-rc", "vbr", "-cq", "23"]);
    } else {
        args.extend(["-preset", "veryfast", "-tune", "zerolatency", "-crf", "20"]);
    }
    // A keyframe, and so a fragment, about every second; zerolatency stops that GOP staying
    // inside the encoder when the recording stops.
    args.extend([
        "-bf", "0", "-g", "30", "-pix_fmt", "yuv420p", "-profile:v", "high", "-colorspace", "bt709", "-color_primaries", "bt709",
        "-color_trc", "bt709", "-color_range", "tv", "-fps_mode", "vfr", "-enc_time_base", "1/90000", "-video_track_timescale", "90000",
        "-flush_packets", "1", "-movflags", "+frag_keyframe+empty_moov+default_base_moof", path,
    ]);
    args.into_iter().map(str::to_owned).collect()
}

struct Queued {
    frame: Arc<Frame>,
    timestamp_us: i64,
}

#[derive(Default)]
struct EncoderQueue {
    queue: VecDeque<Queued>,
    first_us: Option<i64>,
    error: String,
}

/// `CameraEncoder`: pipes queued frames, with their times, to ffmpeg on its own thread. Frames
/// are only dropped when more than a second's worth waits for the encoder.
struct Encoder {
    path: String,
    queue: Mutex<EncoderQueue>,
    queued: Condvar,
    stop_deadline: AtomicI64,
    encoded: AtomicI64,
    dropped: AtomicI64,
    finished: AtomicBool,
}

impl Encoder {
    fn new(path: String) -> Encoder {
        Encoder {
            path,
            queue: Mutex::new(EncoderQueue::default()),
            queued: Condvar::new(),
            stop_deadline: AtomicI64::new(0),
            encoded: AtomicI64::new(0),
            dropped: AtomicI64::new(0),
            finished: AtomicBool::new(false),
        }
    }

    /// Queues a frame captured at `time_us`; the first frame queued is time zero.
    fn submit(&self, frame: Arc<Frame>, time_us: i64) -> bool {
        const QUEUE_LIMIT: usize = 30;
        let mut queue = self.queue.lock().unwrap();
        if self.stop_deadline.load(Ordering::SeqCst) != 0 {
            return false;
        }
        let first = *queue.first_us.get_or_insert(time_us);
        if queue.queue.len() >= QUEUE_LIMIT {
            let dropped = self.dropped.fetch_add(1, Ordering::SeqCst) + 1;
            if dropped == 1 || dropped % 30 == 0 {
                eprintln!("Camera encoder is behind; dropped {dropped} frames");
            }
            return false;
        }
        queue.queue.push_back(Queued { frame, timestamp_us: time_us - first });
        self.queued.notify_one();
        true
    }

    /// Encodes what is queued and closes the file, within four and a half seconds.
    fn request_stop(&self) {
        let _ = self.stop_deadline.compare_exchange(0, monotonic_us() + 4_500_000, Ordering::SeqCst, Ordering::SeqCst);
        let _queue = self.queue.lock().unwrap();
        self.queued.notify_all();
    }

    fn error(&self) -> String {
        self.queue.lock().unwrap().error.clone()
    }

    fn remaining_ms(&self) -> i64 {
        ((self.stop_deadline.load(Ordering::SeqCst) - monotonic_us()) / 1000).max(0)
    }

    fn run(&self) {
        let mut process: Option<Child> = None;
        let mut size = (0, 0);
        let mut failure = String::new();
        let log = Arc::new(Mutex::new(String::new()));
        let error_text = |log: &Arc<Mutex<String>>| {
            let text = log.lock().unwrap();
            text[text.len().saturating_sub(2048)..].to_owned()
        };
        loop {
            let item = {
                let mut queue = self.queue.lock().unwrap();
                while queue.queue.is_empty() && self.stop_deadline.load(Ordering::SeqCst) == 0 {
                    queue = self.queued.wait(queue).unwrap();
                }
                if queue.queue.is_empty() {
                    break;
                }
                // Frames still waiting when the stop deadline nears are dropped; ffmpeg keeps a
                // second and a half to finish the file.
                if self.stop_deadline.load(Ordering::SeqCst) != 0 && self.remaining_ms() < 1500 {
                    self.dropped.fetch_add(queue.queue.len() as i64, Ordering::SeqCst);
                    queue.queue.clear();
                    break;
                }
                queue.queue.pop_front().expect("a queued frame")
            };
            let frame = item.frame;
            match &mut process {
                None => {
                    let nvenc = omareel_record::screen_capture::probe::cached_preferred_encoder_available();
                    let args = encoder_arguments(nvenc, &self.path);
                    eprintln!("Camera encoder: {} raw format: nv12 {}", if nvenc { "h264_nvenc" } else { "libx264" }, args[args.iter().position(|a| a == "-vf").unwrap() + 1]);
                    let spawned = Command::new("ffmpeg").args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn();
                    let mut child = match spawned {
                        Ok(child) => child,
                        Err(error) => {
                            failure = format!("Could not start camera ffmpeg: {error}");
                            break;
                        }
                    };
                    let stderr = child.stderr.take().expect("piped stderr");
                    let thread_log = log.clone();
                    std::thread::spawn(move || {
                        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                            eprintln!("Camera ffmpeg: {line}");
                            let mut text = thread_log.lock().unwrap();
                            text.push_str(&line);
                            text.push('\n');
                        }
                    });
                    size = (frame.width, frame.height);
                    let ok = write_all(&mut child, &stream_header(frame.width, frame.height));
                    process = Some(child);
                    if !ok {
                        failure = format!("Camera encoder exited: {}", error_text(&log));
                        break;
                    }
                }
                Some(_) if (frame.width, frame.height) != size => {
                    failure = "Camera changed its raw frame format during recording".into();
                    break;
                }
                Some(_) => {}
            }
            let child = process.as_mut().expect("the encoder runs");
            if !write_all(child, &frame_header(item.timestamp_us, frame.bytes.len())) || !write_all(child, &frame.bytes) {
                failure = format!("Camera encoder pipe failed or stalled for two seconds: {}", error_text(&log));
                break;
            }
            self.encoded.fetch_add(1, Ordering::SeqCst);
        }
        if !failure.is_empty() {
            self.request_stop();
            if let Some(child) = &mut process {
                let _ = child.kill();
            }
        }
        self.queue.lock().unwrap().queue.clear();
        if let Some(child) = &mut process {
            drop(child.stdin.take());
            let deadline = std::time::Instant::now() + Duration::from_millis(self.remaining_ms() as u64);
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                    _ => break None,
                }
            };
            match status {
                Some(status) if failure.is_empty() && !status.success() => {
                    failure = format!("Camera encoder failed: {}", error_text(&log));
                }
                Some(_) => {}
                None => {
                    if failure.is_empty() {
                        failure = "Camera encoder did not finish within five seconds".into();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        eprintln!("Camera encoder: {} frames encoded, {} dropped", self.encoded.load(Ordering::SeqCst), self.dropped.load(Ordering::SeqCst));
        self.queue.lock().unwrap().error = failure;
    }
}

/// Writes all of `bytes` to the child's stdin, failing when the pipe takes none of it for two
/// seconds or the child is gone (what the C++ waits for with `waitForBytesWritten`).
fn write_all(child: &mut Child, bytes: &[u8]) -> bool {
    let Some(stdin) = child.stdin.as_mut() else { return false };
    let fd = stdin.as_raw_fd();
    // SAFETY: fcntl on a pipe this process owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let mut written = 0;
    let mut stalled_since = std::time::Instant::now();
    while written < bytes.len() {
        // SAFETY: writing from a live slice to an owned descriptor.
        let n = unsafe { libc::write(fd, bytes[written..].as_ptr().cast(), bytes.len() - written) };
        if n > 0 {
            written += n as usize;
            stalled_since = std::time::Instant::now();
            continue;
        }
        let error = std::io::Error::last_os_error();
        if n < 0 && error.kind() != std::io::ErrorKind::WouldBlock && error.kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
        if stalled_since.elapsed() > Duration::from_secs(2) || matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        let mut poll = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
        // SAFETY: one pollfd for an owned descriptor.
        unsafe { libc::poll(&mut poll, 1, 10) };
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showinfo_lines() {
        let line = "[Parsed_showinfo_0 @ 0xaaaa] n:  12 pts:1734512345678 pts_time:1734512.345678 duration:33333 fmt:yuvj422p";
        assert_eq!(showinfo_time(line), Some((12, 1734512345678)));
        assert_eq!(showinfo_time("frame=  30 fps=30"), None);
    }

    #[test]
    fn clock_keeps_monotonic_driver_time() {
        let mut clock = FrameClock::default();
        assert_eq!(clock.frame_time_us(Some(1_000_000), 1_020_000), 1_000_000);
        // Never two frames within a millisecond.
        assert_eq!(clock.frame_time_us(Some(1_000_100), 1_040_000), 1_001_000);
        // No driver time: arrival.
        assert_eq!(clock.frame_time_us(None, 1_100_000), 1_100_000);
    }

    #[test]
    fn frame_header_matches_the_cpp_layout() {
        let header = frame_header(1000, 6);
        // Cluster ID, 8-byte size, timestamp element (ID + size + 8 bytes), block ID + size + header.
        assert_eq!(&header[..4], &[0x1F, 0x43, 0xB6, 0x75]);
        assert_eq!(header.len(), 4 + 8 + (1 + 8 + 8) + (1 + 8 + 4));
        let stream = stream_header(1920, 1080);
        assert_eq!(&stream[..4], &[0x1A, 0x45, 0xDF, 0xA3]);
    }
}
