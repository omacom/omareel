//! The `launcher` object of the window `omareel` opens to start a recording. It replaced
//! src/ui/Launcher.cpp and src/ui/MicrophoneMeter.cpp and keeps the launcher's QML surface: the
//! same properties (the settings read and written, sharing `recordingPreferencesChanged`), the
//! constant capture exclusion note, the invokables and the signals `quitRequested`,
//! `errorOccurred` and `recordingStarting`.
//!
//! What the launcher decides is `omareel_record::launcher`; the recorder's state files are
//! `omareel_record::recorder::state`, the preferences `omareel_core::recording_preferences`.
//! `LauncherBase` (crates/omareel_qt/cpp) owns QMediaDevices, the cameras Qt lists, the
//! microphone meter's QAudioSource and the timers, and reports to the overrides at the end of the
//! bridge. `omareel record` runs on a thread of its own and its result is queued back, as the
//! QProcess's `finished` was; a run still going when the launcher goes is killed, as destroying
//! the QProcess did.

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qprocess::{ChannelMode, ExitStatus, Process, ProcessState};
use omareel_core::compat::qstring;
use omareel_core::microphone_meter::{MicrophoneLevel, SampleFormat};
use omareel_core::omarchy_paths;
use omareel_core::recording_preferences::RecordingPreferences;
use omareel_record::camera::{self, CameraDevice, CameraFormat};
use omareel_record::capture_exclusion;
use omareel_record::launcher::{self, AudioDevice, WebcamDevice};
use omareel_record::recorder::{self, state};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crate::support::{env_is_empty, kill, print_error};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        // QtShims.h first: it brings in QString whole before cxx-qt-lib's qstring.h (see
        // export.rs).
        include!("QtShims.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    #[namespace = "Omareel"]
    extern "Rust" {
        /// The cameras Qt lists, as LauncherBase hands them over: each camera's id and
        /// description, then its formats (Qt keeps the frame rates as floats).
        type CameraList;
        #[cxx_name = "addCamera"]
        fn add_camera(self: &mut CameraList, id: &QByteArray, description: &QString);
        #[cxx_name = "addFormat"]
        fn add_format(self: &mut CameraList, width: i32, height: i32, min_frame_rate: f32, max_frame_rate: f32);
    }

    #[namespace = "Omareel"]
    unsafe extern "C++Qt" {
        include!("LauncherBase.h");
        #[qobject]
        type LauncherBase;
    }

    #[namespace = "Omareel::LauncherShims"]
    unsafe extern "C++" {
        #[cxx_name = "applicationDirPath"]
        fn application_dir_path() -> QString;
        #[cxx_name = "applicationFilePath"]
        fn application_file_path() -> QString;
        #[cxx_name = "bundlePath"]
        fn bundle_path(value: &QString) -> QString;
        #[cxx_name = "startDetached"]
        fn start_detached(program: &QString, arguments: &QStringList) -> bool;
        #[cxx_name = "openFolder"]
        fn open_folder(path: &QString) -> bool;
    }

    #[namespace = "Omareel"]
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = LauncherBase]
        #[qproperty(QString, capture_exclusion_note, READ = capture_exclusion_note, CONSTANT)]
        #[qproperty(QString, capture_exclusion_reason, READ = capture_exclusion_reason, CONSTANT)]
        #[qproperty(QString, self_view_status, READ = self_view_status, NOTIFY = self_view_status_changed)]
        #[qproperty(bool, recording, READ = recording, NOTIFY = recording_changed)]
        #[qproperty(bool, starting_recording, READ = starting_recording, NOTIFY = starting_recording_changed)]
        #[qproperty(bool, stopping_recording, READ = stopping_recording, NOTIFY = stopping_recording_changed)]
        #[qproperty(QString, recording_elapsed, READ = recording_elapsed, NOTIFY = recording_elapsed_changed)]
        #[qproperty(bool, system_audio, READ = system_audio, WRITE = set_system_audio, NOTIFY = recording_preferences_changed)]
        #[qproperty(bool, microphone, READ = microphone, WRITE = set_microphone, NOTIFY = recording_preferences_changed)]
        #[qproperty(QString, microphone_device, READ = microphone_device, WRITE = set_microphone_device, NOTIFY = recording_preferences_changed)]
        #[qproperty(QList_QVariant, audio_devices, READ = audio_devices, NOTIFY = audio_devices_changed)]
        #[qproperty(f64, microphone_level, READ = microphone_level, NOTIFY = microphone_level_changed)]
        #[qproperty(bool, webcam, READ = webcam, WRITE = set_webcam, NOTIFY = recording_preferences_changed)]
        #[qproperty(QString, webcam_device, READ = webcam_device, WRITE = set_webcam_device, NOTIFY = recording_preferences_changed)]
        #[qproperty(i32, webcam_height, READ = webcam_height, WRITE = set_webcam_height, NOTIFY = recording_preferences_changed)]
        #[qproperty(i32, webcam_rotation, READ = webcam_rotation, WRITE = set_webcam_rotation, NOTIFY = recording_preferences_changed)]
        #[qproperty(bool, webcam_flip_horizontal, READ = webcam_flip_horizontal, WRITE = set_webcam_flip_horizontal, NOTIFY = recording_preferences_changed)]
        #[qproperty(bool, self_view_enabled, READ = self_view_enabled, WRITE = set_self_view_enabled, NOTIFY = recording_preferences_changed)]
        #[qproperty(QString, self_view_size, READ = self_view_size, WRITE = set_self_view_size, NOTIFY = recording_preferences_changed)]
        #[qproperty(bool, countdown_before_recording, READ = countdown_before_recording, WRITE = set_countdown_before_recording, NOTIFY = recording_preferences_changed)]
        #[qproperty(QVariant, webcam_camera_device, READ = webcam_camera_device, NOTIFY = recording_preferences_changed)]
        #[qproperty(bool, webcam_preview_available, READ = webcam_preview_available, NOTIFY = webcam_devices_changed)]
        #[qproperty(QList_QVariant, webcam_devices, READ = webcam_devices, NOTIFY = webcam_devices_changed)]
        type Launcher = super::LauncherRust;

        #[qsignal]
        fn self_view_status_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn recording_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn starting_recording_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn stopping_recording_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn recording_elapsed_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn recording_preferences_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn audio_devices_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn microphone_level_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn webcam_devices_changed(self: Pin<&mut Launcher>);
        #[qsignal]
        fn quit_requested(self: Pin<&mut Launcher>);
        #[qsignal]
        fn error_occurred(self: Pin<&mut Launcher>, message: QString);
        #[qsignal]
        fn recording_starting(self: Pin<&mut Launcher>);

        fn capture_exclusion_note(self: &Launcher) -> QString;
        fn capture_exclusion_reason(self: &Launcher) -> QString;
        fn self_view_status(self: &Launcher) -> QString;
        fn recording(self: &Launcher) -> bool;
        fn starting_recording(self: &Launcher) -> bool;
        fn stopping_recording(self: &Launcher) -> bool;
        fn recording_elapsed(self: &Launcher) -> QString;
        fn system_audio(self: &Launcher) -> bool;
        fn microphone(self: &Launcher) -> bool;
        fn microphone_device(self: &Launcher) -> QString;
        fn audio_devices(self: &Launcher) -> QList_QVariant;
        fn microphone_level(self: &Launcher) -> f64;
        fn webcam(self: &Launcher) -> bool;
        fn webcam_device(self: &Launcher) -> QString;
        fn webcam_height(self: &Launcher) -> i32;
        fn webcam_rotation(self: &Launcher) -> i32;
        fn webcam_flip_horizontal(self: &Launcher) -> bool;
        fn self_view_enabled(self: &Launcher) -> bool;
        fn self_view_size(self: &Launcher) -> QString;
        fn countdown_before_recording(self: &Launcher) -> bool;
        fn webcam_camera_device(self: &Launcher) -> QVariant;
        fn webcam_preview_available(self: &Launcher) -> bool;
        fn webcam_devices(self: &Launcher) -> QList_QVariant;

        fn set_system_audio(self: Pin<&mut Launcher>, value: bool);
        fn set_microphone(self: Pin<&mut Launcher>, value: bool);
        fn set_microphone_device(self: Pin<&mut Launcher>, value: &QString);
        fn set_webcam(self: Pin<&mut Launcher>, value: bool);
        fn set_webcam_device(self: Pin<&mut Launcher>, value: &QString);
        fn set_webcam_height(self: Pin<&mut Launcher>, value: i32);
        fn set_webcam_rotation(self: Pin<&mut Launcher>, value: i32);
        fn set_webcam_flip_horizontal(self: Pin<&mut Launcher>, value: bool);
        fn set_self_view_enabled(self: Pin<&mut Launcher>, value: bool);
        fn set_self_view_size(self: Pin<&mut Launcher>, value: &QString);
        fn set_countdown_before_recording(self: Pin<&mut Launcher>, value: bool);

        #[qinvokable]
        fn open_bundle(self: Pin<&mut Launcher>, path: &QString);
        #[qinvokable]
        fn show_recordings_folder(self: Pin<&mut Launcher>);
        #[qinvokable]
        fn record(self: Pin<&mut Launcher>);
        #[qinvokable]
        fn stop_recording(self: Pin<&mut Launcher>);
        #[qinvokable]
        fn cancel_recording(self: Pin<&mut Launcher>);

        #[cxx_override]
        fn handle_video_inputs_changed(self: Pin<&mut Launcher>);
        #[cxx_override]
        fn handle_audio_inputs_changed(self: Pin<&mut Launcher>);
        #[cxx_override]
        fn recording_timer_fired(self: Pin<&mut Launcher>);
        #[cxx_override]
        fn start_timer_fired(self: Pin<&mut Launcher>);
        #[cxx_override]
        fn microphone_read(self: Pin<&mut Launcher>, sample_format: i32, samples: &QByteArray);
    }

    #[namespace = "Omareel"]
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[inherit]
        fn screen_count(self: &Launcher) -> i32;
        #[inherit]
        fn refresh_camera_devices(self: Pin<&mut Launcher>, cameras: &mut CameraList);
        #[inherit]
        fn camera_device_variant(self: &Launcher, index: i32) -> QVariant;
        #[inherit]
        fn listen_to_microphone(self: Pin<&mut Launcher>, device: &QString) -> bool;
        #[inherit]
        fn watch_media_devices(self: Pin<&mut Launcher>);
        #[inherit]
        fn start_recording_timer(self: Pin<&mut Launcher>);
        #[inherit]
        fn start_start_timer(self: Pin<&mut Launcher>);
        #[inherit]
        fn stop_start_timer(self: Pin<&mut Launcher>);
        #[inherit]
        fn start_detached_self_view_host(self: Pin<&mut Launcher>, pid: &mut i64);
    }

    impl cxx_qt::Threading for Launcher {}
    impl cxx_qt::Initialize for Launcher {}
}

/// A run of `omareel record`: its thread, and the flag that asks it to kill the run.
struct RecordJob {
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// The `Launcher`'s Rust side, with `Launcher.h`'s defaults.
pub struct LauncherRust {
    capture_exclusion_note: String,
    capture_exclusion_reason: String,
    self_view_status: String,
    host_pid: i64,
    recording: bool,
    recording_elapsed: String,
    system_audio: bool,
    microphone: bool,
    /// The microphone recorded from: the one last picked while it is plugged in, else the
    /// default.
    microphone_device: String,
    chosen_microphone_device: String,
    audio_devices: QList<QVariant>,
    microphone_level: MicrophoneLevel,
    webcam: bool,
    webcam_device: String,
    webcam_height: i32,
    webcam_rotation: i32,
    webcam_flip_horizontal: bool,
    self_view_enabled: bool,
    self_view_size: String,
    webcam_devices: QList<QVariant>,
    /// The cameras of the last refresh, in LauncherBase's order.
    cameras: Vec<CameraDevice>,
    /// The camera last picked, selected again whenever it is plugged in.
    chosen_webcam_device: String,
    starting_recording: bool,
    stopping_recording: bool,
    countdown_before_recording: bool,
    quit_when_started: bool,
    /// How often `record`'s start timer has waited for the self-view host.
    start_attempts: i32,
    record_jobs: Vec<RecordJob>,
}

impl Default for LauncherRust {
    fn default() -> LauncherRust {
        LauncherRust {
            capture_exclusion_note: String::new(),
            capture_exclusion_reason: String::new(),
            self_view_status: "Starting camera…".into(),
            host_pid: 0,
            recording: false,
            recording_elapsed: "00:00".into(),
            system_audio: true,
            microphone: true,
            microphone_device: "default_input".into(),
            chosen_microphone_device: "default_input".into(),
            audio_devices: QList::default(),
            microphone_level: MicrophoneLevel::default(),
            webcam: false,
            webcam_device: "/dev/video2".into(),
            webcam_height: 1080,
            webcam_rotation: 0,
            webcam_flip_horizontal: false,
            self_view_enabled: true,
            self_view_size: "M".into(),
            webcam_devices: QList::default(),
            cameras: Vec::new(),
            chosen_webcam_device: String::new(),
            starting_recording: false,
            stopping_recording: false,
            countdown_before_recording: false,
            quit_when_started: false,
            start_attempts: 0,
            record_jobs: Vec::new(),
        }
    }
}

impl Drop for LauncherRust {
    /// The launcher's QProcess was its child: a run of `omareel record` still going is killed and
    /// waited for.
    fn drop(&mut self) {
        for job in &self.record_jobs {
            job.cancel.store(true, Ordering::SeqCst);
        }
        for job in &mut self.record_jobs {
            if let Some(worker) = job.worker.take() {
                let _ = worker.join();
            }
        }
    }
}

fn qstring(text: &str) -> QString {
    QString::from(text)
}

fn map<const N: usize>(entries: [(&str, QVariant); N]) -> QVariant {
    let map: QMap<QMapPair_QString_QVariant> = entries.into_iter().map(|(key, value)| (qstring(key), value)).collect();
    QVariant::from(&map)
}

fn text_variant(text: &str) -> QVariant {
    QVariant::from(&qstring(text))
}

fn audio_device_list(devices: &[AudioDevice]) -> QList<QVariant> {
    devices.iter().map(|device| map([("value", text_variant(&device.value)), ("text", text_variant(&device.text))])).collect()
}

fn webcam_device_list(devices: &[WebcamDevice]) -> QList<QVariant> {
    devices
        .iter()
        .map(|device| match &device.node {
            Some(node) => map([
                ("value", text_variant(&device.value)),
                ("text", text_variant(&device.text)),
                ("node", text_variant(node)),
            ]),
            None => map([("value", text_variant(&device.value)), ("text", text_variant(&device.text))]),
        })
        .collect()
}

/// The cameras Qt lists, as plain data in Qt's order.
#[derive(Default)]
pub struct CameraList(Vec<CameraDevice>);

impl CameraList {
    fn add_camera(&mut self, id: &QByteArray, description: &QString) {
        self.0.push(CameraDevice { id: id.as_slice().to_vec(), description: description.to_string(), formats: Vec::new() });
    }

    /// A format of the camera added last.
    fn add_format(&mut self, width: i32, height: i32, min_frame_rate: f32, max_frame_rate: f32) {
        if let Some(camera) = self.0.last_mut() {
            camera.formats.push(CameraFormat {
                pixel_format: String::new(),
                width,
                height,
                min_frame_rate,
                max_frame_rate,
            });
        }
    }
}

/// Whether the launcher runs for a screenshot that must not touch the camera or the microphone
/// (`OMAREEL_SCREENSHOT` without `OMAREEL_SCREENSHOT_LIVE`).
fn screenshot() -> bool {
    !env_is_empty("OMAREEL_SCREENSHOT") && env_is_empty("OMAREEL_SCREENSHOT_LIVE")
}

/// `QProcess::finished`'s arguments and what the run printed, read and trimmed.
struct RecordOutcome {
    exit_code: i32,
    crashed: bool,
    output: String,
}

/// Runs `omareel record` as the launcher's QProcess did, with merged channels, and returns how it
/// ended; `None` when it failed to start (QProcess then emitted no `finished`) or `cancel` asked
/// for it to be killed (dropping the Process kills it and waits, as `~QProcess` did).
fn run_record(program: &str, arguments: &[String], cancel: &AtomicBool) -> Option<RecordOutcome> {
    let mut process = Process::new();
    process.set_process_channel_mode(ChannelMode::Merged);
    process.start(program, arguments);
    if process.state() != ProcessState::Running {
        return None;
    }
    // Short waits, so a launcher closing kills the run about as promptly as ~QProcess did.
    while !process.wait_for_finished(10) {
        if cancel.load(Ordering::SeqCst) || process.state() != ProcessState::Running {
            return None;
        }
    }
    let output = qstring::trimmed(&qstring::from_utf8(&process.read_all())).to_owned();
    Some(RecordOutcome {
        exit_code: process.exit_code(),
        crashed: process.exit_status() == ExitStatus::CrashExit,
        output,
    })
}

impl cxx_qt::Initialize for qobject::Launcher {
    /// `Launcher::Launcher`, after the members LauncherBase holds and its connections.
    fn initialize(mut self: Pin<&mut Self>) {
        let exclusion = capture_exclusion::ensure_loaded(&qobject::application_dir_path().to_string());
        if !exclusion.loaded {
            let screens = usize::try_from(self.screen_count()).unwrap_or(0);
            let mut rust = self.as_mut().rust_mut();
            rust.capture_exclusion_note = launcher::capture_exclusion_note(screens).into();
            rust.capture_exclusion_reason = exclusion.reason.clone();
            print_error(&format!("omareel: {}; recording overlays stay off the recorded screen\n", exclusion.reason));
        }
        let preferences = RecordingPreferences::load();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.system_audio = preferences.system_audio;
            rust.microphone = preferences.microphone;
            rust.microphone_device = preferences.microphone_device.clone();
            rust.chosen_microphone_device = preferences.microphone_device.clone();
            rust.webcam = preferences.webcam_enabled;
            // Older settings saved the /dev/videoN node, which the camera keeps only until it is
            // replugged.
            rust.webcam_device = camera::stable_camera_path(&preferences.webcam_device);
            rust.chosen_webcam_device = camera::stable_camera_path(&preferences.webcam_chosen_device);
            rust.webcam_height = preferences.webcam_height;
            rust.webcam_rotation = preferences.webcam_rotation;
            rust.webcam_flip_horizontal = preferences.webcam_flip_horizontal;
            rust.self_view_enabled = preferences.self_view_enabled;
            rust.self_view_size = preferences.self_view_size.clone();
            rust.countdown_before_recording = preferences.countdown_before_recording;
        }
        self.as_mut().refresh_audio_devices();
        self.as_mut().refresh_webcam_devices();
        if self.webcam_device != preferences.webcam_device || self.chosen_webcam_device != preferences.webcam_chosen_device
        {
            self.as_mut().save_recording_preferences();
        }
        self.as_mut().watch_media_devices();
        self.as_mut().start_recording_timer();
        self.as_mut().refresh_recording();
        self.as_mut().update_microphone_meter();
        if self.webcam && !state::is_recording() {
            self.as_mut().start_self_view_host();
        }
    }
}

impl qobject::Launcher {
    // ---------------------------------------------------------------------------------------
    // Properties.

    fn capture_exclusion_note(&self) -> QString {
        qstring(&self.capture_exclusion_note)
    }

    fn capture_exclusion_reason(&self) -> QString {
        qstring(&self.capture_exclusion_reason)
    }

    fn self_view_status(&self) -> QString {
        qstring(&self.self_view_status)
    }

    fn recording(&self) -> bool {
        self.recording
    }

    fn starting_recording(&self) -> bool {
        self.starting_recording
    }

    fn stopping_recording(&self) -> bool {
        self.stopping_recording
    }

    fn recording_elapsed(&self) -> QString {
        qstring(&self.recording_elapsed)
    }

    fn system_audio(&self) -> bool {
        self.system_audio
    }

    fn microphone(&self) -> bool {
        self.microphone
    }

    fn microphone_device(&self) -> QString {
        qstring(&self.microphone_device)
    }

    fn audio_devices(&self) -> QList<QVariant> {
        self.audio_devices.clone()
    }

    fn microphone_level(&self) -> f64 {
        self.microphone_level.level()
    }

    fn webcam(&self) -> bool {
        self.webcam
    }

    fn webcam_device(&self) -> QString {
        qstring(&self.webcam_device)
    }

    fn webcam_height(&self) -> i32 {
        self.webcam_height
    }

    fn webcam_rotation(&self) -> i32 {
        self.webcam_rotation
    }

    fn webcam_flip_horizontal(&self) -> bool {
        self.webcam_flip_horizontal
    }

    fn self_view_enabled(&self) -> bool {
        self.self_view_enabled
    }

    fn self_view_size(&self) -> QString {
        qstring(&self.self_view_size)
    }

    fn countdown_before_recording(&self) -> bool {
        self.countdown_before_recording
    }

    /// The listed camera the chosen path resolves to now (`cameraDeviceForPath`), by index.
    fn webcam_camera_index(&self) -> Option<usize> {
        let found = camera::camera_device_for_path(&self.cameras, &self.webcam_device)?;
        self.cameras.iter().position(|device| std::ptr::eq(device, found))
    }

    fn webcam_camera_device(&self) -> QVariant {
        match self.webcam_camera_index().and_then(|index| i32::try_from(index).ok()) {
            Some(index) => self.camera_device_variant(index),
            None => QVariant::default(),
        }
    }

    fn webcam_preview_available(&self) -> bool {
        if screenshot() {
            return false;
        }
        self.webcam_camera_index().is_some()
    }

    fn webcam_devices(&self) -> QList<QVariant> {
        self.webcam_devices.clone()
    }

    // ---------------------------------------------------------------------------------------
    // Setters.

    fn save_recording_preferences(mut self: Pin<&mut Self>) {
        let mut preferences = RecordingPreferences::load();
        preferences.system_audio = self.system_audio;
        preferences.microphone = self.microphone;
        preferences.microphone_device = self.chosen_microphone_device.clone();
        preferences.webcam_enabled = self.webcam;
        preferences.webcam_device = self.webcam_device.clone();
        preferences.webcam_chosen_device = self.chosen_webcam_device.clone();
        preferences.webcam_height = self.webcam_height;
        preferences.webcam_rotation = self.webcam_rotation;
        preferences.webcam_flip_horizontal = self.webcam_flip_horizontal;
        preferences.self_view_enabled = self.self_view_enabled;
        preferences.self_view_size = self.self_view_size.clone();
        preferences.countdown_before_recording = self.countdown_before_recording;
        if let Err(error) = preferences.save() {
            self.as_mut().error_occurred(qstring(&error));
        }
    }

    fn set_system_audio(mut self: Pin<&mut Self>, value: bool) {
        if self.system_audio == value {
            return;
        }
        self.as_mut().rust_mut().system_audio = value;
        self.as_mut().save_recording_preferences();
        self.recording_preferences_changed();
    }

    fn set_microphone(mut self: Pin<&mut Self>, value: bool) {
        if self.microphone == value {
            return;
        }
        self.as_mut().rust_mut().microphone = value;
        self.as_mut().save_recording_preferences();
        self.as_mut().update_microphone_meter();
        self.recording_preferences_changed();
    }

    fn set_microphone_device(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value.is_empty() || (self.microphone_device == value && self.chosen_microphone_device == value) {
            return;
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.microphone_device = value.clone();
            rust.chosen_microphone_device = value;
        }
        self.as_mut().save_recording_preferences();
        self.as_mut().update_microphone_meter();
        self.recording_preferences_changed();
    }

    fn set_webcam(mut self: Pin<&mut Self>, value: bool) {
        if self.webcam == value {
            return;
        }
        self.as_mut().rust_mut().webcam = value;
        self.as_mut().save_recording_preferences();
        if value {
            self.as_mut().start_self_view_host();
        } else {
            state::send_host_command("quit");
        }
        self.recording_preferences_changed();
    }

    fn set_webcam_device(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value.is_empty() || self.webcam_device == value {
            return;
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.webcam_device = value.clone();
            rust.chosen_webcam_device = value;
        }
        self.as_mut().save_recording_preferences();
        if self.webcam {
            state::send_host_command("configure");
        }
        self.recording_preferences_changed();
    }

    fn set_webcam_height(mut self: Pin<&mut Self>, value: i32) {
        let value = launcher::normalized_webcam_height(value);
        if self.webcam_height == value {
            return;
        }
        self.as_mut().rust_mut().webcam_height = value;
        self.as_mut().save_recording_preferences();
        if self.webcam {
            state::send_host_command("configure");
        }
        self.recording_preferences_changed();
    }

    fn set_webcam_rotation(mut self: Pin<&mut Self>, value: i32) {
        let value = launcher::normalized_webcam_rotation(value);
        if self.webcam_rotation == value {
            return;
        }
        self.as_mut().rust_mut().webcam_rotation = value;
        state::send_host_command("rotate");
        self.recording_preferences_changed();
    }

    fn set_webcam_flip_horizontal(mut self: Pin<&mut Self>, value: bool) {
        if self.webcam_flip_horizontal == value {
            return;
        }
        self.as_mut().rust_mut().webcam_flip_horizontal = value;
        state::send_host_command("flip");
        self.recording_preferences_changed();
    }

    fn set_self_view_enabled(mut self: Pin<&mut Self>, value: bool) {
        if self.self_view_enabled == value {
            return;
        }
        self.as_mut().rust_mut().self_view_enabled = value;
        self.as_mut().save_recording_preferences();
        state::send_host_command(if value { "show" } else { "hide" });
        self.recording_preferences_changed();
    }

    fn set_self_view_size(mut self: Pin<&mut Self>, value: &QString) {
        let Some(normalized) = launcher::normalized_self_view_size(&value.to_string()) else { return };
        if self.self_view_size == normalized {
            return;
        }
        self.as_mut().rust_mut().self_view_size = normalized.clone();
        self.as_mut().save_recording_preferences();
        state::send_host_command(&normalized);
        self.recording_preferences_changed();
    }

    fn set_countdown_before_recording(mut self: Pin<&mut Self>, value: bool) {
        if self.countdown_before_recording == value {
            return;
        }
        self.as_mut().rust_mut().countdown_before_recording = value;
        self.as_mut().save_recording_preferences();
        self.recording_preferences_changed();
    }

    // ---------------------------------------------------------------------------------------
    // Devices.

    /// `refreshAudioDevices`: asks gpu-screen-recorder (up to five seconds), keeps the chosen
    /// microphone while it is listed, and reopens the meter.
    fn refresh_audio_devices(mut self: Pin<&mut Self>) {
        let devices = launcher::audio_devices();
        let selection = launcher::select_microphone(&devices, &self.chosen_microphone_device, &self.microphone_device);
        self.as_mut().rust_mut().audio_devices = audio_device_list(&devices);
        self.as_mut().audio_devices_changed();
        self.as_mut().rust_mut().microphone_device = selection.microphone;
        // Also reopens the meter's device when the default microphone changed or the last one
        // vanished.
        self.as_mut().update_microphone_meter();
        if selection.changed {
            self.recording_preferences_changed();
        }
    }

    /// `updateMicrophoneMeter`: the meter listens while the microphone is on and no recording is
    /// starting or running, except for a screenshot.
    fn update_microphone_meter(mut self: Pin<&mut Self>) {
        let listening = self.microphone && !self.recording && !self.starting_recording && !screenshot();
        let device = if listening { qstring(&self.microphone_device) } else { QString::default() };
        if self.as_mut().listen_to_microphone(&device) && self.as_mut().rust_mut().microphone_level.reset() {
            self.microphone_level_changed();
        }
    }

    /// `refreshWebcamDevices`: the cameras Qt lists that can record, and the one selected. A
    /// camera plugged in or pulled out while the launcher is open moves the self-view and
    /// `omareel record` with it, since they read the saved camera.
    fn refresh_webcam_devices(mut self: Pin<&mut Self>) {
        let mut list = CameraList::default();
        self.as_mut().refresh_camera_devices(&mut list);
        let cameras = list.0;
        let selection = launcher::webcam_devices(
            &cameras,
            &self.webcam_device,
            &self.chosen_webcam_device,
            camera::stable_camera_path,
            qdir::file_exists,
        );
        {
            let mut rust = self.as_mut().rust_mut();
            rust.cameras = cameras;
            rust.webcam_devices = webcam_device_list(&selection.devices);
        }
        self.as_mut().webcam_devices_changed();
        if !selection.changed {
            return;
        }
        self.as_mut().rust_mut().webcam_device = selection.device;
        self.as_mut().save_recording_preferences();
        if self.webcam && !self.recording {
            state::send_host_command("configure");
        }
        self.recording_preferences_changed();
    }

    fn handle_video_inputs_changed(self: Pin<&mut Self>) {
        self.refresh_webcam_devices();
    }

    fn handle_audio_inputs_changed(self: Pin<&mut Self>) {
        self.refresh_audio_devices();
    }

    fn microphone_read(mut self: Pin<&mut Self>, sample_format: i32, samples: &QByteArray) {
        let format = match sample_format {
            1 => SampleFormat::UInt8,
            2 => SampleFormat::Int16,
            3 => SampleFormat::Int32,
            4 => SampleFormat::Float,
            _ => SampleFormat::Unknown,
        };
        if self.as_mut().rust_mut().microphone_level.read(format, samples.as_slice()) {
            self.microphone_level_changed();
        }
    }

    // ---------------------------------------------------------------------------------------
    // The self-view host and the recording.

    /// `startSelfViewHost`: the standby host already running, or a new one.
    fn start_self_view_host(mut self: Pin<&mut Self>) {
        let host = state::read_state_file(&state::self_view_host_path());
        let existing = host.value("pid").to_variant_long_long();
        if existing > 0 && kill(existing, 0) == 0 {
            self.as_mut().rust_mut().host_pid = existing;
            return;
        }
        let mut pid = self.host_pid;
        self.as_mut().start_detached_self_view_host(&mut pid);
        self.as_mut().rust_mut().host_pid = pid;
    }

    fn recording_timer_fired(self: Pin<&mut Self>) {
        self.refresh_recording();
    }

    /// `refreshRecording`: the self-view status and the recording's state, every 50 ms.
    fn refresh_recording(mut self: Pin<&mut Self>) {
        if self.webcam {
            let host = state::read_state_file(&state::self_view_host_path());
            let host_pid = self.host_pid;
            let status = launcher::self_view_status(host_pid, || kill(host_pid, 0) == 0, &host);
            if status != self.self_view_status {
                self.as_mut().rust_mut().self_view_status = status;
                self.as_mut().self_view_status_changed();
            }
        }
        let active = state::is_recording();
        let state = if active { state::recording_state() } else { Default::default() };
        let capture_started = state.value("capture_started").to_bool(false);
        let stopping = state.value("stopping").to_bool(false);
        if capture_started != self.recording {
            self.as_mut().rust_mut().recording = capture_started;
            self.as_mut().update_microphone_meter();
            self.as_mut().recording_changed();
        }
        if stopping != self.stopping_recording {
            self.as_mut().rust_mut().stopping_recording = stopping;
            self.as_mut().stopping_recording_changed();
        }
        // The time stays where Stop left it while the recording is saved.
        let mut elapsed = if stopping { self.recording_elapsed.clone() } else { "00:00".to_owned() };
        if capture_started && !stopping {
            let started_us = state::recording_started_us();
            elapsed = launcher::format_duration(recorder::monotonic_us().wrapping_sub(started_us).max(0) as f64 / 1_000_000.0);
        }
        if elapsed != self.recording_elapsed {
            self.as_mut().rust_mut().recording_elapsed = elapsed;
            self.as_mut().recording_elapsed_changed();
        }
        if capture_started && self.starting_recording {
            self.as_mut().rust_mut().starting_recording = false;
            self.as_mut().starting_recording_changed();
        }
        if capture_started && self.quit_when_started {
            self.as_mut().rust_mut().quit_when_started = false;
            self.as_mut().quit_requested();
        }
    }

    fn open_bundle(mut self: Pin<&mut Self>, value: &QString) {
        let path = qobject::bundle_path(value);
        if !FileInfo::new(path.to_string()).is_dir() {
            self.as_mut().error_occurred(qstring("Choose an .omareel or legacy .omarecord bundle directory"));
            return;
        }
        let mut arguments = QStringList::default();
        arguments.append(qstring("edit"));
        arguments.append(path);
        if !qobject::start_detached(&qobject::application_file_path(), &arguments) {
            self.as_mut().error_occurred(qstring("Could not launch editor"));
            return;
        }
        self.quit_requested();
    }

    fn show_recordings_folder(self: Pin<&mut Self>) {
        let path = omarchy_paths::recordings_directory();
        qdir::mkpath(&path);
        if !qobject::open_folder(&qstring(&path)) {
            self.error_occurred(qstring("Could not open the recordings folder"));
        }
    }

    /// `record`: shows the start, then waits for the self-view host's camera (up to three
    /// seconds) before running `omareel record`.
    fn record(mut self: Pin<&mut Self>) {
        if self.recording || self.starting_recording {
            return;
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.starting_recording = true;
            rust.quit_when_started = true;
            rust.start_attempts = 0;
        }
        self.as_mut().update_microphone_meter();
        self.as_mut().starting_recording_changed();
        self.as_mut().recording_starting();
        self.start_start_timer();
    }

    fn start_timer_fired(mut self: Pin<&mut Self>) {
        if self.webcam {
            let attempt = self.start_attempts;
            self.as_mut().rust_mut().start_attempts = attempt.saturating_add(1);
            if attempt < 60 {
                let host = state::read_state_file(&state::self_view_host_path());
                if host.value("pid").to_variant_long_long() != self.host_pid
                    || host.value("camera_status").to_string_or("") == "starting"
                {
                    return;
                }
            }
        }
        self.as_mut().stop_start_timer();
        let arguments = launcher::record_arguments(
            self.system_audio,
            self.microphone,
            &self.chosen_microphone_device,
            self.webcam,
            &self.webcam_device,
            self.webcam_height,
        );
        self.start_record_job(arguments);
    }

    /// Runs `omareel record` on a thread; its end comes back to [`record_finished`].
    fn start_record_job(mut self: Pin<&mut Self>, arguments: Vec<String>) {
        let program = qobject::application_file_path().to_string();
        let thread = self.qt_thread();
        let cancel = Arc::new(AtomicBool::new(false));
        let work = {
            let cancel = Arc::clone(&cancel);
            move || {
                if let Some(outcome) = run_record(&program, &arguments, &cancel) {
                    let _ = thread.queue(move |launcher| launcher.record_finished(outcome));
                }
            }
        };
        let mut rust = self.as_mut().rust_mut();
        rust.record_jobs.retain(|job| job.worker.as_ref().is_some_and(|worker| !worker.is_finished()));
        // A thread that cannot be started is a run that failed to start, which QProcess did not
        // report through `finished` either.
        let worker = std::thread::Builder::new().spawn(work).ok();
        rust.record_jobs.push(RecordJob { cancel, worker });
    }

    /// The QProcess's `finished`: an error unless `omareel record` exited 0.
    fn record_finished(mut self: Pin<&mut Self>, outcome: RecordOutcome) {
        self.as_mut().refresh_recording();
        if !outcome.crashed && outcome.exit_code == 0 {
            return;
        }
        self.as_mut().rust_mut().quit_when_started = false;
        if self.starting_recording {
            self.as_mut().rust_mut().starting_recording = false;
            self.as_mut().update_microphone_meter();
            self.as_mut().starting_recording_changed();
        }
        let message = if outcome.output.is_empty() { "Could not start recording".to_owned() } else { outcome.output };
        self.error_occurred(qstring(&message));
    }

    fn stop_recording(self: Pin<&mut Self>) {
        if let Err(error) = state::signal_existing(false) {
            self.error_occurred(qstring(&error));
        }
    }

    fn cancel_recording(self: Pin<&mut Self>) {
        if let Err(error) = state::signal_existing(true) {
            self.error_occurred(qstring(&error));
        }
    }
}
