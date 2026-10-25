//! launcher: `launcher` against the decisions copied from `Launcher.cpp` (README "launcher"):
//! camera names, durations, the setters' normalisation, the capture exclusion note, the
//! microphone list from `gpu-screen-recorder --list-audio-devices` (replayed), the camera list
//! over cameras given as data, with `stableCameraPath` and `QFileInfo::exists` answered from the
//! case, the self-view status line and the arguments of `omareel record`.

use crate::camera::camera;
use crate::hermetic::*;
use crate::replay::*;
use crate::support::*;
use omareel_core::compat::qjson::{self, Format, Object, Value};
use omareel_record::launcher::*;

/// The **devices** cell: `[{<key>=<value str>,...},...]`, keys in `QVariantMap` order.
fn audio_list(devices: &[AudioDevice]) -> String {
    let items: Vec<String> = devices
        .iter()
        .map(|device| format!("{{text={},value={}}}", quote(&device.text), quote(&device.value)))
        .collect();
    format!("[{}]", items.join(","))
}

fn webcam_list(devices: &[WebcamDevice]) -> String {
    let items: Vec<String> = devices
        .iter()
        .map(|device| match &device.node {
            Some(node) => {
                format!("{{node={},text={},value={}}}", quote(node), quote(&device.text), quote(&device.value))
            }
            None => format!("{{text={},value={}}}", quote(&device.text), quote(&device.value)),
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// `formatDuration` goes through x86_64's qRound (0.49999999999999994 rounds to 1 there).
#[cfg(target_arch = "x86_64")]
#[test]
fn helpers_txt() {
    let cases = read_json("launcher/cases.json");
    let mut out = Text::new();
    for name in cases.get("cameraNames").to_array() {
        let name = name.to_string_or("");
        out.row(["cameraName".to_owned(), quote(&name), quote(&camera_name(&name))]);
    }
    let inf = f64::INFINITY;
    let seconds = [
        0.0,
        -0.0,
        -1.0,
        0.4,
        0.5,
        0.49999999999999994,
        1.5,
        2.5,
        59.4,
        59.5,
        60.0,
        61.0,
        599.5,
        3599.4,
        3600.0,
        5999.0,
        6000.0,
        86399.5,
        359999.0,
        2147483646.0,
        -2147483647.0,
        f64::NAN,
        inf,
        -inf,
    ];
    for value in seconds {
        // qRound of NaN, infinities and values past int are undefined; the oracle skips them.
        let text =
            if value.is_finite() && value.abs() < 2147483647.0 { quote(&format_duration(value)) } else { "-".into() };
        out.row(["formatDuration".to_owned(), num(value), text]);
    }
    for value in [720, 1080, 0, 480, 721, -720, 2160] {
        out.row(["setWebcamHeight".to_owned(), value.to_string(), normalized_webcam_height(value).to_string()]);
    }
    for value in [0, 90, 180, 270, 360, 450, -90, -180, -270, -360, 45, 89, 1, -1, 720, i32::MAX, i32::MIN] {
        out.row(["setWebcamRotation".to_owned(), value.to_string(), normalized_webcam_rotation(value).to_string()]);
    }
    for value in ["S", "m", "l", "XL", "", " M", "ß", "ſ"] {
        let normalized = normalized_self_view_size(value).unwrap_or_else(|| "<ignored>".into());
        out.row(["setSelfViewSize".to_owned(), quote(value), quote(&normalized)]);
    }
    for screens in 0..4 {
        out.row(["captureExclusionNote".to_owned(), screens.to_string(), quote(capture_exclusion_note(screens))]);
    }
    assert_golden("launcher/helpers.txt", &out.into_string());
}

#[test]
fn audio_devices_txt() {
    let h = Hermetic::new("launcher");
    let cases = read_json("launcher/cases.json");
    let mut out = Text::new();
    for input in cases.get("audio").to_array() {
        let input = input.to_object();
        let mut replay = Replay::new(&format!("{}/replay", h.scratch()));
        let mut environment = ScopedEnvironment::new(&h);
        let file = input.value("file").as_str().map(str::to_owned);
        let status = input.value("status").to_int(0);
        match &file {
            Some(file) => replay.answer(
                "gpu-screen-recorder",
                &["--list-audio-devices"],
                &Answer { status, ..Answer::stdout(read_corpus(file)) },
                0,
            ),
            None => environment.set("PATH", shim_directory(&h.scratch(), "empty", &[])),
        }
        replay.activate(&h);
        let chosen = input.value("chosen").to_string_or("");
        let current = input.value("current").to_string_or("");
        let devices = audio_devices();
        let selection = select_microphone(&devices, &chosen, &current);
        out.row([
            file.unwrap_or_else(|| "<gpu-screen-recorder not on PATH>".into()),
            status.to_string(),
            quote(&chosen),
            quote(&current),
            audio_list(&devices),
            quote(&selection.microphone),
            boolean(selection.changed).into(),
        ]);
    }
    assert_golden("launcher/audio-devices.txt", &out.into_string());
}

#[test]
fn webcam_devices_txt() {
    let cases = read_json("launcher/cases.json");
    let stable = cases.get("stable").to_object();
    let mut out = Text::new();
    for (name, input) in cases.get("webcams").to_object().iter() {
        let input = input.to_object();
        let existing: Vec<String> = input.value("exists").to_array().iter().map(|path| path.to_string_or("")).collect();
        let cameras: Vec<_> = input
            .value("cameras")
            .to_array()
            .iter()
            .map(|camera_value| {
                let object = camera_value.to_object();
                camera(
                    object.value("id").to_string_or("").as_bytes(),
                    &object.value("description").to_string_or(""),
                    object.value("formats"),
                )
            })
            .collect();
        let selection = webcam_devices(
            &cameras,
            &input.value("device").to_string_or(""),
            &input.value("chosen").to_string_or(""),
            |node| stable.value(node).to_string_or(node),
            |path| existing.iter().any(|existing| existing == path),
        );
        out.section(name);
        out.row(["devices".to_owned(), webcam_list(&selection.devices)]);
        out.row(["selected".to_owned(), quote(&selection.device), boolean(selection.changed).into()]);
    }
    assert_golden("launcher/webcam-devices.txt", &out.into_string());
}

#[test]
fn status_txt() {
    let hosts = [
        r#"{}"#,
        r#"{"camera_status": "starting"}"#,
        r#"{"camera_status": "ready", "visible": true, "monitor": "DP-1"}"#,
        r#"{"camera_status": "ready", "visible": false, "monitor": "DP-1"}"#,
        r#"{"camera_status": "ready", "visible": "true", "monitor": 3}"#,
        r#"{"camera_status": "ready", "visible": true}"#,
        r#"{"camera_status": "ready", "camera_error": "Selected camera is unavailable"}"#,
        r#"{"camera_status": "failed", "camera_error": ""}"#,
        r#"{"camera_status": "READY", "visible": true}"#,
        r#"{"camera_status": "ready", "visible": true, "monitor": "Écran %1 ☃"}"#,
    ];
    let hosts: Vec<Object> =
        hosts.iter().map(|json| qjson::object_from_json(json.as_bytes()).expect("an object")).collect();
    let mut out = Text::new();
    for pid in [0i64, 4242] {
        for alive in [true, false] {
            for host in &hosts {
                if pid == 0 && !alive {
                    continue;
                }
                out.row([
                    "selfViewStatus".to_owned(),
                    pid.to_string(),
                    boolean(alive).into(),
                    Value::Object(host.clone()).to_json(Format::Compact),
                    quote(&self_view_status(pid, || alive, host)),
                ]);
            }
        }
    }
    for system in [false, true] {
        for microphone in [false, true] {
            for webcam in [false, true] {
                let arguments = record_arguments(
                    system,
                    microphone,
                    "alsa_input.unplugged",
                    webcam,
                    "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0",
                    720,
                );
                out.row([
                    "recordArguments".to_owned(),
                    boolean(system).into(),
                    boolean(microphone).into(),
                    boolean(webcam).into(),
                    quote_list(&arguments),
                ]);
            }
        }
    }
    assert_golden("launcher/status.txt", &out.into_string());
}
