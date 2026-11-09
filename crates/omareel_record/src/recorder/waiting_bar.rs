//! A recording bar started ahead of the recording, so it shows the moment one starts. The
//! recorder would otherwise start the bar (and its countdown) once the region is picked, and the
//! user waited for Qt to start before either appeared.
//!
//! `omareel record` starts one before it shows the region picker, and the launcher when it
//! opens: `<omareel> __record-bar --wait <starter pid>` (main.cpp). The bar starts Qt and
//! compiles its QML, writes its pid to `waiting-bar.pid` beside the recorder's state file as soon
//! as it can take the signal, and waits. The recorder adopts it with SIGUSR1 (sent with
//! `sigqueue`, its value the bar's flags: [`HIDDEN`]) once the state file is written, and starts
//! a new bar only when none is waiting. A bar nobody adopts quits a little after whatever started
//! it has gone. `OMAREEL_BAR_WAIT=0` turns this off.

use super::state::state_directory;
use super::environment_variable;
use omareel_core::compat::qcoreapplication::application_pid;
use omareel_core::compat::qprocess::{self, DetachedChannels};

/// The flag a hidden bar (`--no-bar` with a webcam or countdown) is adopted with.
pub const HIDDEN: i32 = 1;

/// `waiting-bar.pid` beside the state file.
pub fn path() -> String {
    state_directory() + "/waiting-bar.pid"
}

fn enabled() -> bool {
    environment_variable("OMAREEL_BAR_WAIT") != "0"
}

/// The pid in [`path`] when it names a live `<application> __record-bar --wait` process.
pub fn waiting(application: &str) -> Option<i64> {
    let pid: i64 = std::fs::read_to_string(path()).ok()?.trim().parse().ok()?;
    let command = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let arguments: Vec<&[u8]> = command.split(|byte| *byte == 0).collect();
    let same_program = std::fs::canonicalize(String::from_utf8_lossy(arguments.first()?).as_ref()).ok()
        == std::fs::canonicalize(application).ok();
    (same_program && arguments.get(1) == Some(&&b"__record-bar"[..]) && arguments.get(2) == Some(&&b"--wait"[..]))
        .then_some(pid)
}

/// Starts a waiting bar for this process, unless one is already waiting.
pub fn start(application: &str) {
    if !enabled() || waiting(application).is_some() {
        return;
    }
    let pid = application_pid().to_string();
    qprocess::start_detached(application, &["__record-bar", "--wait", pid.as_str()], DetachedChannels::default());
}

/// For the bar: it can take the signal now.
pub fn register() {
    std::fs::create_dir_all(state_directory()).ok();
    let temporary = format!("{}.{}", path(), application_pid());
    if std::fs::write(&temporary, application_pid().to_string()).is_ok() {
        let _ = std::fs::rename(&temporary, path());
    }
}

/// For the bar, as it stops waiting: the file goes if it still names this process.
pub fn unregister() {
    let mine = application_pid().to_string();
    if std::fs::read_to_string(path()).is_ok_and(|pid| pid.trim() == mine) {
        let _ = std::fs::remove_file(path());
    }
}

/// For the recorder: adopts the waiting bar with `flags`, true when it was signalled. The file
/// goes either way, so no second recording adopts the same bar.
pub fn adopt(application: &str, flags: i32) -> bool {
    if !enabled() {
        return false;
    }
    let Some(pid) = waiting(application) else { return false };
    let _ = std::fs::remove_file(path());
    let value = libc::sigval { sival_ptr: flags as usize as *mut libc::c_void };
    // SAFETY: sigqueue takes a pid, a signal and a value; the pid is the waiting bar's.
    unsafe { libc::sigqueue(pid as libc::pid_t, libc::SIGUSR1, value) == 0 }
}
