//! `CaptureExclusion` and `HardwareCursorHold` (`src/record/CaptureExclusion.cpp`): omareel's
//! Hyprland plugin keeps the recording bar and the self-view out of the captured frames. These
//! ask Hyprland (through `hyprctl`, 2 s per call) whether the plugin is loaded and built for the
//! running commit, load it when it is not, choose the monitor the overlays go to when they have
//! to stay off the recorded one, and hold hardware cursors on while recording.
//!
//! The reasons are the C++'s words exactly; the launcher shows them.

use crate::recorder::state;
use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qfile;
use omareel_core::compat::qjson::{self, Format, Object, Value};
use omareel_core::compat::qprocess::{ExitStatus, Process, ProcessError, ProcessState};
use omareel_core::compat::qsavefile::SaveFile;
use omareel_core::compat::qstring;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

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
/// configurations), and through `hyprctl keyword` when that fails. Hyprland answers `ok` when it
/// took the value; a refusal comes back with hyprctl's exit status 0 all the same (0.56 answers
/// `eval` under a hyprlang configuration with "eval is only supported with the lua config
/// manager"), so anything but `ok` is a failure.
fn set_no_hardware_cursors(value: i32) -> bool {
    let accepted = |answer: Answer| answer.ok && answer.reply == "ok";
    let eval = format!("hl.config({{ cursor = {{ no_hardware_cursors = {value} }} }})");
    accepted(hyprctl(&["eval", &eval])) || accepted(hyprctl(&["keyword", "cursor:no_hardware_cursors", &value.to_string()]))
}

/// `cursor:no_hardware_cursors` as `hyprctl -j getoption` gives it, `None` when it can't be read
/// or is not an `int`.
fn no_hardware_cursors() -> Option<i32> {
    let answer = hyprctl(&["-j", "getoption", "cursor:no_hardware_cursors"]);
    let option = json_object(&answer.output);
    if !answer.ok || !option.contains("int") {
        return None;
    }
    Some(option.value("int").to_int(0))
}

/// An exclusive `flock` on [`state::hardware_cursor_hold_lock_path`] for as long as it lives, so
/// that restoring an abandoned hold, taking a hold and giving one back are each one step for
/// every process: a launcher restoring a dead recorder's value can't land between a new
/// recorder's marker and its change. The kernel lets go of it when the process dies. Without a
/// lock file (an unwritable runtime directory) it holds nothing and the steps run unlocked.
struct HoldLock {
    _fd: Option<OwnedFd>,
}

impl HoldLock {
    /// Waits for the lock: the steps it guards take at most a few `hyprctl` calls.
    fn acquire() -> HoldLock {
        let path = state::hardware_cursor_hold_lock_path();
        qdir::mkpath(&FileInfo::new(path.clone()).absolute_path());
        let Ok(path) = std::ffi::CString::new(path) else { return HoldLock { _fd: None } };
        // SAFETY: a NUL-terminated path; open returns a new descriptor or -1.
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC, 0o600) };
        if fd < 0 {
            return HoldLock { _fd: None };
        }
        // SAFETY: the descriptor was just opened and is owned here.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        loop {
            // SAFETY: flock on a descriptor owned here.
            if unsafe { libc::flock(fd.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return HoldLock { _fd: Some(fd) };
            }
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return HoldLock { _fd: None };
            }
        }
    }
}

/// Writes the hold's marker at `path` through a `QSaveFile`, so it appears whole or not at all:
/// the value to put back, the process holding it, and the Hyprland instance it was changed on.
/// False when it could not be written.
fn write_hold_marker(path: &str, previous: i32) -> bool {
    qdir::mkpath(&FileInfo::new(path).absolute_path());
    let mut marker = Object::new();
    marker.insert("previous", previous);
    marker.insert("pid", i64::from(std::process::id()));
    marker.insert("instance", hyprland_instance());
    let mut file = SaveFile::new(path);
    if !file.open() {
        return false;
    }
    file.write(Value::Object(marker).to_json(Format::Indented).as_bytes());
    file.commit()
}

/// `$HYPRLAND_INSTANCE_SIGNATURE`, the instance `hyprctl` talks to.
fn hyprland_instance() -> String {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .map(|value| qstring::from_utf8(value.as_encoded_bytes()))
        .unwrap_or_default()
}

/// Whether Hyprland instance `instance` still runs: its request socket is in
/// `$XDG_RUNTIME_DIR/hypr/`.
fn instance_running(instance: &str) -> bool {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default();
    !instance.is_empty() && std::path::Path::new(&runtime).join("hypr").join(instance).join(".socket.sock").exists()
}

/// Removes the markers of Hyprland instances that are gone (a new Hyprland read its
/// configuration afresh, so there is nothing to put back), leaving this instance's and running
/// ones' alone.
fn forget_gone_instances(current: &str) {
    let directory = FileInfo::new(state::hardware_cursor_hold_lock_path()).absolute_path();
    let Ok(entries) = std::fs::read_dir(&directory) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("hardware-cursor-hold-") || !name.ends_with(".json") {
            continue;
        }
        let path = entry.path().to_string_lossy().into_owned();
        if path == current {
            continue;
        }
        let marker = qfile::read_all(&path).map(|bytes| json_object(&bytes)).unwrap_or_default();
        if !instance_running(&marker.value("instance").to_string_or("")) {
            qfile::remove(&path);
        }
    }
}

/// [`restore_abandoned_hardware_cursor`] with the [`HoldLock`] already held.
fn restore_abandoned_locked() -> bool {
    let path = state::hardware_cursor_hold_path(&hyprland_instance());
    forget_gone_instances(&path);
    let Ok(bytes) = qfile::read_all(&path) else { return false };
    let marker = json_object(&bytes);
    let pid = marker.value("pid").to_variant_long_long();
    if pid == i64::from(std::process::id()) || state::recorder_daemon_alive(pid) {
        return false;
    }
    let previous = marker.value("previous").to_int(-1);
    match no_hardware_cursors() {
        // Hyprland did not answer: try again at the next start.
        None => false,
        Some(0) if previous > 0 => {
            let restored = set_no_hardware_cursors(previous);
            if restored {
                qfile::remove(&path);
            }
            restored
        }
        Some(_) => {
            qfile::remove(&path);
            false
        }
    }
}

/// Puts back the `cursor:no_hardware_cursors` value of a hold on this Hyprland instance whose
/// recorder is gone (killed, or crashed before its hold was dropped), and says whether it did.
/// The instance's marker is left alone while its process is a live recorder daemon or is this
/// process (whose hold is still to be dropped); it is removed once the value is back, and when
/// the setting no longer holds the 0 the recorder set (the user or a reload changed it since).
/// Markers of instances that are gone are removed; those of other running instances are theirs
/// to restore. Launcher and recorder starts call it, and `omareel stop` after killing a daemon;
/// it runs under the [`HoldLock`].
pub fn restore_abandoned_hardware_cursor() -> bool {
    let _lock = HoldLock::acquire();
    restore_abandoned_locked()
}

/// `HardwareCursorHold`: Hyprland draws a software cursor into the frame itself, so screen
/// capture records it, and by default it uses one on NVIDIA whenever a monitor has VRR on.
/// Holding hardware cursors for the recording keeps the pointer out of `screen.mp4`; the
/// user's setting comes back when the hold is dropped.
///
/// A process that dies holding it never drops it, so the user's value is also written to this
/// instance's marker in the runtime directory before the setting changes, and removed once it is
/// back; the next start puts back what a dead recorder left
/// ([`restore_abandoned_hardware_cursor`]). Taking and giving back the hold happen under the
/// [`HoldLock`].
#[derive(Debug)]
pub struct HardwareCursorHold {
    previous: i32,
    /// This instance's marker.
    marker: String,
}

impl HardwareCursorHold {
    /// Puts back what an abandoned hold left, then reads `cursor:no_hardware_cursors`; a value
    /// other than 0 is set to 0 and remembered (in the marker too when it is one the drop puts
    /// back: a negative value never was).
    pub fn new() -> HardwareCursorHold {
        let _lock = HoldLock::acquire();
        restore_abandoned_locked();
        let marker = state::hardware_cursor_hold_path(&hyprland_instance());
        let mut hold = HardwareCursorHold { previous: -1, marker };
        let Some(previous) = no_hardware_cursors() else { return hold };
        if previous == 0 {
            return hold;
        }
        // Written first, so no moment has the setting changed and nothing to restore it from.
        // A marker that can't be written leaves the hold as it was before markers.
        let marked = previous > 0 && write_hold_marker(&hold.marker, previous);
        if set_no_hardware_cursors(0) {
            hold.previous = previous;
        } else if marked {
            qfile::remove(&hold.marker);
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
        if self.previous < 0 {
            return;
        }
        let _lock = HoldLock::acquire();
        // A failed restore keeps the marker, for the next start to retry.
        if set_no_hardware_cursors(self.previous) {
            qfile::remove(&self.marker);
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
