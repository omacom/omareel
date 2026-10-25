//! `Recorder::startDetached`: how `omareel record` begins a recording. The region is picked
//! (`RegionPicker::pick`), the recorder daemon (`omareel __record-daemon ...`) is started
//! detached with its output thrown away, and the command waits for the daemon's state file to
//! say that capture has started, or for the daemon to give up and leave its reason in
//! `last-error.txt`.

use super::state::{last_error_file_path, process_alive, read_last_error, read_state_file, state_file_path};
use super::{debug_start_stage, mode_name, monotonic_us};
use crate::capture::RecordOptions;
use crate::region_picker;
use crate::screen_capture;
use omareel_core::compat::qcoreapplication::application_file_path;
use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qprocess::{self, DetachedChannels};
use omareel_core::compat::{qfile, qnumber};
use std::time::{Duration, Instant};

/// What `startDetached` returns: the exit status for `omareel record` (0 started, 1 the
/// selection was cancelled, 2 failed) and the message it prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartOutcome {
    pub code: i32,
    pub message: String,
}

/// The daemon's arguments for a picked region (`startDetached`'s list): the region with
/// `QString::number(value, 'g', 16)` for its coordinates, then the options.
pub fn daemon_arguments(region: &crate::capture::CaptureRegion, options: &RecordOptions) -> Vec<String> {
    let number = |value: f64| qnumber::number_f64(value, 'g', 16);
    let mut arguments: Vec<String> = vec![
        "__record-daemon".into(),
        "--mode".into(),
        mode_name(region.mode).into(),
        "--monitor".into(),
        region.monitor_name.clone(),
        "--x".into(),
        number(region.x),
        "--y".into(),
        number(region.y),
        "--w".into(),
        number(region.width),
        "--h".into(),
        number(region.height),
        "--scale".into(),
        number(region.scale),
        "--physical-w".into(),
        region.physical_width.to_string(),
        "--physical-h".into(),
        region.physical_height.to_string(),
        "--fps".into(),
        options.fps.to_string(),
        "--dir".into(),
        options.output_directory.clone(),
    ];
    let mut push = |items: &[&str]| arguments.extend(items.iter().map(|item| (*item).to_owned()));
    if options.desktop_audio {
        push(&["--desktop-audio"]);
    }
    if options.microphone_audio {
        push(&["--microphone-audio", "--microphone-device", &options.microphone_device]);
    }
    if options.webcam {
        let height = options.webcam_height.to_string();
        let rotation = options.webcam_rotation.to_string();
        push(&[
            "--webcam",
            "--webcam-device",
            &options.webcam_device,
            "--webcam-height",
            &height,
            "--webcam-rotation",
            &rotation,
        ]);
    }
    if options.webcam && options.webcam_flip_horizontal {
        push(&["--webcam-flip-horizontal"]);
    }
    if !options.self_view {
        push(&["--no-selfview"]);
    }
    push(&["--selfview-size", &options.self_view_size]);
    push(&["--capture-backend", &options.capture_backend]);
    if options.no_open {
        push(&["--no-open"]);
    }
    if options.no_bar {
        push(&["--no-bar"]);
    }
    if options.countdown {
        push(&["--countdown"]);
    }
    arguments
}

/// `Recorder::startDetached(options, &message)`.
pub fn start_detached(options: &RecordOptions) -> StartOutcome {
    let pick = region_picker::pick(options.mode, &screen_capture::is_supported);
    let Some(region) = pick.region else {
        let error = pick.message.unwrap_or_default();
        let code = if error == "Selection cancelled" { 1 } else { 2 };
        return StartOutcome { code, message: error };
    };
    let selection_note = pick.message.unwrap_or_default();
    qdir::mkpath(&FileInfo::new(state_file_path()).absolute_path());
    qfile::remove(&last_error_file_path());
    let arguments = daemon_arguments(&region, options);
    let channels =
        DetachedChannels { standard_output_file: Some("/dev/null"), standard_error_file: Some("/dev/null") };
    let Some(pid) = qprocess::start_detached(&application_file_path(), &arguments, channels) else {
        return StartOutcome { code: 2, message: "Could not start recorder daemon".into() };
    };
    let spawn_us = monotonic_us();
    debug_start_stage("daemon_spawn", spawn_us, &format!("pid={pid}"));
    let ready_timer = Instant::now();
    let mut state_appeared = false;
    let startup_timeout_ms: u128 = if options.countdown { 7000 } else { 3000 };
    while ready_timer.elapsed().as_millis() < startup_timeout_ms {
        let state = read_state_file(&state_file_path());
        if state.value("pid").to_variant_long_long() == pid {
            state_appeared = true;
            if state.value("capture_started").to_bool(false) {
                debug_start_stage("cli_ready", spawn_us, "");
                let message = if selection_note.is_empty() {
                    "Recording started".to_owned()
                } else {
                    format!("{selection_note}\nRecording started")
                };
                return StartOutcome { code: 0, message };
            }
        }
        // exec can briefly show an empty cmdline. This pid was just started here, so its
        // process state is checked rather than its argv.
        if !process_alive(pid) {
            let mut reason = read_last_error();
            if reason.is_empty() {
                reason = "Recorder daemon exited during startup".into();
            }
            qfile::remove(&last_error_file_path());
            return StartOutcome { code: 2, message: reason };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !state_appeared {
        return StartOutcome { code: 2, message: "Recorder daemon did not become ready".into() };
    }
    StartOutcome { code: 2, message: "Recorder daemon did not start capture in time".into() }
}
