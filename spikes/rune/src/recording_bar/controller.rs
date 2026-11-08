//! The `recordingBar` object (`crates/omareel_qt/src/recording_bar.rs` with
//! `RecordingBarBase`), and the window glue main.cpp wraps around it, on a thread of its own.
//!
//! It polls the recorder's state file every 50 ms (and, as a standby host, the self-view host
//! file the launcher commands it through), runs the camera, and after every step pushes what the
//! three overlays show to them (`sync`): which monitor each is on, whether it is mapped, where
//! the bubble sits, and their view models. The overlays send what the person does back as
//! [`Command`]s. What it computes is `omareel_record::recording_bar`; the state files are
//! `omareel_record::recorder::state`.

use super::camera::{self, Camera};
use super::outputs::Outputs;
use super::view::{BarModel, BarView, BubbleModel, BubbleView, CountdownModel, CountdownView};
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qfile;
use omareel_core::compat::qjson::{Object, Value};
use omareel_core::recording_preferences::RecordingPreferences;
use omareel_record::cursor_sampler;
use omareel_record::recorder::{self, state};
use omareel_record::recording_bar::{
    self, CameraShape, Point, SelfViewGeometry, Size, clamped_self_view_drag_position, self_view_host_should_exit, self_view_pixels,
};
use rune::{Align, DisplayTarget, NativeDisplayId, OverlayHandle, Pixels, ScreenEdge};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// What the overlays (and the camera) tell the bar: the QML's calls into `recordingBar`.
pub enum Command {
    Rotate,
    Flip,
    CycleShape,
    NextCorner,
    SetSelfViewVisible(bool),
    Stop,
    Cancel,
    /// Pressed on the bubble at this point of it (the overlay hears the press a frame late,
    /// so the cursor has moved on by then).
    BeginDrag(f64, f64),
    /// The pointer's global position, when the compositor could say.
    DragTo(Option<PointF>),
    EndDrag,
    Camera(camera::Event),
}

/// The bar window's size: room for the discard confirmation to unfold, the pill centred.
pub fn bar_size(webcam: bool) -> (f32, f32) {
    (if webcam { 448.0 } else { 288.0 }, 40.0)
}

/// The countdown window's size.
pub const COUNTDOWN_SIZE: (f32, f32) = (240.0, 240.0);

fn object<const N: usize>(entries: [(&str, Value); N]) -> Object {
    let mut object = Object::new();
    for (key, value) in entries {
        object.insert(key, value);
    }
    object
}

/// `Recorder::updateRecordingState(values)` with the error ignored, as the bar does.
fn update_recording_state(values: &Object) {
    let _ = state::update_recording_state(values);
}

fn env_equals(name: &str, value: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v == value)
}

fn application_pid() -> i64 {
    i64::from(std::process::id())
}

fn append_debug_log(line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).open("/tmp/omareel.log") {
        let _ = file.write_all(line.as_bytes());
    }
}

/// `debugBarStage(stage, detail)`: with `OMAREEL_DEBUG=1`, a startup trace line with the time
/// since the daemon started, when the recording's state has it.
fn debug_bar_stage(stage: &str, detail: &str) {
    if !env_equals("OMAREEL_DEBUG", "1") {
        return;
    }
    let mut line = format!("OMAREEL_START stage={stage} monotonic_us={}", recorder::monotonic_us());
    let daemon_start_us = state::recording_state().value("daemon_started_us").to_variant_long_long();
    if daemon_start_us > 0 {
        line += &format!(" elapsed_ms={:.3}", (recorder::monotonic_us() - daemon_start_us) as f64 / 1000.0);
    }
    if !detail.is_empty() {
        line.push(' ');
        line += detail;
    }
    line.push('\n');
    append_debug_log(&line);
}

/// `debugUiStage(stage)` for the windows: always `elapsed_ms`, 0 without the daemon's start.
fn log_ui_stage(stage: &str) {
    if !env_equals("OMAREEL_DEBUG", "1") {
        return;
    }
    let monotonic_us = recorder::monotonic_us();
    let daemon_start_us = state::recording_state().value("daemon_started_us").to_variant_long_long();
    let elapsed_ms = if daemon_start_us > 0 { (monotonic_us - daemon_start_us) as f64 / 1000.0 } else { 0.0 };
    append_debug_log(&format!("OMAREEL_START stage={stage} monotonic_us={monotonic_us} elapsed_ms={elapsed_ms:.3}\n"));
}

/// `QLockFile(path).tryLock(0)`, held while the file stays open: an flock, which the kernel
/// drops with a crashed process, so a stale lock never blocks the next host.
fn try_lock(path: &str) -> Option<std::fs::File> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path).ok()?;
    // SAFETY: flock on a descriptor this process owns.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return None;
    }
    let _ = file.set_len(0);
    let _ = writeln!(file, "{}", std::process::id());
    Some(file)
}

/// The overlays the controller places and feeds.
pub struct Windows {
    pub app: rune::AppHandle<()>,
    pub bar: OverlayHandle<BarView>,
    pub countdown: OverlayHandle<CountdownView>,
    pub bubble: Option<OverlayHandle<BubbleView>>,
}

/// What was last pushed to the overlays, so `sync` only sends changes.
#[derive(Default)]
struct Shown {
    bar_display: Option<u32>,
    bar_visible: bool,
    bar_model: Option<BarModel>,
    bar_mapped_logged: bool,
    countdown_display: Option<u32>,
    countdown_visible: bool,
    countdown_margin: Option<f32>,
    countdown_model: Option<CountdownModel>,
    bubble_display: Option<u32>,
    bubble_visible: bool,
    bubble_place: Option<(i32, i32, i32)>,
    bubble_model: Option<BubbleModel>,
}

/// `RecordingBar`: its state (with `RecordingBar.h`'s defaults), the camera, the timers.
pub struct Controller {
    standby: bool,
    adopted: bool,
    suppressed: bool,
    standby_monitor: String,
    standby_position: Point,
    quit: bool,
    owner: i64,
    host_sequence: i64,
    host_lock: Option<std::fs::File>,
    elapsed: String,
    recorded_monitor: String,
    webcam: bool,
    hidden: bool,
    capture_started: bool,
    stopping: bool,
    countdown_active: bool,
    countdown_value: i32,
    /// Bumped on every `countdownChanged`, which restarts the dial's sweep and pop.
    countdown_generation: u64,
    camera_rotation: i32,
    camera_flip_horizontal: bool,
    shape: CameraShape,
    camera_error: String,
    self_view_visible: bool,
    self_view_safe: bool,
    self_view_monitor: String,
    geometry: SelfViewGeometry,
    drag_press_pointer: PointF,
    drag_press_bubble: Point,
    drag_active: bool,
    command_sequence: i64,
    camera_stop_requested: bool,
    camera_record_requested: bool,
    camera_failed: bool,
    capture_stage_logged: bool,
    camera: Option<Camera>,
    /// What the camera draws into; the bubble shows it.
    pub preview: Pixels,
    send: Sender<Command>,
    // `RecordingBarBase`'s timers: the poll (50 ms), the position write (34 ms, single shot)
    // and the position save (180 ms, single shot).
    next_poll: Instant,
    position_write_at: Option<Instant>,
    save_self_view_at: Option<Instant>,
    /// `finished()` was emitted: the event loop ends.
    finished: bool,
    outputs: Outputs,
    next_output_check: Instant,
    shown: Shown,
}

impl Controller {
    /// `RecordingBar(hidden, standby, owner)`, member initializers and constructor body. A
    /// standby host that finds another host already there is `finished` at once.
    pub fn new(hidden: bool, standby: bool, owner: i64, send: Sender<Command>) -> Controller {
        let initial = state::recording_state();
        let recorded_monitor = initial.value("overlay_monitor").to_string_or(&state::recorded_monitor());
        let mut bar = Controller {
            standby,
            adopted: false,
            suppressed: false,
            standby_monitor: String::new(),
            standby_position: Point::default(),
            quit: false,
            owner,
            host_sequence: 0,
            host_lock: None,
            elapsed: "00:00".into(),
            recorded_monitor,
            webcam: state::recording_has_webcam(),
            hidden,
            capture_started: false,
            stopping: false,
            countdown_active: false,
            countdown_value: 3,
            countdown_generation: 0,
            camera_rotation: 0,
            camera_flip_horizontal: false,
            shape: CameraShape::default(),
            camera_error: String::new(),
            self_view_visible: false,
            self_view_safe: false,
            self_view_monitor: String::new(),
            geometry: SelfViewGeometry::default(),
            drag_press_pointer: PointF::default(),
            drag_press_bubble: Point::default(),
            drag_active: false,
            command_sequence: 0,
            camera_stop_requested: false,
            camera_record_requested: false,
            camera_failed: false,
            capture_stage_logged: false,
            camera: None,
            preview: Pixels::new(),
            send,
            next_poll: Instant::now(),
            position_write_at: None,
            save_self_view_at: None,
            finished: false,
            outputs: Outputs::new(),
            next_output_check: Instant::now() + Duration::from_secs(1),
            shown: Shown::default(),
        };
        bar.construct();
        bar
    }

    fn construct(&mut self) {
        let mut state = state::recording_state();
        if self.standby {
            let host_path = state::self_view_host_path();
            qdir::mkpath(&FileInfo::new(host_path.clone()).absolute_path());
            match try_lock(&format!("{host_path}.host-lock")) {
                Some(lock) => self.host_lock = Some(lock),
                None => {
                    self.finished = true;
                    return;
                }
            }
            // Holding the host lock proves that no previous host owns this state.
            qfile::remove(&host_path);
            let preferences = RecordingPreferences::load();
            let placement = recording_bar::standby_placement(&preferences);
            self.recorded_monitor = placement.recorded_monitor;
            self.webcam = true;
            state = placement.state;
            let _ = state::update_state_file(&host_path, &object([("command", "".into()), ("command_sequence", 0.into())]));
        }
        let preferences = RecordingPreferences::load();
        self.camera_rotation = state.value("camera_rotation").to_int(0);
        self.camera_flip_horizontal = state.value("camera_flip_horizontal").to_bool(false);
        // The project's vocabulary: round, or square with a corner radius (0 for sharp corners).
        self.shape = CameraShape::from_preferences(&preferences);
        self.self_view_visible = state.value("selfview").to_bool(false);
        self.self_view_safe = state.value("selfview_safe").to_bool(false);
        self.self_view_monitor = state.value("selfview_monitor").to_string_or(&self.recorded_monitor);
        self.geometry.pixels = state.value("selfview_pixels").to_int(160);
        self.geometry.x = state.value("selfview_x").to_int(16);
        self.geometry.y = state.value("selfview_y").to_int(16);
        self.capture_started = state.value("capture_started").to_bool(false);
        self.countdown_active = state.value("countdown_active").to_bool(false);
        self.command_sequence = state.value("recording_bar_command_sequence").to_variant_long_long();
        if self.webcam {
            self.create_camera(&state);
        }
        if self.standby {
            self.publish_host();
        }
        // main.cpp: the self-view's screen is the one it is placed on, else the bar's.
        let screen = self.outputs.logical_size(&self.self_view_monitor).or_else(|| self.outputs.logical_size(&self.recorded_monitor));
        if let Some((w, h)) = screen {
            self.geometry.set_screen_size(Size::new(w, h));
            if std::env::var_os("OMAREEL_SELFVIEW_DRAG_LOG").is_some() {
                eprintln!("self-view logical-screen={w}x{h}");
            }
        }
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn webcam(&self) -> bool {
        self.webcam
    }

    pub fn standby(&self) -> bool {
        self.standby
    }

    /// `ownsStandbyHost`: whether this standby host is the one the self-view host file names.
    pub fn owns_standby_host(&self) -> bool {
        state::read_state_file(&state::self_view_host_path()).value("pid").to_variant_long_long() == application_pid()
    }

    /// `publishBarGeometry`: the bar's window size, which the recorder keeps out of the capture.
    pub fn publish_bar_geometry(&self) {
        let (w, h) = bar_size(self.webcam);
        update_recording_state(&object([("bar_width", (w as i32).into()), ("bar_height", (h as i32).into())]));
    }

    /// `publishCountdownGeometry`: the recorder starts the countdown once this is published.
    pub fn publish_countdown_geometry(&self) {
        update_recording_state(&object([
            ("countdown_width", (COUNTDOWN_SIZE.0 as i32).into()),
            ("countdown_height", (COUNTDOWN_SIZE.1 as i32).into()),
            ("countdown_ready", true.into()),
        ]));
    }

    // -----------------------------------------------------------------------------------------
    // Properties.

    fn bubble_mapped(&self) -> bool {
        self.self_view_visible && !self.suppressed && self.self_view_safe && (self.standby || self.capture_started)
    }

    fn camera_ready(&self) -> bool {
        self.camera.as_ref().is_some_and(Camera::ready)
    }

    fn camera_size(&self) -> (i32, i32) {
        self.camera.as_ref().map_or((-1, -1), Camera::capture_size)
    }

    fn camera_frame_rate(&self) -> f64 {
        self.camera.as_ref().map_or(30.0, Camera::frame_rate)
    }

    /// The `camera_status` the bar reports: failed, ready once the camera shows frames, else
    /// starting.
    fn camera_status(&self) -> &'static str {
        if self.camera_failed {
            "failed"
        } else if self.camera_ready() {
            "ready"
        } else {
            "starting"
        }
    }

    /// Whether the recording's state is the bar's to write: always for a recording's bar, and
    /// for a standby host once a recording adopted it.
    fn writes_recording_state(&self) -> bool {
        !self.standby || self.adopted
    }

    // -----------------------------------------------------------------------------------------
    // The event loop.

    /// Runs until `finished`: commands as they come, the timers when they are due, and the
    /// overlays brought up to date after each step. Then the camera finishes and the app quits.
    pub fn run(mut self, receive: Receiver<Command>, windows: Windows, quit: Arc<AtomicBool>) {
        // Let the windows exist before an invalid or dead owner ends the host (callLater).
        self.poll();
        self.sync(&windows);
        while !self.finished && !quit.load(Ordering::SeqCst) {
            let now = Instant::now();
            // While the bubble is dragged it follows the compositor's cursor every 8 ms, not
            // only the moves the overlay hears: those come a frame apart, and a quick drag
            // otherwise leaves the surface (and so ends) before the bubble catches up.
            let follow = self.drag_active.then(|| now + Duration::from_millis(8));
            let due = [Some(self.next_poll), self.position_write_at, self.save_self_view_at, follow].into_iter().flatten().min().unwrap_or(now);
            match receive.recv_timeout(due.saturating_duration_since(now)) {
                Ok(command) => self.command(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = Instant::now();
            if self.drag_active
                && let Some(pointer) = global_cursor_pos()
            {
                self.drag_to(pointer);
            }
            if self.position_write_at.is_some_and(|at| at <= now) {
                self.position_write_at = None;
                self.publish_geometry();
            }
            if self.save_self_view_at.is_some_and(|at| at <= now) {
                self.save_self_view_at = None;
                self.save_self_view_position();
            }
            if !self.finished && self.next_poll <= now {
                self.next_poll = now + Duration::from_millis(50);
                if now >= self.next_output_check {
                    self.next_output_check = now + Duration::from_secs(1);
                    self.outputs.refresh();
                }
                self.poll();
            }
            self.sync(&windows);
        }
        // `finished` → the app quits; `~RecordingBar` then removes the host file, and the
        // camera (its recording given its deadline) goes before the host lock.
        windows.bar.hide();
        windows.countdown.hide();
        if let Some(bubble) = &windows.bubble {
            bubble.hide();
        }
        drop(self.camera.take());
        if self.standby && self.host_lock.is_some() {
            qfile::remove(&state::self_view_host_path());
        }
        drop(self.host_lock.take());
        windows.app.quit();
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Rotate => self.rotate_camera(),
            Command::Flip => self.flip_camera(),
            Command::CycleShape => self.cycle_camera_shape(),
            Command::NextCorner => self.snap_self_view_to_next_corner(),
            Command::SetSelfViewVisible(visible) => self.set_self_view_visible(visible),
            Command::Stop => self.stop(),
            Command::Cancel => self.cancel(),
            Command::BeginDrag(x, y) => {
                let origin = self.outputs.origin(&self.self_view_monitor);
                let pressed = match origin {
                    Some((ox, oy)) => Some(PointF::new(f64::from(ox + self.geometry.x) + x, f64::from(oy + self.geometry.y) + y)),
                    None => global_cursor_pos(),
                };
                self.begin_drag(pressed.unwrap_or(self.drag_press_pointer));
            }
            Command::DragTo(point) => self.drag_to(point.unwrap_or(self.drag_press_pointer)),
            Command::EndDrag => self.end_drag(),
            Command::Camera(camera::Event::Ready) => self.handle_camera_ready_changed(),
            Command::Camera(camera::Event::Error(message)) => self.handle_camera_error(message),
            Command::Camera(camera::Event::Stopped) => self.handle_camera_stopped(),
        }
    }

    // -----------------------------------------------------------------------------------------
    // The poll.

    /// `RecordingBar::poll`: the recording's state, and for a standby host the launcher's
    /// commands, the adoption handshake and the return to standby.
    fn poll(&mut self) {
        let active = state::is_recording();
        let mut state = state::recording_state();
        if self.standby {
            let host = state::read_state_file(&state::self_view_host_path());
            let sequence = host.value("command_sequence").to_variant_long_long();
            if sequence != self.host_sequence {
                self.host_sequence = sequence;
                self.host_command(&host.value("command").to_string_or(""));
            }
            let ours = active && state.value("host_pid").to_variant_long_long() == application_pid();
            if ours && !self.adopted {
                let fallback = state.value("overlay_exclusion").to_string_or("") == "fallback";
                self.adopted = true;
                self.standby_monitor = self.self_view_monitor.clone();
                self.standby_position = Point::new(self.geometry.x, self.geometry.y);
                self.recorded_monitor = state.value("overlay_monitor").to_string_or("");
                if fallback {
                    self.self_view_monitor = state.value("selfview_monitor").to_string_or("");
                    self.self_view_safe = state.value("selfview_safe").to_bool(false);
                }
                self.hidden = !state.value("bar_visible").to_bool(true);
                let bundle = state.value("bundle").to_string_or("");
                if let Some(camera) = &self.camera {
                    camera.set_recording_output(&format!("{bundle}/camera.mp4"), &format!("{bundle}/camera.mp4.ts"));
                }
                self.publish_geometry();
                let (width, height) = self.camera_size();
                update_recording_state(&object([
                    ("host_acknowledged", true.into()),
                    ("countdown_ready", true.into()),
                    ("camera_rotation", self.camera_rotation.into()),
                    ("camera_flip_horizontal", self.camera_flip_horizontal.into()),
                    ("camera_shape", self.shape.shape.as_str().into()),
                    ("camera_radius", self.shape.radius.into()),
                    ("camera_status", self.camera_status().into()),
                    ("camera_width", width.into()),
                    ("camera_height_actual", height.into()),
                    ("camera_fps", self.camera_frame_rate().into()),
                ]));
                state = state::recording_state();
            }
            self.suppressed = active && !ours;
            let owner = self.owner;
            // SAFETY: kill with signal 0 only checks that the process exists.
            let owner_alive = owner > 0
                && (unsafe { libc::kill(owner as libc::pid_t, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM));
            if self_view_host_should_exit(active, self.quit, owner, owner_alive) {
                self.finished = true;
                return;
            }
            if !active && self.adopted {
                self.adopted = false;
                self.capture_started = false;
                self.hidden = true;
                self.self_view_safe = true;
                self.self_view_monitor = self.standby_monitor.clone();
                self.geometry.x = self.standby_position.x;
                self.geometry.y = self.standby_position.y;
                self.camera_record_requested = false;
                self.camera_stop_requested = false;
                self.stopping = false;
                self.host_command("configure");
                self.publish_host();
            }
            if !ours {
                return;
            }
        } else if !active {
            self.finished = true;
            return;
        }
        self.recorded_monitor = state.value("overlay_monitor").to_string_or(&self.recorded_monitor);
        self.hidden = !state.value("bar_visible").to_bool(!self.hidden);
        self.self_view_safe = state.value("selfview_safe").to_bool(self.self_view_safe);
        self.self_view_monitor = state.value("selfview_monitor").to_string_or(&self.self_view_monitor);
        self.capture_started = state.value("capture_started").to_bool(false);
        if self.capture_started && env_equals("OMAREEL_DEBUG", "1") && !self.capture_stage_logged {
            self.capture_stage_logged = true;
            debug_bar_stage("capture_seen_by_bar", "");
        }
        // Stop can also come from the keybind or the REC indicator.
        if state.value("stopping").to_bool(false) {
            self.stopping = true;
        }
        let countdown_active = state.value("countdown_active").to_bool(false);
        let countdown_end_us = state.value("countdown_end_us").to_variant_long_long();
        let countdown_value = recording_bar::countdown_value(countdown_end_us, recorder::monotonic_us());
        if countdown_active != self.countdown_active || countdown_value != self.countdown_value {
            self.countdown_active = countdown_active;
            self.countdown_value = countdown_value;
            self.countdown_generation += 1;
        }
        self.self_view_visible = state.value("selfview").to_bool(false);
        let requested_x = state.value("selfview_x").to_int(self.geometry.x);
        let requested_y = state.value("selfview_y").to_int(self.geometry.y);
        let next = self.geometry.clamped_requested_position(requested_x, requested_y);
        if !self.drag_active && self.position_write_at.is_none() {
            self.geometry.x = next.x;
            self.geometry.y = next.y;
        }
        let command_sequence = state.value("recording_bar_command_sequence").to_variant_long_long();
        if command_sequence != self.command_sequence {
            self.command_sequence = command_sequence;
            match state.value("recording_bar_command").to_string_or("").as_str() {
                "rotate" => self.rotate_camera(),
                "flip" => self.flip_camera(),
                "drag-sim" => {
                    let point = |x: &str, y: &str| PointF::new(state.value(x).to_double(0.0), state.value(y).to_double(0.0));
                    self.begin_drag(point("drag_x0", "drag_y0"));
                    self.drag_to(point("drag_x1", "drag_y1"));
                    self.end_drag();
                }
                _ => {}
            }
        }
        if self.camera.is_some() && state.value("camera_record").to_bool(false) && !self.camera_record_requested {
            self.camera_record_requested = true;
            debug_bar_stage("camera_record_begin", "");
            if let Some(camera) = &self.camera {
                camera.begin_recording();
            }
        }
        if self.camera.is_some() && state.value("camera_stop").to_bool(false) && !self.camera_stop_requested {
            self.camera_stop_requested = true;
            if let Some(camera) = &self.camera {
                camera.stop();
            }
        }
        if self.stopping {
            return;
        }
        self.elapsed = recording_bar::elapsed_text(recorder::monotonic_us(), state::recording_started_us());
    }

    // -----------------------------------------------------------------------------------------
    // The camera's settings.

    fn cycle_camera_shape(&mut self) {
        // Round, then square with rounded corners, then square with sharp corners.
        self.shape.cycle();
        let mut preferences = RecordingPreferences::load();
        preferences.webcam.insert("shape", self.shape.shape.as_str());
        preferences.webcam.insert("radius", self.shape.radius);
        let _ = preferences.save();
        if self.writes_recording_state() {
            update_recording_state(&object([("camera_shape", self.shape.shape.as_str().into()), ("camera_radius", self.shape.radius.into())]));
        }
        self.publish_host();
    }

    fn log_camera_settings(&self) {
        if std::env::var_os("OMAREEL_CAMERA_SETTINGS_LOG").is_some() {
            eprintln!("camera rotation={} flip={}", self.camera_rotation, self.camera_flip_horizontal);
        }
    }

    fn rotate_camera(&mut self) {
        let rotation = (self.camera_rotation + 90) % 360;
        self.camera_rotation = rotation;
        let mut preferences = RecordingPreferences::load();
        preferences.webcam_rotation = rotation;
        preferences.webcam.insert("rotation", rotation);
        let _ = preferences.save();
        if self.writes_recording_state() {
            update_recording_state(&object([("camera_rotation", rotation.into())]));
        }
        self.publish_host();
        self.log_camera_settings();
    }

    fn flip_camera(&mut self) {
        let flip = !self.camera_flip_horizontal;
        self.camera_flip_horizontal = flip;
        let mut preferences = RecordingPreferences::load();
        preferences.webcam_flip_horizontal = flip;
        preferences.webcam.insert("flipHorizontal", flip);
        let _ = preferences.save();
        if self.writes_recording_state() {
            update_recording_state(&object([("camera_flip_horizontal", flip.into())]));
        }
        self.publish_host();
        self.log_camera_settings();
    }

    // -----------------------------------------------------------------------------------------
    // The self-view's placement.

    /// What `setSelfViewPosition` does once the bubble moved: the position is written soon
    /// (unless a write is already due) and saved a little later.
    fn self_view_moved(&mut self) {
        let now = Instant::now();
        if self.position_write_at.is_none() {
            self.position_write_at = Some(now + Duration::from_millis(34));
        }
        self.save_self_view_at = Some(now + Duration::from_millis(180));
    }

    fn snap_self_view_to_next_corner(&mut self) {
        if self.geometry.snap_to_next_corner() {
            self.self_view_moved();
        }
    }

    fn begin_drag(&mut self, global_position: PointF) {
        self.drag_press_pointer = global_position;
        self.drag_press_bubble = Point::new(self.geometry.x, self.geometry.y);
        self.drag_active = true;
    }

    fn drag_to(&mut self, pointer: PointF) {
        if !self.drag_active || self.geometry.screen_size.is_empty() {
            return;
        }
        let position = clamped_self_view_drag_position(self.drag_press_pointer, self.drag_press_bubble, pointer, self.geometry.screen_size, self.geometry.pixels);
        if std::env::var_os("OMAREEL_SELFVIEW_DRAG_LOG").is_some() {
            let delta = pointer - self.drag_press_pointer;
            eprintln!("pointer=({},{}) bubble=({},{}) delta=({},{})", pointer.x, pointer.y, position.x, position.y, delta.x, delta.y);
        }
        self.set_self_view_position(position.x, position.y);
    }

    fn end_drag(&mut self) {
        self.drag_active = false;
        self.position_write_at = None;
        self.publish_geometry();
        self.save_self_view_at = None;
        self.save_self_view_position();
    }

    fn set_self_view_position(&mut self, x: i32, y: i32) {
        if self.geometry.set_position(x, y) {
            self.self_view_moved();
        }
    }

    fn set_self_view_visible(&mut self, visible: bool) {
        if !self.webcam || self.self_view_visible == visible {
            return;
        }
        self.self_view_visible = visible;
        let mut preferences = RecordingPreferences::load();
        preferences.self_view_enabled = visible;
        let _ = preferences.save();
        self.publish_geometry();
    }

    fn stop(&mut self) {
        if self.stopping || state::signal_existing(false).is_err() {
            return;
        }
        self.stopping = true;
    }

    fn cancel(&mut self) {
        let _ = state::signal_existing(true);
    }

    /// `publishGeometry`: the bubble into the recording's state and the host file.
    fn publish_geometry(&mut self) {
        if self.writes_recording_state() {
            update_recording_state(&object([
                ("selfview_monitor", self.self_view_monitor.as_str().into()),
                ("selfview_x", self.geometry.x.into()),
                ("selfview_y", self.geometry.y.into()),
                ("selfview_pixels", self.geometry.pixels.into()),
                ("selfview", self.self_view_visible.into()),
            ]));
        }
        self.publish_host();
    }

    /// `publishHost`: what the standby host tells the launcher through its file.
    fn publish_host(&self) {
        if !self.standby {
            return;
        }
        let _ = state::update_state_file(
            &state::self_view_host_path(),
            &object([
                ("pid", application_pid().into()),
                ("owner_pid", self.owner.into()),
                ("monitor", self.self_view_monitor.as_str().into()),
                ("x", self.geometry.x.into()),
                ("y", self.geometry.y.into()),
                ("pixels", self.geometry.pixels.into()),
                ("visible", self.self_view_visible.into()),
                ("rotation", self.camera_rotation.into()),
                ("flip_horizontal", self.camera_flip_horizontal.into()),
                ("camera_status", self.camera_status().into()),
                ("camera_error", self.camera_error.as_str().into()),
            ]),
        );
    }

    /// `cameraState`: the camera's state into the recording's state, and the host file.
    fn camera_state(&self, values: &Object) {
        if self.writes_recording_state() {
            update_recording_state(values);
        }
        self.publish_host();
    }

    /// `hostCommand`: what the launcher asked the standby host through its file.
    fn host_command(&mut self, command: &str) {
        match command {
            "quit" => self.quit = true,
            "configure" if !self.adopted => {
                let preferences = RecordingPreferences::load();
                drop(self.camera.take());
                self.camera_failed = false;
                self.camera_error.clear();
                self.create_camera(&object([
                    ("camera_device", preferences.webcam_device.as_str().into()),
                    ("camera_height", preferences.webcam_height.into()),
                ]));
                self.publish_host();
            }
            "rotate" => self.rotate_camera(),
            "flip" => self.flip_camera(),
            "shape" => self.cycle_camera_shape(),
            "corner" => self.snap_self_view_to_next_corner(),
            "show" | "hide" => self.set_self_view_visible(command == "show"),
            "S" | "M" | "L" => {
                self.geometry.pixels = self_view_pixels(command);
                let mut preferences = RecordingPreferences::load();
                preferences.self_view_size = command.to_owned();
                let _ = preferences.save();
                let screen_size = self.geometry.screen_size;
                self.geometry.set_screen_size(screen_size);
                self.publish_geometry();
            }
            _ => {}
        }
    }

    /// `saveSelfViewPosition`: the bubble's place as fractions of its screen, which the next
    /// standby host starts from; not while an adopted host shows the fallback placement.
    fn save_self_view_position(&self) {
        if self.adopted && state::recording_state().value("overlay_exclusion").to_string_or("") == "fallback" {
            return;
        }
        let Some((x, y)) = self.geometry.saved_position() else { return };
        let mut preferences = RecordingPreferences::load();
        preferences.self_view_x = x;
        preferences.self_view_y = y;
        let _ = preferences.save();
    }

    // -----------------------------------------------------------------------------------------
    // The camera.

    /// `createCamera(state)` and the QML's `attachCameraOutput`, which starts it: the camera
    /// named in the state, recording into the bundle's `camera.mp4` when the state has a bundle.
    fn create_camera(&mut self, state: &Object) {
        let bundle = state.value("bundle").to_string_or("");
        let in_bundle = |name: &str| if bundle.is_empty() { String::new() } else { format!("{bundle}/{name}") };
        let send = std::sync::Mutex::new(self.send.clone());
        let camera = Camera::new(
            &state.value("camera_device").to_string_or(""),
            state.value("camera_height").to_int(1080),
            &in_bundle("camera.mp4"),
            &in_bundle("camera.mp4.ts"),
            self.preview.clone(),
            Arc::new(move |event| {
                let _ = send.lock().unwrap().send(Command::Camera(event));
            }),
        );
        camera.start();
        self.camera = Some(camera);
    }

    fn handle_camera_ready_changed(&mut self) {
        if !self.camera_ready() {
            return;
        }
        let (width, height) = self.camera_size();
        self.camera_state(&object([
            ("camera_status", "ready".into()),
            ("camera_backend", "ffmpeg-pipe".into()),
            ("camera_width", width.into()),
            ("camera_height_actual", height.into()),
            ("camera_fps", self.camera_frame_rate().into()),
        ]));
        debug_bar_stage("camera_ready", "");
    }

    fn handle_camera_error(&mut self, message: String) {
        self.camera_failed = true;
        self.camera_error = message.clone();
        self.camera_state(&object([("camera_status", "releasing".into()), ("camera_error", message.as_str().into())]));
    }

    fn handle_camera_stopped(&mut self) {
        let status = if self.camera_failed { "failed" } else { "stopped" };
        self.camera_state(&object([("camera_status", status.into())]));
    }

    // -----------------------------------------------------------------------------------------
    // main.cpp's window glue and the QML's bindings.

    /// Brings the overlays up to date: each on its monitor (when that monitor is connected),
    /// mapped when main.cpp would show it, the bubble at its margins, and their view models.
    fn sync(&mut self, windows: &Windows) {
        let bar_output = self.outputs.global(&self.recorded_monitor);

        // The bar: shown once the capture started, unless hidden (`--no-bar`, or no monitor
        // outside the capture).
        let bar_model = BarModel {
            elapsed: self.elapsed.clone(),
            webcam: self.webcam,
            self_view_visible: self.self_view_visible,
            flip: self.camera_flip_horizontal,
            round: self.shape.shape == "round",
            stopping: self.stopping,
        };
        if self.shown.bar_model.as_ref() != Some(&bar_model) {
            let model = bar_model.clone();
            windows.bar.update(move |view, _| view.model = model);
            self.shown.bar_model = Some(bar_model);
        }
        let bar_visible = bar_output.is_some() && self.capture_started && !self.hidden;
        if bar_output.is_some() && bar_output != self.shown.bar_display {
            windows.bar.set_display(DisplayTarget::Native(NativeDisplayId::Wayland(bar_output.unwrap())));
            self.shown.bar_display = bar_output;
        }
        if bar_visible != self.shown.bar_visible {
            if bar_visible {
                windows.bar.update(|view, _| view.entered = Some(Instant::now()));
                windows.bar.show();
                if !self.shown.bar_mapped_logged {
                    self.shown.bar_mapped_logged = true;
                    log_ui_stage("bar_mapped");
                }
            } else {
                windows.bar.hide();
            }
            self.shown.bar_visible = bar_visible;
        }

        // The countdown, centred on the bar's monitor until the capture starts.
        let countdown_model = CountdownModel { value: self.countdown_value, generation: self.countdown_generation };
        if self.shown.countdown_model.as_ref() != Some(&countdown_model) {
            let model = countdown_model.clone();
            windows.countdown.update(move |view, _| view.set_model(model));
            self.shown.countdown_model = Some(countdown_model);
        }
        if bar_output.is_some() && bar_output != self.shown.countdown_display {
            windows.countdown.set_display(DisplayTarget::Native(NativeDisplayId::Wayland(bar_output.unwrap())));
            self.shown.countdown_display = bar_output;
        }
        if let Some((_, h)) = self.outputs.logical_size(&self.recorded_monitor) {
            let margin = ((h as f32 - COUNTDOWN_SIZE.1) / 2.0).round().max(0.0);
            if self.shown.countdown_margin != Some(margin) {
                windows.countdown.place(ScreenEdge::Top, margin);
                self.shown.countdown_margin = Some(margin);
            }
        }
        let countdown_visible = bar_output.is_some() && self.countdown_active && !self.capture_started;
        if countdown_visible != self.shown.countdown_visible {
            if countdown_visible {
                windows.countdown.show();
            } else {
                windows.countdown.hide();
            }
            self.shown.countdown_visible = countdown_visible;
        }

        // The self-view bubble, placed by its margins on its own monitor.
        let Some(bubble) = &windows.bubble else { return };
        let bubble_output = self.outputs.global(&self.self_view_monitor);
        if let Some(output) = bubble_output
            && Some(output) != self.shown.bubble_display
        {
            if let Some((w, h)) = self.outputs.logical_size(&self.self_view_monitor) {
                self.geometry.set_screen_size(Size::new(w, h));
            }
            bubble.set_display(DisplayTarget::Native(NativeDisplayId::Wayland(output)));
            self.shown.bubble_display = Some(output);
        }
        let place = (self.geometry.x, self.geometry.y, self.geometry.pixels);
        if self.shown.bubble_place != Some(place) {
            let (x, y, pixels) = place;
            if self.shown.bubble_place.is_none_or(|(_, _, p)| p != pixels) {
                bubble.resize((pixels as f32, pixels as f32));
            }
            bubble.place(ScreenEdge::Top, y as f32);
            bubble.set_align(Align::Start, x as f32);
            self.shown.bubble_place = Some(place);
        }
        let mapped = self.bubble_mapped() && bubble_output.is_some();
        if let Some(camera) = &self.camera {
            camera.set_preview(self.camera_rotation, self.camera_flip_horizontal, mapped);
        }
        let bubble_model = BubbleModel {
            pixels: self.geometry.pixels,
            visible: self.self_view_visible,
            round: self.shape.shape == "round",
            radius: self.shape.radius,
            ready: self.camera_ready(),
            error: self.camera_error.clone(),
        };
        if self.shown.bubble_model.as_ref() != Some(&bubble_model) {
            let model = bubble_model.clone();
            bubble.update(move |view, _| view.model = model);
            self.shown.bubble_model = Some(bubble_model);
        }
        if mapped != self.shown.bubble_visible {
            if mapped {
                bubble.show();
            } else {
                bubble.hide();
            }
            self.shown.bubble_visible = mapped;
        }
    }
}

/// `globalCursorPos()`: where the pointer is in the compositor's logical space, which stays
/// put while the bubble moves under it.
pub fn global_cursor_pos() -> Option<PointF> {
    cursor_sampler::cursor_position()
}
