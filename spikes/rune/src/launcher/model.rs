//! The launcher's state and behaviour: `crates/omareel_qt/src/launcher.rs` (the `launcher` object
//! Launcher.qml binds to) without Qt. Same properties, setters, preference saving, device
//! selection, self-view host commands, `omareel record` run and recording poll, in the same
//! order; where the bridge emitted a signal the model sets `dirty` (the view re-runs) or queues
//! an error (`errorOccurred`) or sets `quit_requested` (`quitRequested`).
//!
//! What `LauncherBase` did with Qt is here too: the camera list is [`super::v4l2`], the
//! microphone meter and the device watch are [`super::audio`], the timers are the window's poll
//! (every 50 ms, as both QTimers were).
//!
//! `live` is false in headless runs (snapshots, scripts), Qt's `OMAREEL_SCREENSHOT`: devices are
//! listed, but nothing opens the microphone or the camera, nothing talks to Hyprland or the
//! recorder, and the preferences are never written.

use super::audio::{Meter, Watch};
use omareel_core::microphone_meter::{MicrophoneLevel, SampleFormat};
use omareel_core::recording_preferences::RecordingPreferences;
use omareel_record::camera::{self, CameraDevice};
use omareel_record::launcher::{self, AudioDevice, WebcamDevice};
use omareel_record::recorder::{self, state};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// How `omareel record` ended: its exit, whether it crashed, and what it printed (both
/// channels, trimmed), as the QProcess's `finished` and `readAll`.
pub struct RecordOutcome {
    exit_code: i32,
    crashed: bool,
    output: String,
}

/// A run of `omareel record`: its pid while it runs, killed if the launcher goes first (the
/// QProcess was the launcher's child).
struct RecordJob {
    pid: Arc<Mutex<Option<u32>>>,
}

pub struct Model {
    pub live: bool,
    pub capture_exclusion_note: String,
    pub capture_exclusion_reason: String,
    pub self_view_status: String,
    host_pid: i64,
    pub recording: bool,
    pub recording_elapsed: String,
    pub system_audio: bool,
    pub microphone: bool,
    /// The microphone recorded from: the one last picked while it is plugged in, else the
    /// default.
    pub microphone_device: String,
    chosen_microphone_device: String,
    pub audio_devices: Vec<AudioDevice>,
    microphone_level: MicrophoneLevel,
    pub webcam: bool,
    pub webcam_device: String,
    pub webcam_height: i32,
    pub webcam_rotation: i32,
    pub webcam_flip_horizontal: bool,
    pub self_view_enabled: bool,
    pub self_view_size: String,
    pub webcam_devices: Vec<WebcamDevice>,
    /// The cameras of the last refresh.
    cameras: Vec<CameraDevice>,
    /// The camera last picked, selected again whenever it is plugged in.
    chosen_webcam_device: String,
    pub starting_recording: bool,
    pub stopping_recording: bool,
    pub countdown_before_recording: bool,
    quit_when_started: bool,
    /// How often `record`'s start timer has waited for the self-view host; None while the timer
    /// is stopped.
    start_attempts: Option<i32>,
    record_jobs: Vec<RecordJob>,
    record_results: (Sender<RecordOutcome>, Receiver<RecordOutcome>),
    meter: Option<Meter>,
    watch: Option<Watch>,
    /// Device listings running on threads (gpu-screen-recorder takes up to five seconds), and
    /// whether another was asked for meanwhile.
    audio_listing: Option<Receiver<Vec<AudioDevice>>>,
    audio_again: bool,
    camera_listing: Option<Receiver<Vec<CameraDevice>>>,
    camera_again: bool,
    camera_nodes: Vec<String>,
    last_camera_check: f64,
    exclusion: Option<Receiver<(String, String)>>,
    last_refresh: f64,
    last_start_tick: f64,
    /// `errorOccurred` messages not shown yet.
    pub errors: Vec<String>,
    /// `quitRequested`.
    pub quit_requested: bool,
    /// Something the view shows changed.
    pub dirty: bool,
}

/// The program this is (`QCoreApplication::applicationFilePath()`).
fn program() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "/proc/self/exe".into())
}

fn alive(pid: i64) -> bool {
    pid > 0 && pid <= i64::from(i32::MAX) && unsafe { libc::kill(pid as i32, 0) } == 0
}

/// Starts `program arguments` detached (its own session, no stdio), as
/// `QProcess::startDetached`; its pid, or None when it could not start.
fn start_detached(program: &str, arguments: &[&str], remove_env: &[&str]) -> Option<u32> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(program);
    command.args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
    for name in remove_env {
        command.env_remove(name);
    }
    let mut child = command.spawn().ok()?;
    let pid = child.id();
    // Reap it when it ends, so it never lingers as a zombie of the launcher.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Some(pid)
}

/// Runs `omareel record` with both channels into one pipe; None when it could not start.
fn run_record(program: &str, arguments: &[String], pid: &Mutex<Option<u32>>) -> Option<RecordOutcome> {
    let (mut reader, writer) = std::io::pipe().ok()?;
    let mut command = Command::new(program);
    command.args(arguments).stdin(Stdio::null()).stdout(writer.try_clone().ok()?).stderr(writer);
    let mut child = command.spawn().ok()?;
    drop(command);
    *pid.lock().unwrap() = Some(child.id());
    let mut output = Vec::new();
    let _ = std::io::Read::read_to_end(&mut reader, &mut output);
    let status = child.wait().ok()?;
    *pid.lock().unwrap() = None;
    use std::os::unix::process::ExitStatusExt;
    Some(RecordOutcome {
        exit_code: status.code().unwrap_or(-1),
        crashed: status.signal().is_some(),
        output: String::from_utf8_lossy(&output).trim().to_owned(),
    })
}

impl Model {
    /// `Launcher::initialize`.
    pub fn new(live: bool) -> Model {
        let mut m = Model {
            live,
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
            audio_devices: Vec::new(),
            microphone_level: MicrophoneLevel::default(),
            webcam: false,
            webcam_device: "/dev/video2".into(),
            webcam_height: 1080,
            webcam_rotation: 0,
            webcam_flip_horizontal: false,
            self_view_enabled: true,
            self_view_size: "M".into(),
            webcam_devices: Vec::new(),
            cameras: Vec::new(),
            chosen_webcam_device: String::new(),
            starting_recording: false,
            stopping_recording: false,
            countdown_before_recording: false,
            quit_when_started: false,
            start_attempts: None,
            record_jobs: Vec::new(),
            record_results: channel(),
            meter: None,
            watch: None,
            audio_listing: None,
            audio_again: false,
            camera_listing: None,
            camera_again: false,
            camera_nodes: Vec::new(),
            last_camera_check: 0.0,
            exclusion: None,
            last_refresh: f64::NEG_INFINITY,
            last_start_tick: 0.0,
            errors: Vec::new(),
            quit_requested: false,
            dirty: true,
        };
        if live {
            // A recorder that was killed mid-recording left hardware cursors held on; then the
            // capture exclusion plugin (hyprctl, up to 2 s a call), off the UI thread.
            let (send, receive) = channel();
            std::thread::spawn(move || {
                omareel_record::capture_exclusion::restore_abandoned_hardware_cursor();
                let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned())).unwrap_or_default();
                let exclusion = omareel_record::capture_exclusion::ensure_loaded(&dir);
                if !exclusion.loaded {
                    let screens = omareel_record::region_picker::monitors().map(|m| m.len()).unwrap_or(1);
                    eprintln!("omareel: {}; recording overlays stay off the recorded screen", exclusion.reason);
                    let _ = send.send((launcher::capture_exclusion_note(screens).to_owned(), exclusion.reason));
                }
            });
            m.exclusion = Some(receive);
        }
        let preferences = RecordingPreferences::load();
        m.system_audio = preferences.system_audio;
        m.microphone = preferences.microphone;
        m.microphone_device = preferences.microphone_device.clone();
        m.chosen_microphone_device = preferences.microphone_device.clone();
        m.webcam = preferences.webcam_enabled;
        // Older settings saved the /dev/videoN node, which the camera keeps only until it is
        // replugged.
        m.webcam_device = camera::stable_camera_path(&preferences.webcam_device);
        m.chosen_webcam_device = camera::stable_camera_path(&preferences.webcam_chosen_device);
        m.webcam_height = preferences.webcam_height;
        m.webcam_rotation = preferences.webcam_rotation;
        m.webcam_flip_horizontal = preferences.webcam_flip_horizontal;
        m.self_view_enabled = preferences.self_view_enabled;
        m.self_view_size = preferences.self_view_size.clone();
        m.countdown_before_recording = preferences.countdown_before_recording;
        m.camera_nodes = super::v4l2::nodes();
        if live {
            m.refresh_audio_devices();
            m.refresh_webcam_devices();
            if m.webcam_device != preferences.webcam_device || m.chosen_webcam_device != preferences.webcam_chosen_device {
                m.save_recording_preferences();
            }
            // watchMediaDevices.
            m.watch = Some(Watch::start());
            m.refresh_recording();
            m.update_microphone_meter();
            if m.webcam && !state::is_recording() {
                m.start_self_view_host();
            }
        } else {
            // A snapshot lists the devices as they are, at once, so it is deterministic.
            m.apply_audio_devices(launcher::audio_devices());
            m.apply_webcam_devices(super::v4l2::devices());
        }
        m
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
        self.dirty = true;
    }

    // -------------------------------------------------------------------------------------------
    // Properties the view derives.

    pub fn microphone_level(&self) -> f64 {
        self.microphone_level.level()
    }

    // -------------------------------------------------------------------------------------------
    // Setters.

    fn save_recording_preferences(&mut self) {
        if !self.live {
            return;
        }
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
            self.error(error);
        }
    }

    fn host_command(&self, command: &str) {
        if self.live {
            state::send_host_command(command);
        }
    }

    pub fn set_system_audio(&mut self, value: bool) {
        if self.system_audio == value {
            return;
        }
        self.system_audio = value;
        self.save_recording_preferences();
        self.dirty = true;
    }

    pub fn set_microphone(&mut self, value: bool) {
        if self.microphone == value {
            return;
        }
        self.microphone = value;
        self.save_recording_preferences();
        self.update_microphone_meter();
        self.dirty = true;
    }

    pub fn set_microphone_device(&mut self, value: &str) {
        if value.is_empty() || (self.microphone_device == value && self.chosen_microphone_device == value) {
            return;
        }
        self.microphone_device = value.to_owned();
        self.chosen_microphone_device = value.to_owned();
        self.save_recording_preferences();
        self.update_microphone_meter();
        self.dirty = true;
    }

    pub fn set_webcam(&mut self, value: bool) {
        if self.webcam == value {
            return;
        }
        self.webcam = value;
        self.save_recording_preferences();
        if value {
            self.start_self_view_host();
        } else {
            self.host_command("quit");
        }
        self.dirty = true;
    }

    pub fn set_webcam_device(&mut self, value: &str) {
        if value.is_empty() || self.webcam_device == value {
            return;
        }
        self.webcam_device = value.to_owned();
        self.chosen_webcam_device = value.to_owned();
        self.save_recording_preferences();
        if self.webcam {
            self.host_command("configure");
        }
        self.dirty = true;
    }

    /// No control sets it (as in Launcher.qml); kept so the preference round-trips.
    #[allow(dead_code)]
    pub fn set_webcam_height(&mut self, value: i32) {
        let value = launcher::normalized_webcam_height(value);
        if self.webcam_height == value {
            return;
        }
        self.webcam_height = value;
        self.save_recording_preferences();
        if self.webcam {
            self.host_command("configure");
        }
        self.dirty = true;
    }

    pub fn set_webcam_rotation(&mut self, value: i32) {
        let value = launcher::normalized_webcam_rotation(value);
        if self.webcam_rotation == value {
            return;
        }
        self.webcam_rotation = value;
        self.host_command("rotate");
        self.dirty = true;
    }

    pub fn set_webcam_flip_horizontal(&mut self, value: bool) {
        if self.webcam_flip_horizontal == value {
            return;
        }
        self.webcam_flip_horizontal = value;
        self.host_command("flip");
        self.dirty = true;
    }

    pub fn set_self_view_enabled(&mut self, value: bool) {
        if self.self_view_enabled == value {
            return;
        }
        self.self_view_enabled = value;
        self.save_recording_preferences();
        self.host_command(if value { "show" } else { "hide" });
        self.dirty = true;
    }

    pub fn set_self_view_size(&mut self, value: &str) {
        let Some(normalized) = launcher::normalized_self_view_size(value) else { return };
        if self.self_view_size == normalized {
            return;
        }
        self.self_view_size = normalized.clone();
        self.save_recording_preferences();
        self.host_command(&normalized);
        self.dirty = true;
    }

    pub fn set_countdown_before_recording(&mut self, value: bool) {
        if self.countdown_before_recording == value {
            return;
        }
        self.countdown_before_recording = value;
        self.save_recording_preferences();
        self.dirty = true;
    }

    // -------------------------------------------------------------------------------------------
    // Devices.

    /// `refreshAudioDevices`: asks gpu-screen-recorder on a thread; the answer comes back to
    /// [`Self::apply_audio_devices`].
    fn refresh_audio_devices(&mut self) {
        if self.audio_listing.is_some() {
            self.audio_again = true;
            return;
        }
        let (send, receive) = channel();
        std::thread::spawn(move || {
            let _ = send.send(launcher::audio_devices());
        });
        self.audio_listing = Some(receive);
    }

    /// The rest of `refreshAudioDevices`: keeps the chosen microphone while it is listed, and
    /// reopens the meter.
    fn apply_audio_devices(&mut self, devices: Vec<AudioDevice>) {
        let selection = launcher::select_microphone(&devices, &self.chosen_microphone_device, &self.microphone_device);
        self.audio_devices = devices;
        self.microphone_device = selection.microphone;
        // Also reopens the meter's device when the default microphone changed or the last one
        // vanished.
        self.update_microphone_meter();
        self.dirty = true;
    }

    /// `updateMicrophoneMeter`: the meter listens while the microphone is on and no recording
    /// is starting or running, except in a snapshot.
    fn update_microphone_meter(&mut self) {
        let listening = self.microphone && !self.recording && !self.starting_recording && self.live;
        let device = if listening { self.microphone_device.clone() } else { String::new() };
        // `listenToMicrophone`: the same device still running is left alone.
        if let Some(meter) = &self.meter
            && !device.is_empty()
            && meter.device == device
            && meter.running()
        {
            return;
        }
        self.meter = (!device.is_empty()).then(|| Meter::start(&device));
        if self.microphone_level.reset() {
            self.dirty = true;
        }
    }

    /// `refreshWebcamDevices`, listing on a thread (opening a camera node can be slow).
    fn refresh_webcam_devices(&mut self) {
        if self.camera_listing.is_some() {
            self.camera_again = true;
            return;
        }
        let (send, receive) = channel();
        std::thread::spawn(move || {
            let _ = send.send(super::v4l2::devices());
        });
        self.camera_listing = Some(receive);
    }

    /// The rest of `refreshWebcamDevices`: the cameras that can record, and the one selected. A
    /// camera plugged in or pulled out while the launcher is open moves the self-view and
    /// `omareel record` with it, since they read the saved camera.
    fn apply_webcam_devices(&mut self, cameras: Vec<CameraDevice>) {
        let selection = launcher::webcam_devices(
            &cameras,
            &self.webcam_device,
            &self.chosen_webcam_device,
            camera::stable_camera_path,
            omareel_core::compat::qdir::file_exists,
        );
        self.cameras = cameras;
        self.webcam_devices = selection.devices;
        self.dirty = true;
        if !selection.changed {
            return;
        }
        self.webcam_device = selection.device;
        self.save_recording_preferences();
        if self.webcam && !self.recording {
            self.host_command("configure");
        }
    }

    /// Whether the chosen camera is plugged in now (`webcamPreviewAvailable`).
    #[allow(dead_code)]
    pub fn webcam_preview_available(&self) -> bool {
        self.live && camera::camera_device_for_path(&self.cameras, &self.webcam_device).is_some()
    }

    // -------------------------------------------------------------------------------------------
    // The self-view host and the recording.

    /// `startSelfViewHost`: the standby host already running, or a new one
    /// (`<program> __record-bar --standby --owner <pid>`, without the screenshot variables).
    fn start_self_view_host(&mut self) {
        if !self.live {
            return;
        }
        let host = state::read_state_file(&state::self_view_host_path());
        let existing = host.value("pid").to_variant_long_long();
        if alive(existing) {
            self.host_pid = existing;
            return;
        }
        let owner = std::process::id().to_string();
        self.host_pid = start_detached(&program(), &["__record-bar", "--standby", "--owner", &owner], &["OMAREEL_SCREENSHOT", "OMAREEL_SCREENSHOT_LIVE"])
            .map_or(-1, i64::from);
    }

    /// `refreshRecording`: the self-view status and the recording's state, every 50 ms.
    fn refresh_recording(&mut self) {
        if self.webcam {
            let host = state::read_state_file(&state::self_view_host_path());
            let host_pid = self.host_pid;
            let status = launcher::self_view_status(host_pid, || alive(host_pid), &host);
            if status != self.self_view_status {
                self.self_view_status = status;
                self.dirty = true;
            }
        }
        let active = state::is_recording();
        let state = if active { state::recording_state() } else { Default::default() };
        let capture_started = state.value("capture_started").to_bool(false);
        let stopping = state.value("stopping").to_bool(false);
        if capture_started != self.recording {
            self.recording = capture_started;
            self.update_microphone_meter();
            self.dirty = true;
        }
        if stopping != self.stopping_recording {
            self.stopping_recording = stopping;
            self.dirty = true;
        }
        // The time stays where Stop left it while the recording is saved.
        let mut elapsed = if stopping { self.recording_elapsed.clone() } else { "00:00".to_owned() };
        if capture_started && !stopping {
            let started_us = state::recording_started_us();
            elapsed = launcher::format_duration(recorder::monotonic_us().wrapping_sub(started_us).max(0) as f64 / 1_000_000.0);
        }
        if elapsed != self.recording_elapsed {
            self.recording_elapsed = elapsed;
            self.dirty = true;
        }
        if capture_started && self.starting_recording {
            self.starting_recording = false;
            self.dirty = true;
        }
        if capture_started && self.quit_when_started {
            self.quit_when_started = false;
            self.quit_requested = true;
        }
    }

    /// `openBundle`: the editor on a bundle directory, then quit.
    pub fn open_bundle(&mut self, path: &str) {
        if !std::path::Path::new(path).is_dir() {
            self.error("Choose an .omareel or legacy .omarecord bundle directory");
            return;
        }
        if !self.live {
            // A headless run opens nothing; it only shows the launcher quitting.
            self.quit_requested = true;
            return;
        }
        if start_detached(&program(), &["edit", path], &[]).is_none() {
            self.error("Could not launch editor");
            return;
        }
        self.quit_requested = true;
    }

    /// `showRecordingsFolder`: the file manager on the recordings folder
    /// (`QDesktopServices::openUrl` is `xdg-open` on Linux).
    pub fn show_recordings_folder(&mut self) {
        let path = omareel_core::omarchy_paths::recordings_directory();
        omareel_core::compat::qdir::mkpath(&path);
        if !self.live {
            return;
        }
        if start_detached("xdg-open", &[&path], &[]).is_none() {
            self.error("Could not open the recordings folder");
        }
    }

    /// `record`: shows the start, then waits for the self-view host's camera (up to three
    /// seconds) before running `omareel record`.
    pub fn record(&mut self, now: f64) {
        if self.recording || self.starting_recording {
            return;
        }
        self.starting_recording = true;
        self.quit_when_started = true;
        self.start_attempts = Some(0);
        self.last_start_tick = now;
        self.update_microphone_meter();
        self.dirty = true;
    }

    /// `startTimerFired`.
    fn start_timer_fired(&mut self) {
        let Some(attempt) = self.start_attempts else { return };
        if self.webcam {
            self.start_attempts = Some(attempt.saturating_add(1));
            if attempt < 60 {
                let host = state::read_state_file(&state::self_view_host_path());
                if host.value("pid").to_variant_long_long() != self.host_pid || host.value("camera_status").to_string_or("") == "starting" {
                    return;
                }
            }
        }
        self.start_attempts = None;
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

    /// Runs `omareel record` on a thread; its end comes back to [`Self::record_finished`].
    fn start_record_job(&mut self, arguments: Vec<String>) {
        let pid = Arc::new(Mutex::new(None));
        let send = self.record_results.0.clone();
        let job_pid = pid.clone();
        let program = program();
        // A run that cannot start is reported as QProcess did not: no `finished`.
        std::thread::spawn(move || {
            if let Some(outcome) = run_record(&program, &arguments, &job_pid) {
                let _ = send.send(outcome);
            }
        });
        self.record_jobs.retain(|job| job.pid.lock().unwrap().is_some());
        self.record_jobs.push(RecordJob { pid });
    }

    /// The QProcess's `finished`: an error unless `omareel record` exited 0.
    fn record_finished(&mut self, outcome: RecordOutcome) {
        self.refresh_recording();
        if !outcome.crashed && outcome.exit_code == 0 {
            return;
        }
        self.quit_when_started = false;
        if self.starting_recording {
            self.starting_recording = false;
            self.update_microphone_meter();
            self.dirty = true;
        }
        let message = if outcome.output.is_empty() { "Could not start recording".to_owned() } else { outcome.output };
        self.error(message);
    }

    pub fn stop_recording(&mut self) {
        if let Err(error) = state::signal_existing(false) {
            self.error(error);
        }
    }

    pub fn cancel_recording(&mut self) {
        if let Err(error) = state::signal_existing(true) {
            self.error(error);
        }
    }

    // -------------------------------------------------------------------------------------------
    // The timers.

    /// What the QTimers and QMediaDevices did, from the window's poll (`now` in seconds).
    pub fn tick(&mut self, now: f64) {
        if let Some(receive) = &self.exclusion
            && let Ok((note, reason)) = receive.try_recv()
        {
            self.capture_exclusion_note = note;
            self.capture_exclusion_reason = reason;
            self.exclusion = None;
            self.dirty = true;
        }
        if let Some(receive) = &self.audio_listing
            && let Ok(devices) = receive.try_recv()
        {
            self.audio_listing = None;
            self.apply_audio_devices(devices);
            if std::mem::take(&mut self.audio_again) {
                self.refresh_audio_devices();
            }
        }
        if let Some(receive) = &self.camera_listing
            && let Ok(cameras) = receive.try_recv()
        {
            self.camera_listing = None;
            self.apply_webcam_devices(cameras);
            if std::mem::take(&mut self.camera_again) {
                self.refresh_webcam_devices();
            }
        }
        if !self.live {
            return;
        }
        // QMediaDevices' change signals.
        if self.watch.as_ref().is_some_and(Watch::take_changed) {
            self.refresh_audio_devices();
        }
        if now - self.last_camera_check >= 1.0 {
            self.last_camera_check = now;
            let nodes = super::v4l2::nodes();
            if nodes != self.camera_nodes {
                self.camera_nodes = nodes;
                self.refresh_webcam_devices();
            }
        }
        // The meter's 50 ms read, and a stopped input reopened.
        if let Some(meter) = &mut self.meter {
            let samples = meter.take();
            if !samples.is_empty() && self.microphone_level.read(SampleFormat::Int16, &samples) {
                self.dirty = true;
            }
            if !meter.running() {
                self.meter = None;
                self.update_microphone_meter();
            }
        }
        while let Ok(outcome) = self.record_results.1.try_recv() {
            self.record_finished(outcome);
        }
        if self.start_attempts.is_some() && now - self.last_start_tick >= 0.05 {
            self.last_start_tick = now;
            self.start_timer_fired();
        }
        if now - self.last_refresh >= 0.05 {
            self.last_refresh = now;
            self.refresh_recording();
        }
    }

    // -------------------------------------------------------------------------------------------
    // Snapshot scenes (Launcher.qml's `prepareScreenshot`).

    pub fn scene_webcam(&mut self) {
        self.webcam = true;
        self.self_view_status = "Self-view floating on DP-1 — drag it where you want it".into();
    }
}

impl Drop for Model {
    /// The launcher's QProcess was its child: a run of `omareel record` still going is killed.
    fn drop(&mut self) {
        for job in &self.record_jobs {
            if let Some(pid) = *job.pid.lock().unwrap() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
        }
    }
}
