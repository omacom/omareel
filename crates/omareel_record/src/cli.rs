//! The recording commands of `main.cpp`: `omareel record` (`recordCommand`), and the two small
//! commands the bar and the tests use to talk to a running recording through its state files,
//! `__selfview-ipc` and `__record-bar-ipc`. Each takes the arguments after the command and the
//! two channels it prints to, and returns the exit status.

use crate::camera::camera_node_path;
use crate::capture::{CaptureMode, RecordOptions};
use crate::recorder::start::start_detached;
use crate::recorder::state;
use omareel_core::compat::qdir;
use omareel_core::compat::qjson::Object;
use omareel_core::compat::qnumber;
use omareel_core::omarchy_paths;
use omareel_core::recording_preferences::RecordingPreferences;
use std::io::Write;

/// `usage(error)`: the error and the help on `err`, status 2.
fn usage(err: &mut dyn Write, error: &str) -> i32 {
    let _ = err.write_all(omareel_core::cli::usage_text(error).as_bytes());
    let _ = err.flush();
    omareel_core::cli::usage_status(error)
}

/// One line, as `QTextStream(stream) << text << '\n'` writes it.
fn line(stream: &mut dyn Write, text: &str) {
    let _ = stream.write_all(format!("{text}\n").as_bytes());
    let _ = stream.flush();
}

/// `QString::toInt()`: 0 when the text is not a number.
fn to_int(text: &str) -> i32 {
    qnumber::to_int(text).unwrap_or(0)
}

/// `omareel record [options]`: stops (or with `--cancel` discards) the recording that is
/// running, or starts one with the saved preferences and the options given.
pub fn record_command(arguments: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let has = |name: &str| arguments.iter().any(|argument| argument == name);
    if state::is_recording() {
        if has("--stop") && has("--cancel") {
            return usage(err, "choose either --stop or --cancel");
        }
        let cancel = has("--cancel");
        let outcome = state::stop_existing(cancel);
        if outcome.code == 0 {
            let bundle = outcome.bundle_path.unwrap_or_default();
            line(out, if cancel { "Recording discarded" } else { &bundle });
        } else {
            line(err, &format!("omareel: {}", outcome.error));
        }
        return outcome.code;
    }
    if has("--stop") || has("--cancel") {
        line(err, "omareel: no recording is active");
        return 1;
    }
    if has("--with-webcam") && has("--no-webcam") {
        return usage(err, "--with-webcam cannot be combined with --no-webcam");
    }
    let mut options = RecordOptions { output_directory: omarchy_paths::recordings_directory(), ..RecordOptions::default() };
    let preferences = RecordingPreferences::load();
    let explicit_audio = has("--with-desktop-audio") || has("--with-microphone-audio") || has("--no-audio");
    if !explicit_audio {
        options.desktop_audio = preferences.system_audio;
        options.microphone_audio = preferences.microphone;
    }
    options.microphone_device = preferences.microphone_device.clone();
    let explicit_webcam = has("--with-webcam") || has("--no-webcam");
    if !explicit_webcam {
        options.webcam = preferences.webcam_enabled;
    }
    // The launcher saves the camera it fell back to while the chosen one was unplugged.
    options.webcam_device = if camera_node_path(&preferences.webcam_chosen_device).is_empty() {
        preferences.webcam_device.clone()
    } else {
        preferences.webcam_chosen_device.clone()
    };
    options.webcam_height = preferences.webcam_height;
    options.webcam_rotation = preferences.webcam_rotation;
    options.webcam_flip_horizontal = preferences.webcam_flip_horizontal;
    options.self_view = preferences.self_view_enabled;
    options.self_view_size = preferences.self_view_size.clone();
    options.capture_backend = preferences.capture_backend.clone();
    options.countdown = preferences.countdown_before_recording;
    let mut mode_count = 0;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        let mut value = || {
            index += 1;
            arguments.get(index).cloned()
        };
        match argument {
            "--fullscreen" | "--region" | "--window" => {
                options.mode = match argument {
                    "--fullscreen" => CaptureMode::Fullscreen,
                    "--region" => CaptureMode::Region,
                    _ => CaptureMode::Window,
                };
                mode_count += 1;
            }
            "--with-desktop-audio" => options.desktop_audio = true,
            "--with-microphone-audio" => options.microphone_audio = true,
            "--with-webcam" => options.webcam = true,
            "--no-webcam" => options.webcam = false,
            "--no-selfview" => options.self_view = false,
            _ if argument.starts_with("--webcam-device=") => {
                // QString::section('=', 1): everything after the first '='.
                options.webcam_device = argument.split_once('=').map_or("", |(_, rest)| rest).to_owned();
                options.webcam = true;
            }
            "--webcam-device" => {
                let Some(device) = value() else { return usage(err, "--webcam-device requires a value") };
                options.webcam_device = device;
                options.webcam = true;
            }
            "--webcam-height" => {
                let Some(height) = value() else { return usage(err, "--webcam-height requires a value") };
                options.webcam_height = to_int(&height);
                options.webcam = true;
            }
            "--no-audio" => {
                options.desktop_audio = false;
                options.microphone_audio = false;
            }
            "--microphone-device" => {
                let Some(device) = value() else { return usage(err, "--microphone-device requires a value") };
                options.microphone_device = device;
            }
            "--no-open" => options.no_open = true,
            "--no-bar" => options.no_bar = true,
            "--fps" | "--dir" => {
                let Some(given) = value() else { return usage(err, &format!("{argument} requires a value")) };
                if argument == "--fps" {
                    options.fps = to_int(&given);
                } else {
                    options.output_directory = qdir::absolute_path(&given);
                }
            }
            _ => return usage(err, &format!("unknown record option {argument}")),
        }
        index += 1;
    }
    if mode_count > 1 {
        return usage(err, "choose only one capture mode");
    }
    if has("--no-audio") && (has("--with-desktop-audio") || has("--with-microphone-audio")) {
        return usage(err, "--no-audio cannot be combined with audio enable flags");
    }
    if options.fps <= 0 || options.fps > 240 {
        return usage(err, "fps must be between 1 and 240");
    }
    if options.webcam && !options.webcam_device.starts_with("/dev/video") && !options.webcam_device.starts_with("/dev/v4l/")
    {
        return usage(err, "webcam device must be a /dev/video device or a /dev/v4l link");
    }
    if options.webcam_height != 720 && options.webcam_height != 1080 {
        return usage(err, "webcam height must be 720 or 1080");
    }
    if std::env::var_os("OMAREEL_NO_BAR").is_some_and(|value| value == "1") {
        options.no_bar = true;
    }
    let outcome = start_detached(&options);
    line(if outcome.code == 0 { out } else { err }, &outcome.message);
    outcome.code
}

/// `omareel __selfview-ipc <command>`: the command into the standby self-view host's file.
pub fn self_view_ipc(arguments: &[String]) -> i32 {
    if arguments.len() == 1 && state::send_host_command(&arguments[0]) { 0 } else { 2 }
}

const RECORD_BAR_IPC_USAGE: &str = "__record-bar-ipc requires rotate|flip|selfview hide|show|move|drag-sim";

/// `omareel __record-bar-ipc rotate|flip|selfview hide|show|selfview move X Y|selfview drag-sim
/// X0 Y0 X1 Y1`: tells the recording bar what to do through the recording's state file.
pub fn record_bar_ipc(arguments: &[String], err: &mut dyn Write) -> i32 {
    if arguments.is_empty() {
        return usage(err, RECORD_BAR_IPC_USAGE);
    }
    let next_sequence = || {
        state::recording_state().value("recording_bar_command_sequence").to_variant_long_long().wrapping_add(1)
    };
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let mut values = Object::new();
    match words.as_slice() {
        [command @ ("rotate" | "flip")] => {
            values.insert("recording_bar_command", *command);
            values.insert("recording_bar_command_sequence", next_sequence());
        }
        ["selfview", visibility @ ("hide" | "show")] => values.insert("selfview", *visibility == "show"),
        ["selfview", "move", x, y] => {
            let (Some(x), Some(y)) = (qnumber::to_int(x), qnumber::to_int(y)) else {
                return usage(err, "__record-bar-ipc selfview move requires integer x y");
            };
            values.insert("selfview_x", x);
            values.insert("selfview_y", y);
        }
        ["selfview", "drag-sim", x0, y0, x1, y1] => {
            let positions: Vec<Option<i32>> = [x0, y0, x1, y1].iter().map(|text| qnumber::to_int(text)).collect();
            let [Some(x0), Some(y0), Some(x1), Some(y1)] = positions[..] else {
                return usage(err, "__record-bar-ipc selfview drag-sim requires integer x0 y0 x1 y1");
            };
            values.insert("recording_bar_command", "drag-sim");
            values.insert("recording_bar_command_sequence", next_sequence());
            values.insert("drag_x0", x0);
            values.insert("drag_y0", y0);
            values.insert("drag_x1", x1);
            values.insert("drag_y1", y1);
        }
        _ => return usage(err, RECORD_BAR_IPC_USAGE),
    }
    if let Err(error) = state::update_recording_state(&values) {
        line(err, &format!("omareel: {error}"));
        return 2;
    }
    0
}
