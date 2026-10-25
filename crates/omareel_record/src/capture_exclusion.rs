//! `CaptureExclusion` and `HardwareCursorHold` (`src/record/CaptureExclusion.cpp`): omareel's
//! Hyprland plugin keeps the recording bar and the self-view out of the captured frames. These
//! ask Hyprland (through `hyprctl`, 2 s per call) whether the plugin is loaded and built for the
//! running commit, load it when it is not, choose the monitor the overlays go to when they have
//! to stay off the recorded one, and hold hardware cursors on while recording.
//!
//! The reasons are the C++'s words exactly; the launcher shows them.

use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qfile;
use omareel_core::compat::qjson::{self, Object, Value};
use omareel_core::compat::qprocess::{ExitStatus, Process, ProcessError, ProcessState};
use omareel_core::compat::qstring;

/// `CaptureExclusion::Status`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub loaded: bool,
    pub built_hash: String,
    pub reason: String,
    /// What `hyprctl omareel-exclude status` answered.
    pub details: Object,
}

/// What one `hyprctl` call gave.
struct Answer {
    /// The standard output when the call succeeded, else empty.
    output: Vec<u8>,
    ok: bool,
    /// What it printed on both channels, simplified, or why it said nothing.
    reply: String,
}

/// The file-static `hyprctl(arguments, &ok, &reply)`: waits 2 s, kills a call that is still
/// running after a failure, and keeps what it printed (even on failure) so a refusal can say why.
fn hyprctl(arguments: &[&str]) -> Answer {
    let mut process = Process::new();
    process.start("hyprctl", arguments);
    let finished = process.wait_for_finished(2000);
    let success = finished && process.exit_status() == ExitStatus::NormalExit && process.exit_code() == 0;
    if !success && process.state() != ProcessState::NotRunning {
        process.kill();
        process.wait_for_finished(1000);
    }
    let output = process.read_all_standard_output();
    let mut printed = output.clone();
    printed.extend(process.read_all_standard_error());
    let mut reply = qstring::simplified(&qstring::from_utf8(&printed));
    if !finished {
        reply = "hyprctl did not answer within 2 seconds".into();
    } else if reply.is_empty() && process.error() == ProcessError::FailedToStart {
        reply = "hyprctl could not be started".into();
    }
    Answer { output: if success { output } else { Vec::new() }, ok: success, reply }
}

/// `QJsonDocument::fromJson(bytes).object()`.
fn json_object(bytes: &[u8]) -> Object {
    match qjson::from_json(bytes) {
        Ok(Value::Object(object)) => object,
        _ => Object::new(),
    }
}

/// `QJsonDocument::fromJson(bytes).array()`.
fn json_array(bytes: &[u8]) -> Vec<Value> {
    match qjson::from_json(bytes) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// `setNoHardwareCursors`: `cursor:no_hardware_cursors` set through `hyprctl eval` (Lua
/// configurations), and through `hyprctl keyword` when that fails.
fn set_no_hardware_cursors(value: i32) -> bool {
    let eval = format!("hl.config({{ cursor = {{ no_hardware_cursors = {value} }} }})");
    let mut ok = hyprctl(&["eval", &eval]).ok;
    if !ok {
        ok = hyprctl(&["keyword", "cursor:no_hardware_cursors", &value.to_string()]).ok;
    }
    ok
}

/// `HardwareCursorHold`: Hyprland draws a software cursor into the frame itself, so screen
/// capture records it, and by default it uses one on NVIDIA whenever a monitor has VRR on.
/// Holding hardware cursors for the recording keeps the pointer out of `screen.mp4`; the
/// user's setting comes back when the hold is dropped.
#[derive(Debug)]
pub struct HardwareCursorHold {
    previous: i32,
}

impl HardwareCursorHold {
    /// Reads `cursor:no_hardware_cursors`; a value other than 0 is set to 0 and remembered.
    pub fn new() -> HardwareCursorHold {
        let mut hold = HardwareCursorHold { previous: -1 };
        let answer = hyprctl(&["-j", "getoption", "cursor:no_hardware_cursors"]);
        let option = json_object(&answer.output);
        if !answer.ok || !option.contains("int") {
            return hold;
        }
        let previous = option.value("int").to_int(0);
        if previous != 0 && set_no_hardware_cursors(0) {
            hold.previous = previous;
        }
        hold
    }
}

impl Default for HardwareCursorHold {
    fn default() -> HardwareCursorHold {
        HardwareCursorHold::new()
    }
}

impl Drop for HardwareCursorHold {
    fn drop(&mut self) {
        if self.previous >= 0 {
            set_no_hardware_cursors(self.previous);
        }
    }
}

/// `CaptureExclusion::compatibleHash`: the plugin's build hash is a commit (40 lowercase hex
/// digits, `^[a-f0-9]{40}$`, whose `$` also matches before a final newline) and Hyprland's.
pub fn compatible_hash(built: &str, running: &str) -> bool {
    let digits = built.strip_suffix('\n').unwrap_or(built);
    digits.len() == 40 && digits.bytes().all(|b| matches!(b, b'a'..=b'f' | b'0'..=b'9')) && built == running
}

/// `CaptureExclusion::status`: whether `omareel-capture-exclude` is in `hyprctl -j plugin list`,
/// built for the running commit (`hyprctl -j version`) and excluding the `omareel-` layers.
pub fn status() -> Status {
    let mut result = Status::default();
    let plugins = json_array(&hyprctl(&["-j", "plugin", "list"]).output);
    for value in plugins {
        if value.to_object().value("name").to_string_or("") != "omareel-capture-exclude" {
            continue;
        }
        result.details = json_object(&hyprctl(&["omareel-exclude", "status"]).output);
        result.built_hash = result.details.value("built_hash").to_string_or("");
        let version = json_object(&hyprctl(&["-j", "version"]).output);
        result.loaded = compatible_hash(&result.built_hash, &version.value("commit").to_string_or(""))
            && result.details.value("prefixes").to_array().contains(&Value::from("omareel-"));
        if !result.loaded {
            result.reason = "Plugin status or default prefix is unavailable".into();
        }
        return result;
    }
    result.reason = "Capture exclusion plugin is not loaded".into();
    result
}

/// `CaptureExclusion::ensureLoaded`: the plugin's [`status`], after loading it if it is not
/// loaded. `OMAREEL_PLUGIN_PATH`, when set (even to nothing), is the only candidate and must be
/// a file; otherwise `plugin/omareel-capture-exclude.so` beside the program
/// (`application_dir_path`, the C++'s `QCoreApplication::applicationDirPath()`) and
/// `/usr/lib/omareel/omareel-capture-exclude.so` are tried. A candidate's `.hash` file must hold
/// the running commit, and Hyprland must answer `ok` to `hyprctl plugin load`.
pub fn ensure_loaded(application_dir_path: &str) -> Status {
    let variable = std::env::var_os("OMAREEL_PLUGIN_PATH");
    let override_path =
        variable.as_ref().map(|value| String::from_utf8_lossy(value.as_encoded_bytes()).into_owned()).unwrap_or_default();
    // An explicit path is authoritative, including an intentionally unavailable path.
    if variable.is_some() && !FileInfo::new(override_path.clone()).is_file() {
        return Status { reason: "Configured capture exclusion plugin does not exist".into(), ..Status::default() };
    }
    let mut result = status();
    if result.loaded {
        return result;
    }
    let candidates = if variable.is_some() {
        vec![override_path]
    } else {
        vec![
            qdir::file_path(application_dir_path, "plugin/omareel-capture-exclude.so"),
            "/usr/lib/omareel/omareel-capture-exclude.so".to_owned(),
        ]
    };
    for path in candidates {
        if !FileInfo::new(path.clone()).is_file() {
            continue;
        }
        let built = match qfile::read_all(format!("{path}.hash")) {
            Ok(bytes) => qstring::trimmed(&qstring::from_utf8(&bytes)).to_owned(),
            Err(_) => String::new(),
        };
        let version = json_object(&hyprctl(&["-j", "version"]).output);
        if !compatible_hash(&built, &version.value("commit").to_string_or("")) {
            return Status {
                built_hash: built,
                reason: "Capture exclusion plugin needs rebuilding for this Hyprland commit".into(),
                ..Status::default()
            };
        }
        let absolute = FileInfo::new(path.clone()).absolute_file_path();
        let load = hyprctl(&["plugin", "load", &absolute]);
        // Hyprctl can exit successfully while the compositor returns a load error.
        if !load.ok || load.reply != "ok" {
            let reply = if load.reply.is_empty() { "no reply".to_owned() } else { load.reply };
            return Status {
                built_hash: built,
                reason: format!("Hyprland refused to load the capture exclusion plugin ({reply})"),
                ..Status::default()
            };
        }
        return status();
    }
    result.reason = "Capture exclusion plugin is not installed or was not built (it needs Hyprland's headers)".into();
    result
}

/// `CaptureExclusion::overlayMonitor`: where the bar and the self-view go. With the plugin
/// they may stay on the recorded monitor; otherwise the first other monitor, or none.
pub fn overlay_monitor(plugin: bool, recorded: &str, monitors: &[String]) -> String {
    if plugin && monitors.iter().any(|name| name == recorded) {
        return recorded.to_owned();
    }
    monitors.iter().find(|name| *name != recorded).cloned().unwrap_or_default()
}

/// `CaptureExclusion::overlaysOffMonitor`: whether `hyprctl -j layers` (a valid JSON object
/// naming the monitor) shows no layer whose namespace starts with `omareel-` on `monitor`.
pub fn overlays_off_monitor(monitor: &str) -> bool {
    let answer = hyprctl(&["-j", "layers"]);
    if !answer.ok {
        return false;
    }
    let Ok(Value::Object(document)) = qjson::from_json(&answer.output) else { return false };
    if !document.contains(monitor) {
        return false;
    }
    let levels = document.value(monitor).to_object().value("levels").to_object();
    for (_, level) in levels.iter() {
        for layer in level.to_array() {
            if layer.to_object().value("namespace").to_string_or("").starts_with("omareel-") {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_overlay_monitors() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        assert!(compatible_hash(hash, hash));
        assert!(compatible_hash(&format!("{hash}\n"), &format!("{hash}\n")));
        assert!(!compatible_hash(&hash.to_uppercase(), &hash.to_uppercase()));
        assert!(!compatible_hash(&hash[1..], &hash[1..]));
        assert!(!compatible_hash(hash, ""));
        let monitors = ["DP-1".to_owned(), "HDMI-A-1".to_owned()];
        assert_eq!(overlay_monitor(true, "DP-1", &monitors), "DP-1");
        assert_eq!(overlay_monitor(false, "DP-1", &monitors), "HDMI-A-1");
        assert_eq!(overlay_monitor(true, "X", &monitors[..1]), "DP-1");
        assert_eq!(overlay_monitor(false, "DP-1", &monitors[..1]), "");
        assert_eq!(qstring::simplified(" \t a \n\u{a0} b  "), "a b");
    }

    // test_recorder.cpp's captureExclusionRequiresExactValidHash.
    #[test]
    fn capture_exclusion_requires_exact_valid_hash() {
        let hash = "a".repeat(40);
        assert!(compatible_hash(&hash, &hash));
        assert!(!compatible_hash("", ""));
        assert!(!compatible_hash("unknown", "unknown"));
        assert!(!compatible_hash(&hash, &"b".repeat(40)));
    }

    // test_recorder.cpp's captureExclusionPlacesOverlaysAwayInFallback.
    #[test]
    fn capture_exclusion_places_overlays_away_in_fallback() {
        let monitors = ["DP-3".to_owned(), "DP-5".to_owned()];
        assert_eq!(overlay_monitor(true, "DP-3", &monitors), "DP-3");
        assert_eq!(overlay_monitor(false, "DP-3", &monitors), "DP-5");
        assert_eq!(overlay_monitor(false, "DP-5", &monitors), "DP-3");
        assert_eq!(overlay_monitor(false, "DP-3", &monitors[..1]), "");
        assert_eq!(overlay_monitor(false, "DP-3", &[]), "");
    }
}
