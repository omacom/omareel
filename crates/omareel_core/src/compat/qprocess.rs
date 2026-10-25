//! `QProcess` run synchronously: `start`, `waitForFinished(timeout)`, then the exit code and
//! captured output.
//!
//! Like QProcess with its default channel mode, the child gets a stdin pipe that stays open
//! and both output channels are captured rather than shown. A child still running at the
//! timeout is killed, as destroying a running QProcess does.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Finished {
    /// The exit status, or the signal number when the child was killed by one.
    pub exit_code: i32,
    pub crashed: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Finished {
    /// `waitForFinished() && exitCode() == 0`, the check omareel makes everywhere.
    pub fn succeeded(&self) -> bool {
        !self.crashed && self.exit_code == 0
    }
}

/// Runs `program` and waits up to `timeout_ms` (negative: forever). `None` when it could not
/// start or did not finish in time.
pub fn run(program: &str, args: &[&str], timeout_ms: i64) -> Option<Finished> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Drain both pipes while waiting so a chatty child can't fill one and stall.
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let stdout = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = out.read_to_end(&mut bytes);
        bytes
    });
    let stderr = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = err.read_to_end(&mut bytes);
        bytes
    });
    let deadline = (timeout_ms >= 0).then(|| Instant::now() + Duration::from_millis(timeout_ms as u64));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if deadline.is_some_and(|d| Instant::now() >= d) => break None,
            Ok(None) => thread::sleep(Duration::from_millis(2)),
            Err(_) => break None,
        }
    };
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    use std::os::unix::process::ExitStatusExt;
    Some(Finished {
        exit_code: status.code().or(status.signal()).unwrap_or(0),
        crashed: status.signal().is_some(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_exit_codes() {
        let done = run("sh", &["-c", "echo out; echo err >&2; exit 3"], 5000).unwrap();
        assert_eq!((done.exit_code, done.stdout.as_slice(), done.stderr.as_slice()), (3, &b"out\n"[..], &b"err\n"[..]));
        assert!(!done.succeeded());
        assert!(run("sh", &["-c", "true"], 5000).unwrap().succeeded());
        assert_eq!(run("/nonexistent/program", &[], 5000), None);
        let started = Instant::now();
        assert_eq!(run("sleep", &["5"], 100), None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
