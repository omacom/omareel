//! The decisions inside `Launcher.cpp`, the window `omareel` opens to start a recording: how
//! cameras and microphones are listed and which one is selected, how its setters normalise what
//! QML passes, the status lines and the arguments of `omareel record`. The `Launcher` QObject
//! keeps the state, emits the signals, saves the preferences and talks to the self-view host;
//! these are what it computes.

use crate::camera::{CameraDevice, camera_can_record, camera_id_node};
use omareel_core::compat::qjson::Object;
use omareel_core::compat::qprocess::Process;
use omareel_core::compat::qround::q_round;
use omareel_core::compat::qstring;

/// `cameraName` (file-static): V4L2 names cameras `<device>: <stream>`, cut to 31 characters
/// ("Cam Link 4K: Cam Link 4K", "Integrated Webcam: Integrated W"); a stream half that only
/// repeats the device is dropped.
pub fn camera_name(description: &str) -> String {
    let Some(separator) = description.find(": ").filter(|&separator| separator > 0) else {
        return description.to_owned();
    };
    let device = qstring::trimmed(&description[..separator]);
    let stream = qstring::trimmed(&description[separator + 2..]);
    if device.starts_with(stream) { device.to_owned() } else { description.to_owned() }
}

/// `Launcher::formatDuration`: whole seconds (rounded with `qRound`, never below 0) as
/// `MM:SS`, the minutes growing past two digits.
pub fn format_duration(seconds: f64) -> String {
    let total = q_round(seconds).max(0);
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// What `setWebcamHeight` makes of a value before comparing it: 720, or 1080 for anything else.
pub fn normalized_webcam_height(value: i32) -> i32 {
    if value == 720 { 720 } else { 1080 }
}

/// What `setWebcamRotation` makes of a value: the angle in `[0, 360)`, and 0 unless that is a
/// quarter turn.
pub fn normalized_webcam_rotation(value: i32) -> i32 {
    // ((value % 360) + 360) % 360 in the C++, which is the Euclidean remainder for every int.
    let value = value.rem_euclid(360);
    if matches!(value, 0 | 90 | 180 | 270) { value } else { 0 }
}

/// What `setSelfViewSize` makes of a value: `S`, `M` or `L` in upper case, or `None` when the
/// setter ignores it.
pub fn normalized_self_view_size(value: &str) -> Option<String> {
    let normalized = qstring::to_upper(value);
    matches!(normalized.as_str(), "S" | "M" | "L").then_some(normalized)
}

/// The launcher's note when the capture exclusion plugin is not loaded (the constructor), for
/// the number of screens.
pub fn capture_exclusion_note(screens: usize) -> &'static str {
    if screens > 1 {
        "Overlays move off the recorded screen while recording"
    } else {
        "Self-view hides while recording"
    }
}

/// A microphone as the launcher lists it (`{value, text}` in `audioDevices`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// The PulseAudio source name, as the recorder takes it, or `default_input`.
    pub value: String,
    pub text: String,
}

/// `refreshAudioDevices`'s listing: what `gpu-screen-recorder --list-audio-devices` printed when
/// it ran (`None` when it did not finish in time or failed), as `<id>|<description>` lines of
/// which only `default_input` and `alsa_input.*` count; `Default input` when nothing does.
pub fn audio_devices_from(output: Option<&[u8]>) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    if let Some(output) = output {
        for line in qstring::from_utf8(output).split('\n').filter(|line| !line.is_empty()) {
            let Some(separator) = line.find('|').filter(|&separator| separator > 0) else { continue };
            let id = qstring::trimmed(&line[..separator]);
            if id != "default_input" && !id.starts_with("alsa_input.") {
                continue;
            }
            devices
                .push(AudioDevice { value: id.to_owned(), text: qstring::trimmed(&line[separator + 1..]).to_owned() });
        }
    }
    if devices.is_empty() {
        devices.push(AudioDevice { value: "default_input".into(), text: "Default input".into() });
    }
    devices
}

/// `refreshAudioDevices`'s listing, asking `gpu-screen-recorder --list-audio-devices` (5 s).
pub fn audio_devices() -> Vec<AudioDevice> {
    let mut process = Process::new();
    process.start("gpu-screen-recorder", &["--list-audio-devices"]);
    let output =
        (process.wait_for_finished(5000) && process.exit_code() == 0).then(|| process.read_all_standard_output());
    audio_devices_from(output.as_deref())
}

/// The microphone `refreshAudioDevices` records from: the one last picked while it is listed,
/// else the default; `changed` when that differs from the current one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicrophoneSelection {
    pub microphone: String,
    pub changed: bool,
}

/// `refreshAudioDevices`'s choice among `devices`. A microphone that isn't plugged in stays
/// chosen, so it is used again once it is back.
pub fn select_microphone(devices: &[AudioDevice], chosen: &str, current: &str) -> MicrophoneSelection {
    let chosen_found = devices.iter().any(|device| device.value == chosen);
    let microphone = if chosen_found { chosen.to_owned() } else { "default_input".to_owned() };
    MicrophoneSelection { changed: microphone != current, microphone }
}

/// A camera as the launcher lists it (`webcamDevices`): the path settings keep, the name shown,
/// and the `/dev/videoN` node (missing for the saved path listed when Qt sees no camera).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebcamDevice {
    pub value: String,
    pub text: String,
    pub node: Option<String>,
}

/// What `refreshWebcamDevices` lists and selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebcamSelection {
    pub devices: Vec<WebcamDevice>,
    /// The camera recorded from (`m_webcamDevice` afterwards).
    pub device: String,
    /// Whether that differs from the current one, so the preferences are saved and the
    /// self-view host reconfigured.
    pub changed: bool,
}

/// `refreshWebcamDevices` over the cameras Qt lists: those that can record, once per node, by
/// their stable path ([`crate::camera::stable_camera_path`], passed in) and V4L2 name, two of a
/// name told apart by their node; with no camera at all, the saved path when it exists. The
/// camera last chosen is selected when it is listed, else the current one when it is (or nothing
/// is), else the first.
pub fn webcam_devices(
    cameras: &[CameraDevice],
    webcam_device: &str,
    chosen_webcam_device: &str,
    stable_camera_path: impl Fn(&str) -> String,
    exists: impl Fn(&str) -> bool,
) -> WebcamSelection {
    let mut devices: Vec<WebcamDevice> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for camera in cameras {
        if !camera_can_record(camera) {
            continue;
        }
        let node = camera_id_node(&camera.id);
        if node.is_empty() || seen.contains(&node) {
            continue;
        }
        seen.push(node.clone());
        devices.push(WebcamDevice {
            value: stable_camera_path(&node),
            text: camera_name(&camera.description),
            node: Some(node),
        });
    }
    // Two cameras of the same model are told apart by their device node. Entries renamed
    // earlier in the loop no longer count towards a name.
    for index in 0..devices.len() {
        let text = devices[index].text.clone();
        if devices.iter().filter(|other| other.text == text).count() > 1 {
            // QString("%1 (%2)").arg(text, node): both at once, neither scanned again.
            let node = devices[index].node.clone().unwrap_or_default();
            devices[index].text = format!("{text} ({node})");
        }
    }
    if cameras.is_empty() && exists(webcam_device) {
        devices.push(WebcamDevice { value: webcam_device.to_owned(), text: webcam_device.to_owned(), node: None });
    }
    let listed = |path: &str| devices.iter().any(|device| device.value == path);
    let selected = if listed(chosen_webcam_device) {
        chosen_webcam_device.to_owned()
    } else if listed(webcam_device) || devices.is_empty() {
        webcam_device.to_owned()
    } else {
        devices[0].value.clone()
    };
    WebcamSelection { changed: selected != webcam_device, device: selected, devices }
}

/// The self-view status line of `refreshRecording` (with the webcam on), from the self-view
/// host's state file. `host_alive` stands for the `kill(pid, 0)` check, made only for a pid
/// above 0.
pub fn self_view_status(host_pid: i64, host_alive: impl FnOnce() -> bool, host: &Object) -> String {
    let mut status = "Starting camera…".to_owned();
    let camera_error = host.value("camera_error").to_string_or("");
    if host_pid > 0 && !host_alive() {
        status = "Camera preview closed".into();
    } else if !camera_error.is_empty() {
        status = camera_error;
    } else if host.value("camera_status").to_string_or("") == "ready" {
        status = if host.value("visible").to_bool(false) {
            // arg() does not scan the monitor's name for placeholders.
            format!("Self-view floating on {} — drag it where you want it", host.value("monitor").to_string_or(""))
        } else {
            "Camera ready — self-view hidden".into()
        };
    }
    status
}

/// The arguments of `omareel record` the launcher starts (`record`'s start timer): the audio
/// choice, the chosen microphone even while unplugged (the recorder uses the default one then
/// and says so), and the camera.
pub fn record_arguments(
    system_audio: bool,
    microphone: bool,
    chosen_microphone_device: &str,
    webcam: bool,
    webcam_device: &str,
    webcam_height: i32,
) -> Vec<String> {
    let mut arguments = vec!["record".to_owned()];
    if !system_audio && !microphone {
        arguments.push("--no-audio".into());
    } else {
        if system_audio {
            arguments.push("--with-desktop-audio".into());
        }
        if microphone {
            arguments.extend([
                "--with-microphone-audio".into(),
                "--microphone-device".into(),
                chosen_microphone_device.into(),
            ]);
        }
    }
    if webcam {
        arguments.extend([
            "--with-webcam".into(),
            "--webcam-device".into(),
            webcam_device.into(),
            "--webcam-height".into(),
            webcam_height.to_string(),
        ]);
    }
    arguments
}
