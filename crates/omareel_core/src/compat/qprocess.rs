//! `QProcess`, as Qt 6.11 runs it on Linux.
//!
//! [`run`] is the synchronous pattern (`start`, `waitForFinished(timeout)`, exit code and output)
//! for small helpers. [`Process`] is the object itself, for the exporter's long-running ffmpeg
//! children, whose details it keeps:
//! - The program is looked up with `QStandardPaths::findExecutable` unless it contains a "/", and
//!   started with that path as `argv[0]`; one not found fails to start with execve's ENOENT.
//! - The child gets three pipes (stdin stays open even for a reader), all close-on-exec, the
//!   parent's ends non-blocking. It starts with SIGPIPE at its default and no blocked signals.
//! - Output is only read inside the `wait_for_*` calls, into buffers `read` and the
//!   `read_all_*` calls take from; `write` only buffers, and the `wait_for_*` calls pass the buffer
//!   to the pipe as it takes it. Each wait polls the pipes and the child (a pidfd) together, and
//!   handles what is ready in the order Qt does, which decides which error a dying child gives.
//! - Writing ignores SIGPIPE for the whole process, as `qt_ignore_sigpipe` does, so a child
//!   that went away makes the write fail instead of killing the program.
//! - `kill` sends SIGKILL; the child is reaped by the wait that sees it exit. Dropping a running
//!   process warns as `~QProcess` does, kills it and waits up to 30 seconds.
//!
//! Like QProcess with its default channel mode, the child gets a stdin pipe that stays open
//! and both output channels are captured rather than shown. A child still running at the
//! timeout is killed, as destroying a running QProcess does.

use super::{qdir, qstandardpaths};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
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
    Some(Finished {
        exit_code: status.code().or(status.signal()).unwrap_or(0),
        crashed: status.signal().is_some(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// `QProcess::ProcessState`. Starting is never seen: the child has exec'd (or failed to) when
/// `start` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    NotRunning,
    Running,
}

/// `QProcess::ProcessError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessError {
    FailedToStart,
    Crashed,
    Timedout,
    ReadError,
    WriteError,
    UnknownError,
}

/// `QProcess::ExitStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    NormalExit,
    CrashExit,
}

/// `qt_ignore_sigpipe`: SIGPIPE ignored process-wide, once, before the first write to a child.
pub fn ignore_sigpipe() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: a zeroed sigaction with SIG_IGN is a valid argument.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_IGN;
            libc::sigaction(libc::SIGPIPE, &action, std::ptr::null_mut());
        }
    });
}

/// `QDeadlineTimer(msecs)`: negative is forever.
struct Deadline(Option<Instant>);

impl Deadline {
    fn new(msecs: i32) -> Deadline {
        Deadline((msecs >= 0).then(|| Instant::now() + Duration::from_millis(msecs as u64)))
    }

    fn remaining(&self) -> Option<Duration> {
        self.0.map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }
}

/// What one `QProcessPoller::poll` found, already put through `qt_pollfd_check` (the event
/// asked for, or a hang-up, error or invalid descriptor).
#[derive(Default)]
struct Ready {
    stdin: bool,
    stdout: bool,
    stderr: bool,
    child: bool,
}

/// `QProcess` with separate channels and no event loop, as the exporter's worker threads use it.
pub struct Process {
    program: String,
    child: Option<Child>,
    pidfd: Option<OwnedFd>,
    /// The exit status, once a wait has seen the child go (without a pidfd, `try_wait` reaps it).
    reaped: Option<std::process::ExitStatus>,
    stdin: Option<OwnedFd>,
    stdout: Option<OwnedFd>,
    stderr: Option<OwnedFd>,
    write_buffer: Vec<u8>,
    write_offset: usize,
    /// `stdinChannel.closed`: `close_write_channel` was asked for.
    write_channel_closed: bool,
    stdout_buffer: Vec<u8>,
    stdout_offset: usize,
    stderr_buffer: Vec<u8>,
    state: ProcessState,
    exit_code: i32,
    exit_status: ExitStatus,
    error: ProcessError,
    error_string: String,
}

impl Default for Process {
    fn default() -> Process {
        Process::new()
    }
}

fn set_nonblocking(fd: RawFd) {
    // SAFETY: `fd` is an open descriptor this process owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

fn pidfd_open(pid: u32) -> Option<OwnedFd> {
    // SAFETY: pidfd_open takes a pid and flags and returns a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    // SAFETY: a non-negative result is a descriptor nothing else owns.
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

impl Process {
    pub fn new() -> Process {
        Process {
            program: String::new(),
            child: None,
            pidfd: None,
            reaped: None,
            stdin: None,
            stdout: None,
            stderr: None,
            write_buffer: Vec::new(),
            write_offset: 0,
            write_channel_closed: false,
            stdout_buffer: Vec::new(),
            stdout_offset: 0,
            stderr_buffer: Vec::new(),
            state: ProcessState::NotRunning,
            exit_code: 0,
            exit_status: ExitStatus::NormalExit,
            error: ProcessError::UnknownError,
            error_string: String::new(),
        }
    }

    /// `QProcess::start(program, arguments)`. A failure to start leaves the state NotRunning with
    /// `FailedToStart` and Qt's "Child process set up failed: execve: ..." message.
    pub fn start<S: AsRef<str>>(&mut self, program: &str, arguments: &[S]) {
        if self.state != ProcessState::NotRunning {
            eprintln!("QProcess::start: Process is already running");
            return;
        }
        self.program = program.to_string();
        // QIODevice::open clears the buffers and the error string.
        self.write_buffer.clear();
        self.write_offset = 0;
        self.write_channel_closed = false;
        self.stdout_buffer.clear();
        self.stdout_offset = 0;
        self.stderr_buffer.clear();
        self.exit_code = 0;
        self.exit_status = ExitStatus::NormalExit;
        self.error = ProcessError::UnknownError;
        self.error_string.clear();
        self.reaped = None;
        // QChildProcess::resolveExecutable: a name without "/" goes through findExecutable, and
        // one it does not find is exec'd as "", which fails with ENOENT.
        let resolved =
            if program.contains('/') { program.to_string() } else { qstandardpaths::find_executable(program) };
        if resolved.is_empty() {
            self.fail_to_start(libc::ENOENT);
            return;
        }
        let spawned = Command::new(&resolved)
            .args(arguments.iter().map(AsRef::as_ref))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                self.fail_to_start(error.raw_os_error().unwrap_or(libc::ENOENT));
                return;
            }
        };
        let take = |fd: Option<OwnedFd>| {
            if let Some(fd) = &fd {
                set_nonblocking(fd.as_raw_fd());
            }
            fd
        };
        self.stdin = take(child.stdin.take().map(OwnedFd::from));
        self.stdout = take(child.stdout.take().map(OwnedFd::from));
        self.stderr = take(child.stderr.take().map(OwnedFd::from));
        self.pidfd = pidfd_open(child.id());
        self.child = Some(child);
        self.state = ProcessState::Running;
    }

    fn fail_to_start(&mut self, code: i32) {
        self.state = ProcessState::NotRunning;
        self.error = ProcessError::FailedToStart;
        self.error_string = format!("Child process set up failed: execve: {}", qdir::error_string(code));
    }

    /// `QProcess::waitForStarted`: whether the child is running.
    pub fn wait_for_started(&mut self, _msecs: i32) -> bool {
        self.state == ProcessState::Running
    }

    pub fn state(&self) -> ProcessState {
        self.state
    }

    pub fn error(&self) -> ProcessError {
        self.error
    }

    /// `QIODevice::errorString`: "Unknown error" when no error has a message.
    pub fn error_string(&self) -> String {
        if self.error_string.is_empty() { "Unknown error".into() } else { self.error_string.clone() }
    }

    /// The exit status, or the signal number for a child killed by one, as forkfd reports it.
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn exit_status(&self) -> ExitStatus {
        self.exit_status
    }

    /// `QProcess::kill`: SIGKILL to a child not yet reaped.
    pub fn kill(&mut self) {
        if self.state == ProcessState::Running && self.reaped.is_none() {
            if let Some(child) = &self.child {
                // SAFETY: the child has not been reaped, so its pid is still ours.
                unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGKILL) };
            }
        }
    }

    /// `QProcess::bytesAvailable` on the standard output: what has been read into the buffer.
    pub fn bytes_available(&self) -> usize {
        self.stdout_buffer.len() - self.stdout_offset
    }

    /// `QProcess::read` from the standard output buffer.
    pub fn read(&mut self, data: &mut [u8]) -> usize {
        let available = &self.stdout_buffer[self.stdout_offset..];
        let amount = available.len().min(data.len());
        data[..amount].copy_from_slice(&available[..amount]);
        self.stdout_offset += amount;
        if self.stdout_offset == self.stdout_buffer.len() {
            self.stdout_buffer.clear();
            self.stdout_offset = 0;
        }
        amount
    }

    /// `QProcess::readAllStandardOutput`.
    pub fn read_all_standard_output(&mut self) -> Vec<u8> {
        let rest = self.stdout_buffer.split_off(self.stdout_offset);
        self.stdout_buffer.clear();
        self.stdout_offset = 0;
        rest
    }

    /// `QProcess::readAllStandardError`.
    pub fn read_all_standard_error(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.stderr_buffer)
    }

    /// `QProcess::write`: buffers everything, unless the write channel is being closed (0).
    pub fn write(&mut self, data: &[u8]) -> i64 {
        if self.write_channel_closed {
            return 0;
        }
        if self.write_offset == self.write_buffer.len() {
            self.write_buffer.clear();
            self.write_offset = 0;
        }
        self.write_buffer.extend_from_slice(data);
        data.len() as i64
    }

    /// `QProcess::bytesToWrite`: what the write buffer still holds.
    pub fn bytes_to_write(&self) -> usize {
        self.write_buffer.len() - self.write_offset
    }

    /// `QProcess::closeWriteChannel`: the pipe closes once the buffer has been written.
    pub fn close_write_channel(&mut self) {
        self.write_channel_closed = true;
        if self.bytes_to_write() == 0 {
            self.stdin = None;
        }
    }

    /// `QProcessPoller::poll`: the stdin pipe (only while there is something to write), both
    /// output pipes and the child. `None` when the deadline passed, `Err` when poll failed.
    fn poll(&mut self, deadline: &Deadline) -> Result<Option<Ready>, ()> {
        let fd = |fd: &Option<OwnedFd>| fd.as_ref().map_or(-1, AsRawFd::as_raw_fd);
        let stdin = if self.bytes_to_write() > 0 { fd(&self.stdin) } else { -1 };
        let mut fds = [
            libc::pollfd { fd: stdin, events: libc::POLLOUT, revents: 0 },
            libc::pollfd { fd: fd(&self.stdout), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: fd(&self.stderr), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: fd(&self.pidfd), events: libc::POLLIN, revents: 0 },
        ];
        let child_known = self.child.is_some() && self.reaped.is_none();
        // Without a pidfd, a child `try_wait` reaped in an earlier poll stays ready, as a
        // pidfd would, until a wait gets to it.
        let already_reaped = self.pidfd.is_none() && self.reaped.is_some();
        loop {
            let mut remaining = if already_reaped { Some(Duration::ZERO) } else { deadline.remaining() };
            // Without a pidfd the child is checked between short polls.
            if self.pidfd.is_none() && child_known {
                remaining = Some(remaining.map_or(Duration::from_millis(10), |r| r.min(Duration::from_millis(10))));
            }
            let timeout = remaining.map(|r| libc::timespec {
                tv_sec: r.as_secs() as libc::time_t,
                tv_nsec: r.subsec_nanos() as libc::c_long,
            });
            let timeout_ptr = timeout.as_ref().map_or(std::ptr::null(), |t| t as *const libc::timespec);
            // SAFETY: `fds` holds four pollfds; the timeout is null or points at a timespec.
            let result = unsafe { libc::ppoll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ptr, std::ptr::null()) };
            if result < 0 {
                if last_errno() == libc::EINTR {
                    continue;
                }
                return Err(());
            }
            let mut child = already_reaped;
            if self.pidfd.is_none() && child_known {
                if let Some(Ok(Some(status))) = self.child.as_mut().map(Child::try_wait) {
                    self.reaped = Some(status);
                    child = true;
                }
            }
            if result == 0 && !child {
                if deadline.remaining().is_some_and(|r| r.is_zero()) {
                    return Ok(None);
                }
                continue;
            }
            let check = |pfd: &libc::pollfd, events: libc::c_short| {
                pfd.fd >= 0 && pfd.revents & (events | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
            };
            return Ok(Some(Ready {
                stdin: check(&fds[0], libc::POLLOUT),
                stdout: check(&fds[1], libc::POLLIN),
                stderr: check(&fds[2], libc::POLLIN),
                child: child || check(&fds[3], libc::POLLIN),
            }));
        }
    }

    /// `QProcessPrivate::tryReadFromChannel`: reads what the pipe holds (at least one byte is
    /// tried) into the channel's buffer; true when that was the standard output, the current
    /// read channel. End of file closes the pipe.
    fn read_from_channel(&mut self, stdout: bool) -> bool {
        let pipe = if stdout { &self.stdout } else { &self.stderr };
        let Some(fd) = pipe.as_ref().map(AsRawFd::as_raw_fd) else { return false };
        let mut available: libc::c_int = 0;
        // SAFETY: FIONREAD writes an int.
        if unsafe { libc::ioctl(fd, libc::FIONREAD, &mut available) } < 0 || available <= 0 {
            available = 1;
        }
        let buffer = if stdout { &mut self.stdout_buffer } else { &mut self.stderr_buffer };
        let old = buffer.len();
        buffer.resize(old + available as usize, 0);
        let read = loop {
            // SAFETY: the buffer has `available` bytes reserved at `old`.
            let r = unsafe { libc::read(fd, buffer[old..].as_mut_ptr().cast(), available as usize) };
            if r < 0 && last_errno() == libc::EINTR {
                continue;
            }
            break r;
        };
        buffer.truncate(old + read.max(0) as usize);
        if read < 0 {
            if last_errno() != libc::EAGAIN {
                self.error = ProcessError::ReadError;
                self.error_string = "Error reading from process".into();
            }
            return false;
        }
        if read == 0 {
            if stdout {
                self.stdout = None;
            } else {
                self.stderr = None;
            }
            return false;
        }
        stdout
    }

    /// `QProcessPrivate::_q_canWrite` and `writeToStdin`: one write of what the buffer holds.
    /// A full pipe is not an error; any other failure closes the pipe with `WriteError`.
    fn can_write(&mut self) -> bool {
        if self.bytes_to_write() == 0 {
            return false;
        }
        let Some(fd) = self.stdin.as_ref().map(AsRawFd::as_raw_fd) else { return false };
        ignore_sigpipe();
        let data = &self.write_buffer[self.write_offset..];
        let written = loop {
            // SAFETY: `data` is valid for its length.
            let r = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
            if r < 0 && last_errno() == libc::EINTR {
                continue;
            }
            break r;
        };
        if written < 0 {
            if last_errno() == libc::EAGAIN {
                return true;
            }
            self.stdin = None;
            self.error = ProcessError::WriteError;
            self.error_string = "Error writing to process".into();
            return false;
        }
        self.write_offset += written as usize;
        if self.bytes_to_write() == 0 && self.write_channel_closed {
            self.stdin = None;
        }
        true
    }

    /// `QProcessPrivate::processFinished`: reaps the child and closes every pipe.
    fn process_finished(&mut self) {
        let status = match self.reaped.take() {
            Some(status) => Some(status),
            None => self.child.as_mut().and_then(|child| child.wait().ok()),
        };
        if let Some(status) = status {
            self.exit_code = status.code().or(status.signal()).unwrap_or(0);
            self.exit_status = if status.code().is_some() { ExitStatus::NormalExit } else { ExitStatus::CrashExit };
        }
        self.cleanup();
        if self.exit_status == ExitStatus::CrashExit {
            self.error = ProcessError::Crashed;
            self.error_string = "Process crashed".into();
        }
    }

    /// `QProcessPrivate::cleanup`.
    fn cleanup(&mut self) {
        self.state = ProcessState::NotRunning;
        self.stdin = None;
        self.stdout = None;
        self.stderr = None;
        self.pidfd = None;
        self.child = None;
    }

    fn timed_out(&mut self) {
        self.error = ProcessError::Timedout;
        self.error_string = "Process operation timed out".into();
    }

    /// `QProcess::waitForReadyRead(msecs)`: true once new standard output is buffered; false at
    /// the deadline, or once the child has gone and its output is drained.
    pub fn wait_for_ready_read(&mut self, msecs: i32) -> bool {
        if self.state == ProcessState::NotRunning {
            return false;
        }
        let deadline = Deadline::new(msecs);
        loop {
            let ready = match self.poll(&deadline) {
                Err(()) => return false,
                Ok(None) => {
                    self.timed_out();
                    return false;
                }
                Ok(Some(ready)) => ready,
            };
            let mut ready_read = false;
            if ready.stdout && self.read_from_channel(true) {
                ready_read = true;
            }
            if ready.stderr && self.read_from_channel(false) {
                ready_read = true;
            }
            if ready_read {
                return true;
            }
            if ready.stdin {
                self.can_write();
            }
            if self.state == ProcessState::NotRunning {
                return false;
            }
            // After the pipes, so a dead child's output is read before it is reaped.
            if ready.child {
                self.process_finished();
                return false;
            }
        }
    }

    /// `QProcess::waitForBytesWritten(msecs)`: one write once the pipe takes more (true), false
    /// with nothing to write, at the deadline, on a write error or when the child goes.
    pub fn wait_for_bytes_written(&mut self, msecs: i32) -> bool {
        if self.state == ProcessState::NotRunning {
            return false;
        }
        let deadline = Deadline::new(msecs);
        while self.bytes_to_write() > 0 {
            let ready = match self.poll(&deadline) {
                Err(()) => break,
                Ok(None) => {
                    self.timed_out();
                    return false;
                }
                Ok(Some(ready)) => ready,
            };
            if ready.stdin {
                return self.can_write();
            }
            if ready.stdout {
                self.read_from_channel(true);
            }
            if ready.stderr {
                self.read_from_channel(false);
            }
            if self.state == ProcessState::NotRunning {
                return false;
            }
            if ready.child {
                self.process_finished();
                return false;
            }
        }
        false
    }

    /// `QProcess::waitForFinished(msecs)` (Qt's default is 30000): true once the child has
    /// exited, false at the deadline or when it was not running.
    pub fn wait_for_finished(&mut self, msecs: i32) -> bool {
        if self.state == ProcessState::NotRunning {
            return false;
        }
        let deadline = Deadline::new(msecs);
        loop {
            let ready = match self.poll(&deadline) {
                Err(()) => return false,
                Ok(None) => {
                    self.timed_out();
                    return false;
                }
                Ok(Some(ready)) => ready,
            };
            if ready.stdin {
                self.can_write();
            }
            if ready.stdout {
                self.read_from_channel(true);
            }
            if ready.stderr {
                self.read_from_channel(false);
            }
            if self.state == ProcessState::NotRunning {
                return true;
            }
            if ready.child {
                self.process_finished();
                return true;
            }
        }
    }
}

impl Drop for Process {
    /// `~QProcess`: a child still running is reported, killed and waited for.
    fn drop(&mut self) {
        if self.state != ProcessState::NotRunning {
            eprintln!("QProcess: Destroyed while process (\"{}\") is still running.", self.program);
            self.kill();
            self.wait_for_finished(30000);
        }
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_reads_writes_and_reports_exits() {
        // Echoes its input, then says goodbye on stderr and exits 3.
        let mut p = Process::new();
        p.start("sh", &["-c", "cat; echo bye >&2; exit 3"]);
        assert!(p.wait_for_started(1000));
        assert_eq!(p.write(b"hello\n"), 6);
        assert_eq!(p.bytes_to_write(), 6);
        while p.bytes_to_write() > 0 {
            assert!(p.wait_for_bytes_written(1000));
        }
        while p.bytes_available() < 6 {
            assert!(p.wait_for_ready_read(1000));
        }
        let mut line = [0u8; 6];
        assert_eq!(p.read(&mut line), 6);
        assert_eq!(&line, b"hello\n");
        p.close_write_channel();
        assert_eq!(p.write(b"late"), 0);
        assert!(p.wait_for_finished(2000));
        assert_eq!((p.state(), p.exit_status(), p.exit_code()), (ProcessState::NotRunning, ExitStatus::NormalExit, 3));
        assert_eq!(p.read_all_standard_error(), b"bye\n");
        assert!(!p.wait_for_finished(100));

        // Killed: a crash with the signal as its exit code.
        let mut p = Process::new();
        p.start("sleep", &["10"]);
        p.kill();
        assert!(p.wait_for_finished(2000));
        assert_eq!((p.exit_status(), p.exit_code(), p.error()), (ExitStatus::CrashExit, 9, ProcessError::Crashed));

        // Not found on PATH: execve's error, as Qt words it.
        let mut p = Process::new();
        p.start("omareel-no-such-program", &[] as &[&str]);
        assert!(!p.wait_for_started(1000));
        assert_eq!(p.error(), ProcessError::FailedToStart);
        assert_eq!(p.error_string(), "Child process set up failed: execve: No such file or directory");

        // A wait times out while the child runs.
        let mut p = Process::new();
        p.start("sleep", &["10"]);
        assert!(!p.wait_for_ready_read(50));
        assert_eq!(p.error(), ProcessError::Timedout);
    }

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
