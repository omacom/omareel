//! Small process-level calls the launcher and the recording bar share: Qt's environment checks,
//! `kill`, this process's pid and the C++'s one-piece writes to stderr.

/// `qEnvironmentVariableIsSet(name)`.
pub fn env_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// `qEnvironmentVariableIsEmpty(name)`: unset or empty.
pub fn env_is_empty(name: &str) -> bool {
    std::env::var_os(name).is_none_or(|value| value.is_empty())
}

/// `qEnvironmentVariable(name) == QLatin1String(value)`.
pub fn env_equals(name: &str, value: &str) -> bool {
    std::env::var_os(name).is_some_and(|current| current == value)
}

/// `QCoreApplication::applicationPid()`.
pub fn application_pid() -> i64 {
    i64::from(std::process::id())
}

/// `::kill(pid_t(pid), signal)`, the pid truncated to `pid_t` as the C++ cast does.
pub fn kill(pid: i64, signal: i32) -> i32 {
    // SAFETY: kill takes any pid and signal and reports errors through its result.
    unsafe { libc::kill(pid as libc::pid_t, signal) }
}

/// `errno` after the last call.
pub fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Writes `text` to stderr in one piece, as `QTextStream(stderr) << ...` did.
pub fn print_error(text: &str) {
    use std::io::Write;
    let _ = std::io::stderr().write_all(text.as_bytes());
}

/// Appends a line to /tmp/omareel.log, the startup trace `OMAREEL_DEBUG=1` asks for (opened as
/// `QFile::open(WriteOnly | Append | Text)` does: created when missing, nothing when it cannot be).
pub fn append_debug_log(line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).open("/tmp/omareel.log") {
        let _ = file.write_all(line.as_bytes());
    }
}
