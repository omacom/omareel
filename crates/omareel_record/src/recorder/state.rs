//! The recorder's state files (`src/record/Recorder.cpp`): `recording.json`, which the daemon,
//! the recording bar, the launcher and `omareel stop` share while a recording runs,
//! `selfview-host.json`, through which the launcher commands the standby self-view host, and
//! `last-error.txt`, which tells `omareel record` why a daemon gave up. They live in
//! `$XDG_RUNTIME_DIR/omareel/` (`/tmp/omareel/` without it).
//!
//! Every change goes through [`update_state_file`]: under the QLockFile-compatible
//! `<file>.lock`, the file is read, merged with the new values and replaced through a
//! `QSaveFile`, so the C++ and the Rust processes can write the same files. A `command` without
//! its own `command_sequence` bumps the stored sequence, which is how a reader notices that the
//! same command was given again.

use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qjson::{self, Format, Object, Value};
use omareel_core::compat::qlockfile::{LockError, LockFile};
use omareel_core::compat::qsavefile::SaveFile;
use omareel_core::compat::qprocess::{self, DetachedChannels};
use omareel_core::compat::{qfile, qstandardpaths, qstring};
use std::os::unix::ffi::OsStrExt;

/// `qEnvironmentVariable(name)`: the variable as UTF-8 text, empty when unset.
fn environment_variable(name: &str) -> String {
    std::env::var_os(name).map(|value| qstring::from_utf8(value.as_bytes())).unwrap_or_default()
}

/// `Recorder::stateFilePath()`: `$XDG_RUNTIME_DIR/omareel/recording.json`, with `/tmp` for an
/// unset or empty variable. The variable is used as it is (a relative or slash-ended value too).
pub fn state_file_path() -> String {
    let mut runtime = environment_variable("XDG_RUNTIME_DIR");
    if runtime.is_empty() {
        runtime = "/tmp".into();
    }
    runtime + "/omareel/recording.json"
}

/// The absolute directory of [`state_file_path`] (`QFileInfo(stateFilePath()).absolutePath()`).
fn state_directory() -> String {
    FileInfo::new(state_file_path()).absolute_path()
}

/// `lastErrorFilePath()`: `last-error.txt` beside the state file.
pub fn last_error_file_path() -> String {
    state_directory() + "/last-error.txt"
}

/// `Recorder::selfViewHostPath()`: `selfview-host.json` beside the state file.
pub fn self_view_host_path() -> String {
    state_directory() + "/selfview-host.json"
}

/// `hardware-cursor-hold.json` beside the state file: the `cursor:no_hardware_cursors` value a
/// recording's `HardwareCursorHold` changed, kept while the hold lasts so that a start after a
/// crash can put it back (`capture_exclusion::restore_abandoned_hardware_cursor`).
pub fn hardware_cursor_hold_path() -> String {
    state_directory() + "/hardware-cursor-hold.json"
}

/// `QJsonDocument::fromJson(bytes).object()`: an empty object for anything but an object.
fn object_or_empty(bytes: &[u8]) -> Object {
    match qjson::from_json(bytes) {
        Ok(Value::Object(object)) => object,
        _ => Object::new(),
    }
}

/// `Recorder::readStateFile(path)`: the file's JSON object, empty when it can't be read or holds
/// anything else.
pub fn read_state_file(path: &str) -> Object {
    qfile::read_all(path).map(|bytes| object_or_empty(&bytes)).unwrap_or_default()
}

/// `readState()`: the recording's state file.
fn read_state() -> Object {
    read_state_file(&state_file_path())
}

/// `Recorder::updateStateFile(path, values, &error)`: merges `values` into the file under its
/// lock. `Err` holds what the C++ put in `error`: "Recording state is busy" when another process
/// held the lock for the whole second, "Could not lock recording state" when the lock file
/// could not be made, or the `QSaveFile` error.
pub fn update_state_file(path: &str, values: &Object) -> Result<(), String> {
    qdir::mkpath(&FileInfo::new(path).absolute_path());
    let mut lock = LockFile::new(format!("{path}.lock"));
    lock.set_stale_lock_time(10_000);
    if !lock.try_lock(1000) {
        return Err(if lock.error() == LockError::LockFailedError {
            "Recording state is busy".into()
        } else {
            "Could not lock recording state".into()
        });
    }
    let mut state = read_state_file(path);
    if values.contains("command") && !values.contains("command_sequence") {
        // qint64 + 1; the C++ overflow at INT64_MAX is undefined, so this wraps.
        let sequence = state.value("command_sequence").to_variant_long_long().wrapping_add(1);
        state.insert("command_sequence", sequence);
    }
    for (key, value) in values.iter() {
        state.insert(key, value.clone());
    }
    let mut file = SaveFile::new(path);
    if !file.open() {
        return Err(file.error_string());
    }
    file.write(Value::Object(state).to_json(Format::Indented).as_bytes());
    if file.commit() { Ok(()) } else { Err(file.error_string()) }
}

/// `Recorder::sendHostCommand(command)`: `{"command": command}` into the self-view host's file.
pub fn send_host_command(command: &str) -> bool {
    let mut values = Object::new();
    values.insert("command", command);
    update_state_file(&self_view_host_path(), &values).is_ok()
}

/// `Recorder::recordingState()`.
pub fn recording_state() -> Object {
    read_state()
}

/// `Recorder::updateRecordingState(values, &error)`.
pub fn update_recording_state(values: &Object) -> Result<(), String> {
    update_state_file(&state_file_path(), values)
}

/// `Recorder::recordingStartedUs()`: `started_us` of the state.
pub fn recording_started_us() -> i64 {
    read_state().value("started_us").to_variant_long_long()
}

/// `Recorder::recordedMonitor()`: `monitor` of the state.
pub fn recorded_monitor() -> String {
    read_state().value("monitor").to_string_or("")
}

/// `Recorder::recordingHasWebcam()`: `webcam` of the state.
pub fn recording_has_webcam() -> bool {
    read_state().value("webcam").to_bool(false)
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `kill(pid_t(pid), signal)`, the pid truncated to `pid_t` as the C++ cast does.
fn kill(pid: i64, signal: i32) -> i32 {
    // SAFETY: kill takes any pid and signal and reports errors through its result.
    unsafe { libc::kill(pid as libc::pid_t, signal) }
}

/// `QString::fromLocal8Bit(strerror(code))`.
fn strerror(code: i32) -> String {
    let mut buffer = [0 as libc::c_char; 256];
    // SAFETY: the buffer and its length are passed together; the XSI strerror_r NUL-terminates.
    if unsafe { libc::strerror_r(code, buffer.as_mut_ptr(), buffer.len()) } != 0 {
        return format!("Unknown error {code}");
    }
    // SAFETY: strerror_r wrote a NUL-terminated string into `buffer`.
    let text = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
    qstring::from_utf8(text.to_bytes())
}

/// `processAlive(pid)`: a pid above 1 that `kill(pid, 0)` finds (or may not signal) and whose
/// `/proc/<pid>/stat` state is not zombie (Z) or dead (X): detached children can stay zombies
/// until their parent reaps them.
pub fn process_alive(pid: i64) -> bool {
    if pid <= 1 || (kill(pid, 0) != 0 && errno() != libc::EPERM) {
        return false;
    }
    let Ok(fields) = qfile::read_all(format!("/proc/{pid}/stat")) else { return false };
    let state_offset = fields.iter().rposition(|&b| b == b')').map_or(-1, |i| i as isize) + 2;
    state_offset > 1
        && (state_offset as usize) < fields.len()
        && fields[state_offset as usize] != b'Z'
        && fields[state_offset as usize] != b'X'
}

/// `recorderDaemonAlive(pid)`: [`process_alive`] and its command line holds `__record-daemon`.
pub fn recorder_daemon_alive(pid: i64) -> bool {
    if !process_alive(pid) {
        return false;
    }
    let Ok(command_line) = qfile::read_all(format!("/proc/{pid}/cmdline")) else { return false };
    command_line.windows(b"__record-daemon".len()).any(|window| window == b"__record-daemon")
}

/// `QByteArray::simplified()`: ASCII whitespace trimmed and runs of it made one space.
fn simplified(bytes: &[u8]) -> Vec<u8> {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    bytes.split(space).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(&b' ')
}

/// `QByteArray::toLongLong(&ok)`: base 10, surrounding whitespace allowed.
fn to_long_long(bytes: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(qfile::trimmed(bytes)).ok()?;
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `processDescendants(parentPid)`: every process below it, from
/// `/proc/<pid>/task/<pid>/children`, depth first from the last child found.
pub fn process_descendants(parent_pid: i64) -> Vec<i64> {
    let mut result = Vec::new();
    let mut pending = vec![parent_pid];
    while let Some(parent) = pending.pop() {
        let Ok(children) = qfile::read_all(format!("/proc/{parent}/task/{parent}/children")) else { continue };
        for id in simplified(&children).split(|&b| b == b' ') {
            let Some(child) = to_long_long(id) else { continue };
            if child <= 1 || result.contains(&child) {
                continue;
            }
            result.push(child);
            pending.push(child);
        }
    }
    result
}

/// `Recorder::isRecording()`: whether the state names a live recorder daemon. A state file
/// left by one that is gone is removed.
pub fn is_recording() -> bool {
    let state = read_state();
    let pid = state.value("pid").to_variant_long_long();
    if recorder_daemon_alive(pid) {
        return true;
    }
    if !state.is_empty() {
        qfile::remove(&state_file_path());
    }
    false
}

/// `runOptionalDetached(program, arguments)`: when the program is installed, starts it as
/// `QProcess::startDetached` does (a grandchild in a session of its own, with this process's
/// standard channels, never waited for).
pub fn run_optional_detached<S: AsRef<str>>(program: &str, arguments: &[S]) {
    if qstandardpaths::find_executable(program).is_empty() {
        return;
    }
    qprocess::start_detached(program, arguments, DetachedChannels::default());
}

/// `notifyStopFailure(detail)`: a critical desktop notification that lasts five seconds.
pub fn notify_stop_failure(detail: &str) {
    run_optional_detached(
        "omarchy-notification-send",
        &["-u", "critical", "-t", "5000", "Screen recording error", detail],
    );
}

/// `Recorder::signalExisting(cancel, &error)`: asks the running daemon to stop (SIGUSR1) or to
/// discard the recording (SIGUSR2). Without a live daemon the state file is removed and the
/// error is "No recording is active"; a failed signal is the system's message, with a
/// notification.
pub fn signal_existing(cancel: bool) -> Result<(), String> {
    let state = read_state();
    let pid = state.value("pid").to_variant_long_long();
    if !recorder_daemon_alive(pid) {
        qfile::remove(&state_file_path());
        return Err("No recording is active".into());
    }
    if kill(pid, if cancel { libc::SIGUSR2 } else { libc::SIGUSR1 }) != 0 {
        let error = strerror(errno());
        notify_stop_failure("Could not ask the recording process to stop. The video may be incomplete.");
        return Err(error);
    }
    Ok(())
}

/// What `Recorder::stopExisting` returns through its pointers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StopOutcome {
    /// The exit status for `omareel stop`: 0 done, 1 nothing was recording, 2 failed.
    pub code: i32,
    /// `*bundlePath`, set once the daemon has gone: the bundle, or empty after a discard.
    pub bundle_path: Option<String>,
    /// `*error`: empty on success, else why it failed.
    pub error: String,
}

/// `QFileInfo(path).size()`.
fn file_size(path: &str) -> i64 {
    FileInfo::new(path).size()
}

/// `Recorder::stopExisting(cancel, &bundlePath, &error)`: signals the daemon and waits up to 20
/// seconds (800 checks 25 ms apart) for it to go or for its state file to disappear, then says
/// whether the bundle was finalized (`screen.mp4` and `capture.json` not empty, `input.jsonl` a
/// file) or, after a discard, removed. A daemon that does not stop is killed with its ffmpeg and
/// `__record-gsr` descendants, and the cursor setting it held is put back (up to a second for it
/// to die first).
pub fn stop_existing(cancel: bool) -> StopOutcome {
    let state = read_state();
    let pid = state.value("pid").to_variant_long_long();
    if !recorder_daemon_alive(pid) {
        qfile::remove(&state_file_path());
        return StopOutcome { code: 1, bundle_path: None, error: "No recording is active".into() };
    }
    let bundle = state.value("bundle").to_string_or("");
    if let Err(error) = signal_existing(cancel) {
        return StopOutcome { code: 2, bundle_path: None, error };
    }
    for _ in 0..800 {
        if !recorder_daemon_alive(pid) || !qdir::file_exists(&state_file_path()) {
            let bundle_path = Some(if cancel { String::new() } else { bundle.clone() });
            if cancel {
                let discarded = !qdir::file_exists(&bundle);
                return StopOutcome {
                    code: if discarded { 0 } else { 2 },
                    bundle_path,
                    error: if discarded { String::new() } else { "Recording bundle could not be discarded".into() },
                };
            }
            let complete = file_size(&format!("{bundle}/screen.mp4")) > 0
                && file_size(&format!("{bundle}/capture.json")) > 0
                && FileInfo::new(format!("{bundle}/input.jsonl")).is_file();
            return StopOutcome {
                code: if complete { 0 } else { 2 },
                bundle_path,
                error: if complete { String::new() } else { "Recording could not be finalized".into() },
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    for child in process_descendants(pid) {
        let Ok(command) = qfile::read_all(format!("/proc/{child}/cmdline")) else { continue };
        let contains = |needle: &[u8]| command.windows(needle.len()).any(|window| window == needle);
        if contains(b"ffmpeg") || contains(b"__record-gsr") {
            kill(child, libc::SIGKILL);
        }
    }
    kill(pid, libc::SIGKILL);
    qfile::remove(&state_file_path());
    // The killed daemon never dropped its hold on hardware cursors.
    for _ in 0..40 {
        if !process_alive(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    crate::capture_exclusion::restore_abandoned_hardware_cursor();
    let timeout_message =
        "Recorder did not stop within 20 seconds; force-killed the recorder and capture processes";
    notify_stop_failure(&format!("{timeout_message}. The video may be incomplete."));
    StopOutcome { code: 2, bundle_path: None, error: timeout_message.into() }
}

/// `writeLastError(detail)`: the detail and a newline into `last-error.txt`, replaced whole.
pub fn write_last_error(detail: &str) {
    qdir::mkpath(&FileInfo::new(last_error_file_path()).absolute_path());
    let mut file = SaveFile::new(last_error_file_path());
    if !file.open() {
        return;
    }
    file.write(detail.as_bytes());
    file.write(b"\n");
    file.commit();
}

/// `readLastError()`: `last-error.txt` as text (a `QFile` in text mode drops every `\r`),
/// trimmed; empty when it can't be read.
pub fn read_last_error() -> String {
    let Ok(mut bytes) = qfile::read_all(last_error_file_path()) else { return String::new() };
    bytes.retain(|&b| b != b'\r');
    qstring::trimmed(&qstring::from_utf8(&bytes)).to_owned()
}
