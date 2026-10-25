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
/// start or did not finish in time. As in [`Process::start`], a program without a "/" is looked
/// up with `QStandardPaths::findExecutable` and started with the path found as `argv[0]`; one
/// not found does not start.
pub fn run(program: &str, args: &[&str], timeout_ms: i64) -> Option<Finished> {
    let resolved = if program.contains('/') { program.to_string() } else { qstandardpaths::find_executable(program) };
    if resolved.is_empty() {
        return None;
    }
    let mut child = Command::new(&resolved)
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

/// `QProcess::ProcessChannelMode`, as far as omareel uses it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelMode {
    /// Standard output and error each in a channel of their own.
    #[default]
    Separate,
    /// The child's standard error goes wherever its standard output goes.
    Merged,
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
    channel_mode: ChannelMode,
    /// `setStandardOutputFile`: the file the child's standard output is written to (truncated).
    standard_output_file: Option<String>,
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

/// A close-on-exec pipe: (read end, write end).
fn pipe() -> Option<(OwnedFd, OwnedFd)> {
    let mut fds = [-1; 2];
    // SAFETY: pipe2 writes two descriptors into `fds`.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return None;
    }
    // SAFETY: both are new descriptors nothing else owns.
    Some(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// `qt_safe_open(file, O_WRONLY | O_CREAT | O_TRUNC, 0666)` for an output redirection.
fn open_output_file(path: &str) -> Option<OwnedFd> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: a valid NUL-terminated path.
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC, 0o666) };
    // SAFETY: a non-negative result is a descriptor nothing else owns.
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
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
            channel_mode: ChannelMode::Separate,
            standard_output_file: None,
        }
    }

    /// `QProcess::setProcessChannelMode`, for the next `start`.
    pub fn set_process_channel_mode(&mut self, mode: ChannelMode) {
        self.channel_mode = mode;
    }

    /// `QProcess::setStandardOutputFile(path)` (truncating), for the next `start`. With merged
    /// channels the standard error goes to the file too.
    pub fn set_standard_output_file(&mut self, path: &str) {
        self.standard_output_file = Some(path.to_string());
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
        // QProcessPrivate::openChannels, before the program is looked up: a redirection file is
        // created (truncated, 0666 less the umask) even when the program then fails to start.
        let mut output_file = None;
        if let Some(path) = &self.standard_output_file {
            match open_output_file(path) {
                Some(fd) => output_file = Some(fd),
                None => {
                    // Qt's message for an output file that can't be opened says "input" too.
                    self.state = ProcessState::NotRunning;
                    self.error = ProcessError::FailedToStart;
                    self.error_string = "Could not open input redirection for reading".into();
                    return;
                }
            }
        }
        // QChildProcess::resolveExecutable: a name without "/" goes through findExecutable, and
        // one it does not find is exec'd as "", which fails with ENOENT.
        let resolved =
            if program.contains('/') { program.to_string() } else { qstandardpaths::find_executable(program) };
        if resolved.is_empty() {
            self.fail_to_start(libc::ENOENT);
            return;
        }
        let mut command = Command::new(&resolved);
        command.args(arguments.iter().map(AsRef::as_ref)).stdin(Stdio::piped());
        // commitChannels: standard output to the file or a pipe, and with merged channels the
        // standard error dup'ed from it.
        let mut merged_reader = None;
        match (output_file, self.channel_mode) {
            (Some(fd), ChannelMode::Merged) => {
                let Ok(copy) = fd.try_clone() else {
                    self.fail_to_start(last_errno());
                    return;
                };
                command.stdout(Stdio::from(fd)).stderr(Stdio::from(copy));
            }
            (Some(fd), ChannelMode::Separate) => {
                command.stdout(Stdio::from(fd)).stderr(Stdio::piped());
            }
            (None, ChannelMode::Merged) => {
                let Some((reader, writer)) = pipe() else {
                    self.fail_to_start(last_errno());
                    return;
                };
                let Ok(copy) = writer.try_clone() else {
                    self.fail_to_start(last_errno());
                    return;
                };
                command.stdout(Stdio::from(writer)).stderr(Stdio::from(copy));
                merged_reader = Some(reader);
            }
            (None, ChannelMode::Separate) => {
                command.stdout(Stdio::piped()).stderr(Stdio::piped());
            }
        }
        let spawned = command.spawn();
        // The parent's copies of the child's ends close here (the Command owns them).
        drop(command);
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
        self.stdout = take(child.stdout.take().map(OwnedFd::from).or(merged_reader));
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

    /// `QProcess::terminate`: SIGTERM to a child not yet reaped.
    pub fn terminate(&mut self) {
        if self.state == ProcessState::Running {
            if let Some(child) = &self.child {
                // SAFETY: the child has not been reaped, so its pid is still ours.
                unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
            }
        }
    }

    /// `QProcess::processId`: the child's pid while it runs, else 0.
    pub fn process_id(&self) -> i64 {
        match (&self.child, self.state) {
            (Some(child), ProcessState::Running) => i64::from(child.id()),
            _ => 0,
        }
    }

    /// `QIODevice::readAll` on a process: the current read channel, the standard output (which
    /// holds the standard error too with merged channels).
    pub fn read_all(&mut self) -> Vec<u8> {
        self.read_all_standard_output()
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

/// Where `start_detached` sends the child's standard output and error: `None` inherits this
/// process's channel, as `QProcess::startDetached` does without a redirection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetachedChannels<'a> {
    /// `setStandardOutputFile(path)` (truncating).
    pub standard_output_file: Option<&'a str>,
    /// `setStandardErrorFile(path)` (truncating).
    pub standard_error_file: Option<&'a str>,
}

/// What the two children of [`start_detached`] read: everything is made before they run, in
/// memory they share with the parent while it waits.
#[repr(C)]
struct DetachedSpawn {
    path: *const libc::c_char,
    argv: *const *const libc::c_char,
    stdout_fd: RawFd,
    stderr_fd: RawFd,
    started_read_fd: RawFd,
    started_write_fd: RawFd,
    pid_read_fd: RawFd,
    pid_write_fd: RawFd,
    old_mask: libc::sigset_t,
    grandchild_stack: *mut libc::c_void,
}

/// Reports an errno on the started pipe and exits (`failChildProcess`).
///
/// # Safety
/// Only for the children of [`start_detached`].
unsafe fn fail_child(spawn: &DetachedSpawn) -> ! {
    // SAFETY: async-signal-safe calls on the prepared descriptors.
    unsafe {
        let code: libc::c_int = *libc::__errno_location();
        libc::write(spawn.started_write_fd, (&code as *const libc::c_int).cast(), size_of::<libc::c_int>());
        libc::_exit(-1)
    }
}

/// The grandchild (`QChildProcess::startProcess`): the redirections, SIGPIPE back to its
/// default, the parent's signal mask, then the program.
extern "C" fn detached_grandchild(argument: *mut libc::c_void) -> libc::c_int {
    // SAFETY: the parent is suspended (CLONE_VFORK) and keeps `DetachedSpawn` alive; only
    // async-signal-safe calls follow.
    unsafe {
        let spawn = &*(argument as *const DetachedSpawn);
        if spawn.stdout_fd >= 0 {
            libc::dup2(spawn.stdout_fd, libc::STDOUT_FILENO);
        }
        if spawn.stderr_fd >= 0 {
            libc::dup2(spawn.stderr_fd, libc::STDERR_FILENO);
        }
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        libc::sigprocmask(libc::SIG_SETMASK, &spawn.old_mask, std::ptr::null_mut());
        libc::execv(spawn.path, spawn.argv);
        fail_child(spawn)
    }
}

/// The intermediate child: a session of its own, the grandchild, its pid on the pid pipe.
extern "C" fn detached_intermediate(argument: *mut libc::c_void) -> libc::c_int {
    // SAFETY: as for the grandchild; this child is suspended in turn until the grandchild has
    // exec'd or exited.
    unsafe {
        let spawn = &*(argument as *const DetachedSpawn);
        libc::setsid();
        libc::close(spawn.started_read_fd);
        libc::close(spawn.pid_read_fd);
        let grandchild = libc::clone(
            detached_grandchild,
            spawn.grandchild_stack,
            libc::CLONE_VM | libc::CLONE_VFORK | libc::SIGCHLD,
            argument,
        );
        if grandchild < 0 {
            fail_child(spawn);
        }
        libc::write(spawn.pid_write_fd, (&grandchild as *const libc::pid_t).cast(), size_of::<libc::pid_t>());
        libc::_exit(0)
    }
}

/// `QProcess::startDetached(&pid)` as Qt 6.11 runs it on Linux: the program (looked up with
/// `findExecutable` unless it contains a "/") is started by a grandchild, so it is neither
/// this process's child nor waited for. The intermediate child calls `setsid` before starting
/// it, so the program runs in a session of its own without a controlling terminal, and the
/// intermediate is reaped before this returns. The program inherits this process's standard
/// channels and environment unless redirected, starts with SIGPIPE at its default and this
/// thread's signal mask, and gets none of this process's close-on-exec descriptors. As Qt's
/// children are made with vfork, these share this process's memory (on stacks of their own,
/// with every signal blocked) until each has exec'd, so starting one costs no copy of the
/// address space. Returns the program's pid once it has exec'd, or `None` when it could not be
/// started (not found, a redirection that can't be opened, a failed clone or exec).
pub fn start_detached<S: AsRef<str>>(program: &str, arguments: &[S], channels: DetachedChannels) -> Option<i64> {
    use std::ffi::CString;
    // Everything the children need is made here: they may only make async-signal-safe calls.
    let resolved = if program.contains('/') { program.to_string() } else { qstandardpaths::find_executable(program) };
    let path = CString::new(resolved.as_bytes()).ok()?;
    let mut argv_strings = Vec::with_capacity(arguments.len() + 1);
    argv_strings.push(path.clone());
    for argument in arguments {
        argv_strings.push(CString::new(argument.as_ref().as_bytes()).ok()?);
    }
    let mut argv: Vec<*const libc::c_char> = argv_strings.iter().map(|s| s.as_ptr()).collect();
    argv.push(std::ptr::null());
    // QProcessPrivate::openChannelsForDetached: the redirection files are opened here.
    let open_file = |path: Option<&str>| -> Result<Option<OwnedFd>, ()> {
        match path {
            None => Ok(None),
            Some(path) => open_output_file(path).map(Some).ok_or(()),
        }
    };
    let stdout_file = open_file(channels.standard_output_file).ok()?;
    let stderr_file = open_file(channels.standard_error_file).ok()?;
    let (started_read, started_write) = pipe()?;
    let (pid_read, pid_write) = pipe()?;
    const STACK: usize = 64 * 1024;
    // u128 keeps the stacks 16-byte aligned; they grow down from their ends.
    let mut stacks = vec![0u128; 2 * STACK / size_of::<u128>()];
    let (intermediate_stack, grandchild_stack) = stacks.split_at_mut(STACK / size_of::<u128>());
    let top = |stack: &mut [u128]| stack.as_mut_ptr_range().end.cast::<libc::c_void>();
    let mut spawn = DetachedSpawn {
        path: path.as_ptr(),
        argv: argv.as_ptr(),
        stdout_fd: stdout_file.as_ref().map_or(-1, AsRawFd::as_raw_fd),
        stderr_fd: stderr_file.as_ref().map_or(-1, AsRawFd::as_raw_fd),
        started_read_fd: started_read.as_raw_fd(),
        started_write_fd: started_write.as_raw_fd(),
        pid_read_fd: pid_read.as_raw_fd(),
        pid_write_fd: pid_write.as_raw_fd(),
        // SAFETY: an all-zero sigset_t is a valid value to be overwritten.
        old_mask: unsafe { std::mem::zeroed() },
        grandchild_stack: top(grandchild_stack),
    };
    // SAFETY: every signal is blocked on this thread while the children share its memory (Qt's
    // maybeBlockSignals); the grandchild restores the old mask before its exec.
    let intermediate = unsafe {
        let mut all: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut all);
        libc::pthread_sigmask(libc::SIG_SETMASK, &all, &mut spawn.old_mask);
        let pid = libc::clone(
            detached_intermediate,
            top(intermediate_stack),
            libc::CLONE_VM | libc::CLONE_VFORK | libc::SIGCHLD,
            (&mut spawn as *mut DetachedSpawn).cast(),
        );
        libc::pthread_sigmask(libc::SIG_SETMASK, &spawn.old_mask, std::ptr::null_mut());
        pid
    };
    drop(stdout_file);
    drop(stderr_file);
    drop(started_write);
    drop(pid_write);
    if intermediate < 0 {
        return None;
    }
    // The children have finished with the pipe: the intermediate by exiting, the grandchild by
    // exec'ing. Anything read is an error report.
    let mut report = [0u8; size_of::<libc::c_int>()];
    let started = loop {
        // SAFETY: reads into a local buffer of its size.
        let r = unsafe { libc::read(started_read.as_raw_fd(), report.as_mut_ptr().cast(), report.len()) };
        if r < 0 && last_errno() == libc::EINTR {
            continue;
        }
        break r == 0;
    };
    let mut status = 0;
    loop {
        // SAFETY: reaps the intermediate child, which is ours.
        let r = unsafe { libc::waitpid(intermediate, &mut status, 0) };
        if r < 0 && last_errno() == libc::EINTR {
            continue;
        }
        break;
    }
    if !started {
        return None;
    }
    let mut pid: libc::pid_t = 0;
    // SAFETY: reads one pid_t into a local.
    let r = unsafe { libc::read(pid_read.as_raw_fd(), (&mut pid as *mut libc::pid_t).cast(), size_of::<libc::pid_t>()) };
    // Qt reports 0 when the pid could not be read ("this shouldn't happen").
    Some(if r == size_of::<libc::pid_t>() as isize { i64::from(pid) } else { 0 })
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
    fn merges_channels_and_redirects_to_a_file() {
        // Merged: standard error lands in the standard output buffer, in the order written.
        let mut p = Process::new();
        p.set_process_channel_mode(ChannelMode::Merged);
        p.start("sh", &["-c", "echo out; echo err >&2; echo more"]);
        assert!(p.process_id() > 0);
        assert!(p.wait_for_finished(5000));
        assert_eq!(p.process_id(), 0);
        assert_eq!(p.read_all(), b"out\nerr\nmore\n");
        assert_eq!(p.read_all_standard_error(), b"");

        // A redirection file is truncated and gets both channels when they are merged; it is
        // created even when the program then fails to start.
        let dir = std::env::temp_dir().join(format!("omareel-qprocess-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log");
        let log = log.to_str().unwrap();
        std::fs::write(log, "old contents that are longer").unwrap();
        let mut p = Process::new();
        p.set_process_channel_mode(ChannelMode::Merged);
        p.set_standard_output_file(log);
        p.start("sh", &["-c", "echo out; echo err >&2"]);
        assert!(p.wait_for_finished(5000));
        assert_eq!(p.read_all(), b"");
        assert_eq!(std::fs::read(log).unwrap(), b"out\nerr\n");
        let mut p = Process::new();
        p.set_standard_output_file(log);
        p.start("omareel-no-such-program", &[] as &[&str]);
        assert_eq!(p.error(), ProcessError::FailedToStart);
        assert_eq!(std::fs::read(log).unwrap(), b"");
        let mut p = Process::new();
        p.set_standard_output_file(&format!("{log}/missing/log"));
        p.start("sh", &["-c", "true"]);
        assert!(!p.wait_for_started(1000));
        assert_eq!(p.error_string(), "Could not open input redirection for reading");

        // terminate: SIGTERM, a crash exit with the signal as the code.
        let mut p = Process::new();
        p.start("sleep", &["10"]);
        p.terminate();
        assert!(p.wait_for_finished(2000));
        assert_eq!((p.exit_status(), p.exit_code()), (ExitStatus::CrashExit, libc::SIGTERM));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn starts_detached_in_its_own_session() {
        let dir = std::env::temp_dir().join(format!("omareel-detached-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (out, err) = (dir.join("out"), dir.join("err"));
        let (out, err) = (out.to_str().unwrap(), err.to_str().unwrap());
        // The grandchild says its pid and its session, then writes to its standard error.
        let pid = start_detached(
            "sh",
            &["-c", "echo $$ $(cut -d' ' -f6 /proc/$$/stat); echo err >&2"],
            DetachedChannels { standard_output_file: Some(out), standard_error_file: Some(err) },
        )
        .expect("started");
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::read_to_string(err).unwrap_or_default() != "err\n" && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let text = std::fs::read_to_string(out).unwrap();
        let fields: Vec<&str> = text.split_whitespace().collect();
        assert_eq!(fields.first().copied(), Some(pid.to_string().as_str()), "{text:?}");
        // A session of its own, led by the intermediate child rather than the program.
        // SAFETY: getsid on this process.
        let own_session = unsafe { libc::getsid(0) }.to_string();
        assert!(fields.get(1).is_some_and(|session| *session != pid.to_string() && *session != own_session));
        // Not this process's child.
        // SAFETY: waitpid with WNOHANG on a pid that is not a child.
        assert_eq!(unsafe { libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), libc::WNOHANG) }, -1);
        assert_eq!(start_detached("omareel-no-such-program", &[] as &[&str], DetachedChannels::default()), None);
        let missing = format!("{out}/missing/log");
        let channels = DetachedChannels { standard_output_file: Some(&missing), standard_error_file: None };
        assert_eq!(start_detached("true", &[] as &[&str], channels), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn captures_output_and_exit_codes() {
        let done = run("sh", &["-c", "echo out; echo err >&2; exit 3"], 5000).unwrap();
        assert_eq!((done.exit_code, done.stdout.as_slice(), done.stderr.as_slice()), (3, &b"out\n"[..], &b"err\n"[..]));
        assert!(!done.succeeded());
        assert!(run("sh", &["-c", "true"], 5000).unwrap().succeeded());
        assert_eq!(run("/nonexistent/program", &[], 5000), None);
        assert_eq!(run("omareel-no-such-program", &[], 5000), None);
        // argv[0] is the path findExecutable found, as QProcess passes it, not the name asked for.
        let cat = qstandardpaths::find_executable("cat");
        assert!(cat.starts_with('/'), "{cat}");
        let done = run("cat", &["/proc/self/cmdline"], 5000).unwrap();
        assert_eq!(done.stdout, format!("{cat}\0/proc/self/cmdline\0").into_bytes());
        let started = Instant::now();
        assert_eq!(run("sleep", &["5"], 100), None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
