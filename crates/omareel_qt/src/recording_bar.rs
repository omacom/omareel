//! The `recordingBar` object of `omareel __record-bar`: the bar shown over a recording (elapsed
//! time, stop, discard, the camera's controls), its countdown, and the self-view bubble, which a
//! standby host (`--standby`, started by the launcher) shows before the recording and hands to it
//! when the recorder adopts it. It replaced src/ui/RecordingBar.cpp and keeps its QML surface
//! and the calls `main.cpp`'s window glue makes, which now also goes through it for the recorder's
//! state (`publishBarGeometry`, `publishCountdownGeometry`, `ownsStandbyHost`, `logUiStage`).
//!
//! The bar polls the recorder's state file every 50 ms and whenever its directory changes, and
//! the standby host also the self-view host file the launcher commands it through. What it
//! computes is `omareel_record::recording_bar`; the state files are
//! `omareel_record::recorder::state`. `RecordingBarBase` (crates/omareel_qt/cpp) owns the camera,
//! the watcher, the timers and the host lock, and reports to the overrides at the end of the
//! bridge.

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QPointF, QSize, QString};
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qfile;
use omareel_core::compat::qjson::Object;
use omareel_core::recording_preferences::RecordingPreferences;
use omareel_record::recorder::{self, state};
use omareel_record::recording_bar::{
    self, CameraShape, Point, SelfViewGeometry, Size, clamped_self_view_drag_position, self_view_host_should_exit,
    self_view_pixels,
};
use omareel_record::screen_capture::probe;

use crate::support::{append_debug_log, application_pid, env_equals, env_is_set, errno, kill};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        // QtShims.h first: it brings in QString whole before cxx-qt-lib's qstring.h (see
        // export.rs).
        include!("QtShims.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qpointf.h");
        type QPointF = cxx_qt_lib::QPointF;
        include!("cxx-qt-lib/qsize.h");
        type QSize = cxx_qt_lib::QSize;
    }

    #[namespace = "Omareel"]
    unsafe extern "C++Qt" {
        include!("RecordingBarBase.h");
        #[qobject]
        type RecordingBarBase;
    }

    #[namespace = "Omareel::RecordingBarShims"]
    unsafe extern "C++" {
        #[cxx_name = "logInfo"]
        fn log_info(message: &QString);
        #[cxx_name = "debugReal"]
        fn debug_real(value: f64) -> QString;
    }

    #[namespace = "omareel::record"]
    extern "Rust" {
        /// `cachedPreferredEncoderAvailable()`: whether the screen recorder's cached encoder probe
        /// found NVENC, without probing. Safe on any thread; the camera's encoder asks it.
        #[cxx_name = "cachedPreferredEncoderAvailable"]
        fn cached_preferred_encoder_available() -> bool;
    }

    #[namespace = "Omareel"]
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = RecordingBarBase]
        #[qproperty(bool, bubble_mapped, READ = bubble_mapped, NOTIFY = self_view_visibility_changed)]
        #[qproperty(QString, elapsed, READ = elapsed, NOTIFY = elapsed_changed)]
        #[qproperty(QString, recorded_monitor, READ = recorded_monitor, NOTIFY = capture_started_changed)]
        #[qproperty(bool, webcam, READ = webcam, CONSTANT)]
        #[qproperty(bool, hidden, READ = hidden, NOTIFY = capture_started_changed)]
        #[qproperty(bool, capture_started, READ = capture_started, NOTIFY = capture_started_changed)]
        #[qproperty(bool, stopping, READ = stopping, NOTIFY = stopping_changed)]
        #[qproperty(bool, countdown_active, READ = countdown_active, NOTIFY = countdown_changed)]
        #[qproperty(i32, countdown_value, READ = countdown_value, NOTIFY = countdown_changed)]
        #[qproperty(i32, camera_rotation, READ = camera_rotation, NOTIFY = camera_settings_changed)]
        #[qproperty(bool, camera_flip_horizontal, READ = camera_flip_horizontal, NOTIFY = camera_settings_changed)]
        #[qproperty(QString, camera_shape, READ = camera_shape, NOTIFY = camera_settings_changed)]
        #[qproperty(i32, camera_radius, READ = camera_radius, NOTIFY = camera_settings_changed)]
        #[qproperty(QString, camera_error, READ = camera_error, NOTIFY = camera_error_changed)]
        #[qproperty(bool, camera_ready, READ = camera_ready, NOTIFY = camera_ready_changed)]
        #[qproperty(bool, self_view_visible, READ = self_view_visible, NOTIFY = self_view_visibility_changed)]
        #[qproperty(bool, self_view_safe, READ = self_view_safe, NOTIFY = self_view_visibility_changed)]
        #[qproperty(QString, self_view_monitor, READ = self_view_monitor, NOTIFY = self_view_placement_changed)]
        #[qproperty(i32, self_view_pixels, READ = self_view_pixels, NOTIFY = self_view_placement_changed)]
        #[qproperty(i32, self_view_x, READ = self_view_x, NOTIFY = self_view_placement_changed)]
        #[qproperty(i32, self_view_y, READ = self_view_y, NOTIFY = self_view_placement_changed)]
        type RecordingBar = super::RecordingBarRust;

        #[qsignal]
        fn elapsed_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn camera_settings_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn camera_error_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn camera_ready_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn self_view_visibility_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn self_view_placement_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn capture_started_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn stopping_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn countdown_changed(self: Pin<&mut RecordingBar>);
        #[qsignal]
        fn finished(self: Pin<&mut RecordingBar>);

        fn bubble_mapped(self: &RecordingBar) -> bool;
        fn elapsed(self: &RecordingBar) -> QString;
        fn recorded_monitor(self: &RecordingBar) -> QString;
        fn webcam(self: &RecordingBar) -> bool;
        fn hidden(self: &RecordingBar) -> bool;
        fn capture_started(self: &RecordingBar) -> bool;
        fn stopping(self: &RecordingBar) -> bool;
        fn countdown_active(self: &RecordingBar) -> bool;
        fn countdown_value(self: &RecordingBar) -> i32;
        fn camera_rotation(self: &RecordingBar) -> i32;
        fn camera_flip_horizontal(self: &RecordingBar) -> bool;
        fn camera_shape(self: &RecordingBar) -> QString;
        fn camera_radius(self: &RecordingBar) -> i32;
        fn camera_error(self: &RecordingBar) -> QString;
        fn camera_ready(self: &RecordingBar) -> bool;
        fn self_view_visible(self: &RecordingBar) -> bool;
        fn self_view_safe(self: &RecordingBar) -> bool;
        fn self_view_monitor(self: &RecordingBar) -> QString;
        fn self_view_pixels(self: &RecordingBar) -> i32;
        fn self_view_x(self: &RecordingBar) -> i32;
        fn self_view_y(self: &RecordingBar) -> i32;

        #[qinvokable]
        fn rotate_camera(self: Pin<&mut RecordingBar>);
        #[qinvokable]
        fn flip_camera(self: Pin<&mut RecordingBar>);
        #[qinvokable]
        fn cycle_camera_shape(self: Pin<&mut RecordingBar>);
        #[qinvokable]
        fn snap_self_view_to_next_corner(self: Pin<&mut RecordingBar>);
        #[qinvokable]
        fn global_cursor_pos(self: &RecordingBar) -> QPointF;
        #[qinvokable]
        fn begin_drag(self: Pin<&mut RecordingBar>, global_position: &QPointF);
        #[qinvokable]
        fn drag_to(self: Pin<&mut RecordingBar>, global_position: &QPointF);
        #[qinvokable]
        fn end_drag(self: Pin<&mut RecordingBar>);
        #[qinvokable]
        fn move_self_view(self: Pin<&mut RecordingBar>, delta_x: i32, delta_y: i32);
        #[qinvokable]
        fn set_self_view_position(self: Pin<&mut RecordingBar>, x: i32, y: i32);
        #[qinvokable]
        fn set_self_view_visible(self: Pin<&mut RecordingBar>, visible: bool);
        #[qinvokable]
        fn stop(self: Pin<&mut RecordingBar>);
        #[qinvokable]
        fn cancel(self: Pin<&mut RecordingBar>);

        /// The logical size of the screen the self-view is on, which keeps the bubble on it.
        fn set_self_view_screen_size(self: Pin<&mut RecordingBar>, size: &QSize);
        /// For main.cpp's window glue: whether this standby host is the one the self-view host
        /// file names (another host was there first otherwise).
        fn owns_standby_host(self: &RecordingBar) -> bool;
        /// The bar's window size, which the recorder keeps out of the capture.
        fn publish_bar_geometry(self: &RecordingBar, width: i32, height: i32);
        /// The countdown window's size; the recorder starts the countdown once it is published.
        fn publish_countdown_geometry(self: &RecordingBar, width: i32, height: i32);
        /// The startup trace line for a stage the windows reach (`OMAREEL_DEBUG=1`).
        fn log_ui_stage(self: &RecordingBar, stage: &QString);

        #[cxx_override]
        fn timer_fired(self: Pin<&mut RecordingBar>, timer: i32);
        #[cxx_override]
        fn called_later(self: Pin<&mut RecordingBar>, call: i32);
        #[cxx_override]
        fn state_directory_changed(self: Pin<&mut RecordingBar>);
        #[cxx_override]
        fn handle_camera_ready_changed(self: Pin<&mut RecordingBar>);
        #[cxx_override]
        fn handle_camera_error(self: Pin<&mut RecordingBar>, message: &QString);
        #[cxx_override]
        fn handle_camera_stopped(self: Pin<&mut RecordingBar>);
    }

    #[namespace = "Omareel"]
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[inherit]
        fn start_bar_timer(self: Pin<&mut RecordingBar>, timer: i32);
        #[inherit]
        fn stop_bar_timer(self: Pin<&mut RecordingBar>, timer: i32);
        #[inherit]
        fn bar_timer_active(self: &RecordingBar, timer: i32) -> bool;
        #[inherit]
        fn call_later(self: Pin<&mut RecordingBar>, call: i32);
        #[inherit]
        fn watch_directory(self: Pin<&mut RecordingBar>, path: &QString);
        #[inherit]
        fn try_lock_host(self: Pin<&mut RecordingBar>, path: &QString) -> bool;
        #[inherit]
        #[cxx_name = "createCamera"]
        fn create_camera_capture(
            self: Pin<&mut RecordingBar>,
            device_path: &QString,
            requested_height: i32,
            video_path: &QString,
            timestamp_path: &QString,
        );
        #[inherit]
        fn destroy_camera(self: Pin<&mut RecordingBar>);
        #[inherit]
        fn reattach_camera_output(self: Pin<&mut RecordingBar>);
        #[inherit]
        fn has_camera(self: &RecordingBar) -> bool;
        #[inherit]
        fn camera_capture_ready(self: &RecordingBar) -> bool;
        #[inherit]
        fn camera_capture_width(self: &RecordingBar) -> i32;
        #[inherit]
        fn camera_capture_height(self: &RecordingBar) -> i32;
        #[inherit]
        fn camera_frame_rate(self: &RecordingBar) -> f64;
        #[inherit]
        fn set_camera_recording_output(self: Pin<&mut RecordingBar>, video_path: &QString, timestamp_path: &QString);
        #[inherit]
        fn begin_camera_recording(self: Pin<&mut RecordingBar>);
        #[inherit]
        fn stop_camera(self: Pin<&mut RecordingBar>);
    }

    impl cxx_qt::Constructor<(bool, bool, i64), NewArguments = (bool, bool, i64)> for RecordingBar {}
}

// RecordingBarBase::Timer and RecordingBarBase::Call.
const POLL_TIMER: i32 = 0;
const POSITION_WRITE_TIMER: i32 = 1;
const SAVE_SELF_VIEW_TIMER: i32 = 2;
const FINISHED_CALL: i32 = 0;
const POLL_CALL: i32 = 1;

fn cached_preferred_encoder_available() -> bool {
    probe::cached_preferred_encoder_available()
}

fn qstring(text: &str) -> QString {
    QString::from(text)
}

fn object<const N: usize>(entries: [(&str, omareel_core::compat::qjson::Value); N]) -> Object {
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

/// The `RecordingBar`'s Rust side, with `RecordingBar.h`'s defaults.
pub struct RecordingBarRust {
    standby: bool,
    adopted: bool,
    suppressed: bool,
    standby_monitor: String,
    standby_position: Point,
    quit: bool,
    owner: i64,
    host_sequence: i64,
    /// The standby host's lock is held (RecordingBarBase holds it).
    host_locked: bool,
    elapsed: String,
    recorded_monitor: String,
    webcam: bool,
    hidden: bool,
    capture_started: bool,
    /// Stop was pressed and the recording is being saved.
    stopping: bool,
    countdown_active: bool,
    countdown_value: i32,
    camera_rotation: i32,
    camera_flip_horizontal: bool,
    shape: CameraShape,
    camera_error: String,
    self_view_visible: bool,
    self_view_safe: bool,
    self_view_monitor: String,
    /// The bubble's screen size, size, position and last corner.
    geometry: SelfViewGeometry,
    drag_press_pointer: PointF,
    drag_press_bubble: Point,
    drag_active: bool,
    command_sequence: i64,
    camera_stop_requested: bool,
    camera_record_requested: bool,
    camera_failed: bool,
    /// The `captureStageLogged` property the C++ set on itself once the capture was seen.
    capture_stage_logged: bool,
}

impl Drop for RecordingBarRust {
    /// `~RecordingBar`: the standby host that held the lock removes its file. RecordingBarBase
    /// then destroys the camera and releases the lock.
    fn drop(&mut self) {
        if self.standby && self.host_locked {
            qfile::remove(&state::self_view_host_path());
        }
    }
}

impl cxx_qt::Constructor<(bool, bool, i64)> for qobject::RecordingBar {
    type BaseArguments = ();
    type InitializeArguments = ();
    type NewArguments = (bool, bool, i64);

    fn route_arguments(arguments: (bool, bool, i64)) -> (Self::NewArguments, Self::BaseArguments, Self::InitializeArguments) {
        (arguments, (), ())
    }

    /// The member initializers of `RecordingBar(hidden, standby, owner)`.
    fn new((hidden, standby, owner): (bool, bool, i64)) -> RecordingBarRust {
        let overlay_monitor = state::recording_state();
        let recorded_monitor = overlay_monitor.value("overlay_monitor").to_string_or(&state::recorded_monitor());
        RecordingBarRust {
            standby,
            adopted: false,
            suppressed: false,
            standby_monitor: String::new(),
            standby_position: Point::default(),
            quit: false,
            owner,
            host_sequence: 0,
            host_locked: false,
            elapsed: "00:00".into(),
            recorded_monitor,
            webcam: state::recording_has_webcam(),
            hidden,
            capture_started: false,
            stopping: false,
            countdown_active: false,
            countdown_value: 3,
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
        }
    }

    fn initialize(self: Pin<&mut Self>, _: ()) {
        self.construct();
    }
}

impl qobject::RecordingBar {
    /// The body of `RecordingBar(hidden, standby, owner)`.
    fn construct(mut self: Pin<&mut Self>) {
        let mut state = state::recording_state();
        if self.standby {
            let host_path = state::self_view_host_path();
            qdir::mkpath(&FileInfo::new(host_path.clone()).absolute_path());
            if !self.as_mut().try_lock_host(&qstring(&format!("{host_path}.host-lock"))) {
                self.as_mut().call_later(FINISHED_CALL);
                return;
            }
            self.as_mut().rust_mut().host_locked = true;
            // Holding the host lock proves that no previous host owns this state.
            // QLockFile removes a crashed process's stale lock by PID, immediately.
            qfile::remove(&host_path);
            let preferences = RecordingPreferences::load();
            let placement = recording_bar::standby_placement(&preferences);
            {
                let mut rust = self.as_mut().rust_mut();
                rust.recorded_monitor = placement.recorded_monitor;
                rust.webcam = true;
            }
            state = placement.state;
            let _ = state::update_state_file(&host_path, &object([("command", "".into()), ("command_sequence", 0.into())]));
        }
        {
            let preferences = RecordingPreferences::load();
            let mut rust = self.as_mut().rust_mut();
            rust.camera_rotation = state.value("camera_rotation").to_int(0);
            rust.camera_flip_horizontal = state.value("camera_flip_horizontal").to_bool(false);
            // The project's vocabulary: round, or square with a corner radius (0 for sharp corners).
            rust.shape = CameraShape::from_preferences(&preferences);
            rust.self_view_visible = state.value("selfview").to_bool(false);
            rust.self_view_safe = state.value("selfview_safe").to_bool(false);
            rust.self_view_monitor = state.value("selfview_monitor").to_string_or(&rust.recorded_monitor);
            rust.geometry.pixels = state.value("selfview_pixels").to_int(160);
            rust.geometry.x = state.value("selfview_x").to_int(16);
            rust.geometry.y = state.value("selfview_y").to_int(16);
            rust.capture_started = state.value("capture_started").to_bool(false);
            rust.countdown_active = state.value("countdown_active").to_bool(false);
            rust.command_sequence = state.value("recording_bar_command_sequence").to_variant_long_long();
        }
        if self.webcam {
            self.as_mut().create_camera(&state);
        }
        let directory = FileInfo::new(state::state_file_path()).absolute_path();
        self.as_mut().watch_directory(&qstring(&directory));
        if self.standby {
            self.as_mut().publish_host();
        }
        self.as_mut().start_bar_timer(POLL_TIMER);
        // Let main connect finished() before an invalid/dead owner can end the host.
        self.call_later(POLL_CALL);
    }

    // ---------------------------------------------------------------------------------------
    // Properties.

    fn bubble_mapped(&self) -> bool {
        self.self_view_visible && !self.suppressed && self.self_view_safe && (self.standby || self.capture_started)
    }

    fn elapsed(&self) -> QString {
        qstring(&self.elapsed)
    }

    fn recorded_monitor(&self) -> QString {
        qstring(&self.recorded_monitor)
    }

    fn webcam(&self) -> bool {
        self.webcam
    }

    fn hidden(&self) -> bool {
        self.hidden
    }

    fn capture_started(&self) -> bool {
        self.capture_started
    }

    fn stopping(&self) -> bool {
        self.stopping
    }

    fn countdown_active(&self) -> bool {
        self.countdown_active
    }

    fn countdown_value(&self) -> i32 {
        self.countdown_value
    }

    fn camera_rotation(&self) -> i32 {
        self.camera_rotation
    }

    fn camera_flip_horizontal(&self) -> bool {
        self.camera_flip_horizontal
    }

    fn camera_shape(&self) -> QString {
        qstring(&self.shape.shape)
    }

    fn camera_radius(&self) -> i32 {
        self.shape.radius
    }

    fn camera_error(&self) -> QString {
        qstring(&self.camera_error)
    }

    fn camera_ready(&self) -> bool {
        self.camera_capture_ready()
    }

    fn self_view_visible(&self) -> bool {
        self.self_view_visible
    }

    fn self_view_safe(&self) -> bool {
        self.self_view_safe
    }

    fn self_view_monitor(&self) -> QString {
        qstring(&self.self_view_monitor)
    }

    fn self_view_pixels(&self) -> i32 {
        self.geometry.pixels
    }

    fn self_view_x(&self) -> i32 {
        self.geometry.x
    }

    fn self_view_y(&self) -> i32 {
        self.geometry.y
    }

    /// The `camera_status` the bar reports: failed, ready once the camera shows frames, else
    /// starting.
    fn camera_status(&self) -> &'static str {
        if self.camera_failed {
            "failed"
        } else if self.camera_capture_ready() {
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

    // ---------------------------------------------------------------------------------------
    // The poll.

    fn timer_fired(self: Pin<&mut Self>, timer: i32) {
        match timer {
            POSITION_WRITE_TIMER => self.publish_geometry(),
            SAVE_SELF_VIEW_TIMER => self.save_self_view_position(),
            _ => self.poll(),
        }
    }

    fn called_later(self: Pin<&mut Self>, call: i32) {
        match call {
            FINISHED_CALL => self.finished(),
            _ => self.poll(),
        }
    }

    fn state_directory_changed(self: Pin<&mut Self>) {
        self.poll();
    }

    /// `RecordingBar::poll`: the recording's state, and for a standby host the launcher's
    /// commands, the adoption handshake and the return to standby.
    fn poll(mut self: Pin<&mut Self>) {
        let active = state::is_recording();
        let mut state = state::recording_state();
        if self.standby {
            let host = state::read_state_file(&state::self_view_host_path());
            let sequence = host.value("command_sequence").to_variant_long_long();
            if sequence != self.host_sequence {
                self.as_mut().rust_mut().host_sequence = sequence;
                self.as_mut().host_command(&host.value("command").to_string_or(""));
            }
            let ours = active && state.value("host_pid").to_variant_long_long() == application_pid();
            if ours && !self.adopted {
                let fallback = state.value("overlay_exclusion").to_string_or("") == "fallback";
                {
                    let mut rust = self.as_mut().rust_mut();
                    rust.adopted = true;
                    rust.standby_monitor = rust.self_view_monitor.clone();
                    rust.standby_position = Point::new(rust.geometry.x, rust.geometry.y);
                    rust.recorded_monitor = state.value("overlay_monitor").to_string_or("");
                }
                if fallback {
                    {
                        let mut rust = self.as_mut().rust_mut();
                        rust.self_view_monitor = state.value("selfview_monitor").to_string_or("");
                        rust.self_view_safe = state.value("selfview_safe").to_bool(false);
                    }
                    self.as_mut().self_view_placement_changed();
                    self.as_mut().self_view_visibility_changed();
                }
                self.as_mut().rust_mut().hidden = !state.value("bar_visible").to_bool(true);
                let bundle = state.value("bundle").to_string_or("");
                self.as_mut().set_camera_recording_output(
                    &qstring(&format!("{bundle}/camera.mp4")),
                    &qstring(&format!("{bundle}/camera.mp4.ts")),
                );
                self.as_mut().publish_geometry();
                update_recording_state(&object([
                    ("host_acknowledged", true.into()),
                    ("countdown_ready", true.into()),
                    ("camera_rotation", self.camera_rotation.into()),
                    ("camera_flip_horizontal", self.camera_flip_horizontal.into()),
                    ("camera_shape", self.shape.shape.as_str().into()),
                    ("camera_radius", self.shape.radius.into()),
                    ("camera_status", self.camera_status().into()),
                    ("camera_width", self.camera_capture_width().into()),
                    ("camera_height_actual", self.camera_capture_height().into()),
                    ("camera_fps", self.camera_frame_rate().into()),
                ]));
                state = state::recording_state();
                self.as_mut().capture_started_changed();
            }
            let suppressed = active && !ours;
            if suppressed != self.suppressed {
                self.as_mut().rust_mut().suppressed = suppressed;
                self.as_mut().self_view_visibility_changed();
            }
            let owner = self.owner;
            let owner_alive = owner > 0 && (kill(owner, 0) == 0 || errno() == libc::EPERM);
            if self_view_host_should_exit(active, self.quit, owner, owner_alive) {
                self.as_mut().stop_bar_timer(POLL_TIMER);
                self.finished();
                return;
            }
            if !active && self.adopted {
                {
                    let mut rust = self.as_mut().rust_mut();
                    rust.adopted = false;
                    rust.capture_started = false;
                    rust.hidden = true;
                    rust.self_view_safe = true;
                    rust.self_view_monitor = rust.standby_monitor.clone();
                    rust.geometry.x = rust.standby_position.x;
                    rust.geometry.y = rust.standby_position.y;
                    rust.camera_record_requested = false;
                    rust.camera_stop_requested = false;
                    rust.stopping = false;
                }
                self.as_mut().stopping_changed();
                self.as_mut().capture_started_changed();
                self.as_mut().self_view_placement_changed();
                self.as_mut().self_view_visibility_changed();
                self.as_mut().host_command("configure");
                self.as_mut().publish_host();
            }
            if !ours {
                return;
            }
        } else if !active {
            self.as_mut().stop_bar_timer(POLL_TIMER);
            self.finished();
            return;
        }
        let bar_monitor = state.value("overlay_monitor").to_string_or(&self.recorded_monitor);
        let hidden = !state.value("bar_visible").to_bool(!self.hidden);
        if self.recorded_monitor != bar_monitor || self.hidden != hidden {
            {
                let mut rust = self.as_mut().rust_mut();
                rust.recorded_monitor = bar_monitor;
                rust.hidden = hidden;
            }
            self.as_mut().capture_started_changed();
        }
        let safe = state.value("selfview_safe").to_bool(self.self_view_safe);
        let bubble_monitor = state.value("selfview_monitor").to_string_or(&self.self_view_monitor);
        if safe != self.self_view_safe || bubble_monitor != self.self_view_monitor {
            {
                let mut rust = self.as_mut().rust_mut();
                rust.self_view_safe = safe;
                rust.self_view_monitor = bubble_monitor;
            }
            self.as_mut().self_view_placement_changed();
            self.as_mut().self_view_visibility_changed();
        }
        let capture_started = state.value("capture_started").to_bool(false);
        if capture_started != self.capture_started {
            self.as_mut().rust_mut().capture_started = capture_started;
            self.as_mut().self_view_visibility_changed();
            self.as_mut().capture_started_changed();
        }
        if capture_started && env_equals("OMAREEL_DEBUG", "1") && !self.capture_stage_logged {
            self.as_mut().rust_mut().capture_stage_logged = true;
            debug_bar_stage("capture_seen_by_bar", "");
        }
        // Stop can also come from the keybind or the REC indicator.
        if state.value("stopping").to_bool(false) && !self.stopping {
            self.as_mut().rust_mut().stopping = true;
            self.as_mut().stopping_changed();
        }
        let countdown_active = state.value("countdown_active").to_bool(false);
        let countdown_end_us = state.value("countdown_end_us").to_variant_long_long();
        let countdown_value = recording_bar::countdown_value(countdown_end_us, recorder::monotonic_us());
        if countdown_active != self.countdown_active || countdown_value != self.countdown_value {
            {
                let mut rust = self.as_mut().rust_mut();
                rust.countdown_active = countdown_active;
                rust.countdown_value = countdown_value;
            }
            self.as_mut().countdown_changed();
        }
        let requested_self_view = state.value("selfview").to_bool(false);
        if requested_self_view != self.self_view_visible {
            self.as_mut().rust_mut().self_view_visible = requested_self_view;
            self.as_mut().self_view_visibility_changed();
        }
        let requested_x = state.value("selfview_x").to_int(self.geometry.x);
        let requested_y = state.value("selfview_y").to_int(self.geometry.y);
        let next = self.geometry.clamped_requested_position(requested_x, requested_y);
        if !self.drag_active
            && !self.bar_timer_active(POSITION_WRITE_TIMER)
            && (next.x != self.geometry.x || next.y != self.geometry.y)
        {
            {
                let mut rust = self.as_mut().rust_mut();
                rust.geometry.x = next.x;
                rust.geometry.y = next.y;
            }
            self.as_mut().self_view_placement_changed();
        }
        let command_sequence = state.value("recording_bar_command_sequence").to_variant_long_long();
        if command_sequence != self.command_sequence {
            self.as_mut().rust_mut().command_sequence = command_sequence;
            match state.value("recording_bar_command").to_string_or("").as_str() {
                "rotate" => self.as_mut().rotate_camera(),
                "flip" => self.as_mut().flip_camera(),
                "drag-sim" => {
                    let point = |x: &str, y: &str| {
                        QPointF::new(state.value(x).to_double(0.0), state.value(y).to_double(0.0))
                    };
                    self.as_mut().begin_drag(&point("drag_x0", "drag_y0"));
                    self.as_mut().drag_to(&point("drag_x1", "drag_y1"));
                    self.as_mut().end_drag();
                }
                _ => {}
            }
        }
        if self.has_camera() && state.value("camera_record").to_bool(false) && !self.camera_record_requested {
            self.as_mut().rust_mut().camera_record_requested = true;
            debug_bar_stage("camera_record_begin", "");
            self.as_mut().begin_camera_recording();
        }
        if self.has_camera() && state.value("camera_stop").to_bool(false) && !self.camera_stop_requested {
            self.as_mut().rust_mut().camera_stop_requested = true;
            self.as_mut().stop_camera();
        }
        if self.stopping {
            return;
        }
        let elapsed = recording_bar::elapsed_text(recorder::monotonic_us(), state::recording_started_us());
        if elapsed != self.elapsed {
            self.as_mut().rust_mut().elapsed = elapsed;
            self.elapsed_changed();
        }
    }

    // ---------------------------------------------------------------------------------------
    // The camera's settings.

    fn cycle_camera_shape(mut self: Pin<&mut Self>) {
        // Round, then square with rounded corners, then square with sharp corners.
        self.as_mut().rust_mut().shape.cycle();
        let mut preferences = RecordingPreferences::load();
        preferences.webcam.insert("shape", self.shape.shape.as_str());
        preferences.webcam.insert("radius", self.shape.radius);
        let _ = preferences.save();
        if self.writes_recording_state() {
            update_recording_state(&object([
                ("camera_shape", self.shape.shape.as_str().into()),
                ("camera_radius", self.shape.radius.into()),
            ]));
        }
        self.as_mut().publish_host();
        self.camera_settings_changed();
    }

    fn log_camera_settings(&self) {
        if env_is_set("OMAREEL_CAMERA_SETTINGS_LOG") {
            qobject::log_info(&qstring(&format!(
                "camera rotation={} flip={}",
                self.camera_rotation, self.camera_flip_horizontal
            )));
        }
    }

    fn rotate_camera(mut self: Pin<&mut Self>) {
        let rotation = (self.camera_rotation + 90) % 360;
        self.as_mut().rust_mut().camera_rotation = rotation;
        let mut preferences = RecordingPreferences::load();
        preferences.webcam_rotation = rotation;
        preferences.webcam.insert("rotation", rotation);
        let _ = preferences.save();
        if self.writes_recording_state() {
            update_recording_state(&object([("camera_rotation", rotation.into())]));
        }
        self.as_mut().publish_host();
        self.log_camera_settings();
        self.camera_settings_changed();
    }

    fn flip_camera(mut self: Pin<&mut Self>) {
        let flip = !self.camera_flip_horizontal;
        self.as_mut().rust_mut().camera_flip_horizontal = flip;
        let mut preferences = RecordingPreferences::load();
        preferences.webcam_flip_horizontal = flip;
        preferences.webcam.insert("flipHorizontal", flip);
        let _ = preferences.save();
        if self.writes_recording_state() {
            update_recording_state(&object([("camera_flip_horizontal", flip.into())]));
        }
        self.as_mut().publish_host();
        self.log_camera_settings();
        self.camera_settings_changed();
    }

    // ---------------------------------------------------------------------------------------
    // The self-view's placement.

    /// What `setSelfViewPosition` does once the bubble moved: the position is written soon
    /// (unless a write is already due) and saved a little later, and the placement announced.
    fn self_view_moved(mut self: Pin<&mut Self>) {
        if !self.bar_timer_active(POSITION_WRITE_TIMER) {
            self.as_mut().start_bar_timer(POSITION_WRITE_TIMER);
        }
        self.as_mut().start_bar_timer(SAVE_SELF_VIEW_TIMER);
        self.self_view_placement_changed();
    }

    fn snap_self_view_to_next_corner(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().geometry.snap_to_next_corner() {
            self.self_view_moved();
        }
    }

    fn global_cursor_pos(&self) -> QPointF {
        let point = recording_bar::global_cursor_position().unwrap_or(self.drag_press_pointer);
        QPointF::new(point.x, point.y)
    }

    fn begin_drag(mut self: Pin<&mut Self>, global_position: &QPointF) {
        let mut rust = self.as_mut().rust_mut();
        rust.drag_press_pointer = PointF::new(global_position.x(), global_position.y());
        rust.drag_press_bubble = Point::new(rust.geometry.x, rust.geometry.y);
        rust.drag_active = true;
    }

    fn drag_to(self: Pin<&mut Self>, global_position: &QPointF) {
        if !self.drag_active || self.geometry.screen_size.is_empty() {
            return;
        }
        let pointer = PointF::new(global_position.x(), global_position.y());
        let position = clamped_self_view_drag_position(
            self.drag_press_pointer,
            self.drag_press_bubble,
            pointer,
            self.geometry.screen_size,
            self.geometry.pixels,
        );
        if env_is_set("OMAREEL_SELFVIEW_DRAG_LOG") {
            let real = |value: f64| qobject::debug_real(value).to_string();
            let delta = pointer - self.drag_press_pointer;
            qobject::log_info(&qstring(&format!(
                "pointer=({},{}) bubble=({},{}) delta=({},{})",
                real(pointer.x),
                real(pointer.y),
                position.x,
                position.y,
                real(delta.x),
                real(delta.y)
            )));
        }
        self.set_self_view_position(position.x, position.y);
    }

    fn end_drag(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().drag_active = false;
        self.as_mut().stop_bar_timer(POSITION_WRITE_TIMER);
        self.as_mut().publish_geometry();
        self.as_mut().stop_bar_timer(SAVE_SELF_VIEW_TIMER);
        self.save_self_view_position();
    }

    fn set_self_view_screen_size(mut self: Pin<&mut Self>, size: &QSize) {
        self.as_mut().rust_mut().geometry.set_screen_size(Size::new(size.width(), size.height()));
        self.self_view_placement_changed();
    }

    fn move_self_view(self: Pin<&mut Self>, delta_x: i32, delta_y: i32) {
        let (x, y) = (self.geometry.x.wrapping_add(delta_x), self.geometry.y.wrapping_add(delta_y));
        self.set_self_view_position(x, y);
    }

    fn set_self_view_position(mut self: Pin<&mut Self>, x: i32, y: i32) {
        if self.as_mut().rust_mut().geometry.set_position(x, y) {
            self.self_view_moved();
        }
    }

    fn set_self_view_visible(mut self: Pin<&mut Self>, visible: bool) {
        if !self.webcam || self.self_view_visible == visible {
            return;
        }
        self.as_mut().rust_mut().self_view_visible = visible;
        let mut preferences = RecordingPreferences::load();
        preferences.self_view_enabled = visible;
        let _ = preferences.save();
        self.as_mut().publish_geometry();
        self.self_view_visibility_changed();
    }

    fn stop(mut self: Pin<&mut Self>) {
        if self.stopping || state::signal_existing(false).is_err() {
            return;
        }
        self.as_mut().rust_mut().stopping = true;
        self.stopping_changed();
    }

    fn cancel(self: Pin<&mut Self>) {
        let _ = state::signal_existing(true);
    }

    /// `publishGeometry`: the bubble into the recording's state and the host file.
    fn publish_geometry(self: Pin<&mut Self>) {
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
    fn publish_host(self: Pin<&mut Self>) {
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
    fn camera_state(self: Pin<&mut Self>, values: &Object) {
        if self.writes_recording_state() {
            update_recording_state(values);
        }
        self.publish_host();
    }

    /// `hostCommand`: what the launcher asked the standby host through its file.
    fn host_command(mut self: Pin<&mut Self>, command: &str) {
        match command {
            "quit" => self.as_mut().rust_mut().quit = true,
            "configure" if !self.adopted => {
                let preferences = RecordingPreferences::load();
                self.as_mut().destroy_camera();
                {
                    let mut rust = self.as_mut().rust_mut();
                    rust.camera_failed = false;
                    rust.camera_error.clear();
                }
                self.as_mut().camera_error_changed();
                self.as_mut().create_camera(&object([
                    ("camera_device", preferences.webcam_device.as_str().into()),
                    ("camera_height", preferences.webcam_height.into()),
                ]));
                self.as_mut().reattach_camera_output();
                self.publish_host();
            }
            "rotate" => self.rotate_camera(),
            "flip" => self.flip_camera(),
            "shape" => self.cycle_camera_shape(),
            "corner" => self.snap_self_view_to_next_corner(),
            "show" | "hide" => self.set_self_view_visible(command == "show"),
            "S" | "M" | "L" => {
                self.as_mut().rust_mut().geometry.pixels = self_view_pixels(command);
                let mut preferences = RecordingPreferences::load();
                preferences.self_view_size = command.to_owned();
                let _ = preferences.save();
                let screen_size = self.geometry.screen_size;
                self.as_mut().rust_mut().geometry.set_screen_size(screen_size);
                self.as_mut().self_view_placement_changed();
                self.publish_geometry();
            }
            _ => {}
        }
    }

    /// `saveSelfViewPosition`: the bubble's place as fractions of its screen, which the next
    /// standby host starts from; not while an adopted host shows the fallback placement.
    fn save_self_view_position(self: Pin<&mut Self>) {
        if self.adopted && state::recording_state().value("overlay_exclusion").to_string_or("") == "fallback" {
            return;
        }
        let Some((x, y)) = self.geometry.saved_position() else { return };
        let mut preferences = RecordingPreferences::load();
        preferences.self_view_x = x;
        preferences.self_view_y = y;
        let _ = preferences.save();
    }

    // ---------------------------------------------------------------------------------------
    // The camera.

    /// `createCamera(state)`: the camera named in the state, recording into the bundle's
    /// `camera.mp4` when the state has a bundle.
    fn create_camera(mut self: Pin<&mut Self>, state: &Object) {
        let bundle = state.value("bundle").to_string_or("");
        let in_bundle = |name: &str| if bundle.is_empty() { String::new() } else { format!("{bundle}/{name}") };
        self.as_mut().create_camera_capture(
            &qstring(&state.value("camera_device").to_string_or("")),
            state.value("camera_height").to_int(1080),
            &qstring(&in_bundle("camera.mp4")),
            &qstring(&in_bundle("camera.mp4.ts")),
        );
        self.camera_ready_changed();
    }

    fn handle_camera_ready_changed(mut self: Pin<&mut Self>) {
        self.as_mut().camera_ready_changed();
        if !self.camera_capture_ready() {
            return;
        }
        let values = object([
            ("camera_status", "ready".into()),
            ("camera_backend", "ffmpeg-pipe".into()),
            ("camera_width", self.camera_capture_width().into()),
            ("camera_height_actual", self.camera_capture_height().into()),
            ("camera_fps", self.camera_frame_rate().into()),
        ]);
        self.camera_state(&values);
        debug_bar_stage("camera_ready", "");
    }

    fn handle_camera_error(mut self: Pin<&mut Self>, message: &QString) {
        let message = message.to_string();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.camera_failed = true;
            rust.camera_error = message.clone();
        }
        self.as_mut().camera_error_changed();
        self.camera_state(&object([("camera_status", "releasing".into()), ("camera_error", message.into())]));
    }

    fn handle_camera_stopped(self: Pin<&mut Self>) {
        let status = if self.camera_failed { "failed" } else { "stopped" };
        self.camera_state(&object([("camera_status", status.into())]));
    }

    // ---------------------------------------------------------------------------------------
    // main.cpp's window glue.

    fn owns_standby_host(&self) -> bool {
        state::read_state_file(&state::self_view_host_path()).value("pid").to_variant_long_long() == application_pid()
    }

    fn publish_bar_geometry(&self, width: i32, height: i32) {
        update_recording_state(&object([("bar_width", width.into()), ("bar_height", height.into())]));
    }

    fn publish_countdown_geometry(&self, width: i32, height: i32) {
        update_recording_state(&object([
            ("countdown_width", width.into()),
            ("countdown_height", height.into()),
            ("countdown_ready", true.into()),
        ]));
    }

    /// `debugUiStage(stage)`, which main.cpp kept for the bar's window: unlike the bar's own
    /// trace it always prints `elapsed_ms`, 0 without the daemon's start time.
    fn log_ui_stage(&self, stage: &QString) {
        if !env_equals("OMAREEL_DEBUG", "1") {
            return;
        }
        let monotonic_us = recorder::monotonic_us();
        let daemon_start_us = state::recording_state().value("daemon_started_us").to_variant_long_long();
        let elapsed_ms = if daemon_start_us > 0 { (monotonic_us - daemon_start_us) as f64 / 1000.0 } else { 0.0 };
        append_debug_log(&format!("OMAREEL_START stage={stage} monotonic_us={monotonic_us} elapsed_ms={elapsed_ms:.3}\n"));
    }
}
