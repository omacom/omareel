//! The screen encoder process of `ScreenCapture.cpp` (`startEncoder`, `readEncoderError` and
//! the waiting part of `stopEncoder`): ffmpeg started with `posix_spawnp` (argv[0] "ffmpeg",
//! looked up on `PATH`, this process's environment), its standard input and error on pipes this
//! side keeps non-blocking; its standard output is this process's.

use super::writer::strerror;
use omareel_core::compat::qstring;
use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::raw::c_char;
use std::time::{Duration, Instant};

unsafe extern "C" {
    static environ: *const *mut c_char;
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// A close-on-exec pipe: the read end, then the write end.
fn pipe() -> Result<(OwnedFd, OwnedFd), i32> {
    let mut fds = [-1 as RawFd; 2];
    // SAFETY: pipe2 writes two descriptors.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(errno());
    }
    // SAFETY: both descriptors are new and owned here.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn set_nonblocking(fd: RawFd) {
    // SAFETY: fcntl on an open descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

/// A running encoder: its pid, the pipe to its standard input (closed by the owner when the
/// stream ends) and to its standard error, and what it has said there.
#[derive(Debug)]
pub struct Encoder {
    pub pid: libc::pid_t,
    pub input: Option<OwnedFd>,
    pub error_pipe: Option<OwnedFd>,
    pub error_output: Vec<u8>,
}

impl Encoder {
    /// `startEncoder`: spawns `ffmpeg` with `arguments`. `Err` is `strerror` of the failing
    /// `pipe2` or of `posix_spawnp`'s result.
    pub fn start<S: AsRef<str>>(arguments: &[S]) -> Result<Encoder, String> {
        let (input_read, input_write) = pipe().map_err(strerror)?;
        let (error_read, error_write) = pipe().map_err(strerror)?;
        let mut encoded = Vec::with_capacity(arguments.len() + 1);
        encoded.push(CString::new("ffmpeg").unwrap_or_default());
        for argument in arguments {
            // QString::toLocal8Bit; an argument with a NUL would be cut there by exec.
            let bytes = argument.as_ref().as_bytes();
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            encoded.push(CString::new(&bytes[..end]).unwrap_or_default());
        }
        let mut argv: Vec<*mut c_char> = encoded.iter().map(|argument| argument.as_ptr().cast_mut()).collect();
        argv.push(std::ptr::null_mut());
        let mut pid: libc::pid_t = 0;
        // SAFETY: the file actions are initialised, used and destroyed here; argv and environ
        // are NUL-terminated arrays of NUL-terminated strings that outlive the call.
        let result = unsafe {
            let mut actions: libc::posix_spawn_file_actions_t = std::mem::zeroed();
            libc::posix_spawn_file_actions_init(&mut actions);
            libc::posix_spawn_file_actions_adddup2(&mut actions, input_read.as_raw_fd(), libc::STDIN_FILENO);
            libc::posix_spawn_file_actions_adddup2(&mut actions, error_write.as_raw_fd(), libc::STDERR_FILENO);
            libc::posix_spawn_file_actions_addclose(&mut actions, input_write.as_raw_fd());
            libc::posix_spawn_file_actions_addclose(&mut actions, error_read.as_raw_fd());
            let result = libc::posix_spawnp(
                &mut pid,
                c"ffmpeg".as_ptr(),
                &actions,
                std::ptr::null(),
                argv.as_ptr(),
                environ.cast(),
            );
            libc::posix_spawn_file_actions_destroy(&mut actions);
            result
        };
        drop(input_read);
        drop(error_write);
        if result != 0 {
            return Err(strerror(result));
        }
        set_nonblocking(input_write.as_raw_fd());
        set_nonblocking(error_read.as_raw_fd());
        Ok(Encoder { pid, input: Some(input_write), error_pipe: Some(error_read), error_output: Vec::new() })
    }

    /// `readEncoderError`: what the encoder's standard error holds now, up to 256 reads of 4096
    /// bytes.
    pub fn read_error(&mut self) {
        let Some(fd) = self.error_pipe.as_ref().map(AsRawFd::as_raw_fd) else { return };
        let mut buffer = [0u8; 4096];
        for _ in 0..256 {
            // SAFETY: reads into the local buffer.
            let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
            if count > 0 {
                self.error_output.extend_from_slice(&buffer[..count as usize]);
                continue;
            }
            if count < 0 && errno() == libc::EINTR {
                continue;
            }
            break;
        }
    }

    /// The trimmed standard error, as `QString::fromUtf8(encoderError).trimmed()`.
    pub fn error_text(&self) -> String {
        qstring::trimmed(&qstring::from_utf8(&self.error_output)).to_owned()
    }
}

/// How `stopEncoder` saw the encoder end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderExit {
    /// Reaped with this wait status (`waitpid` gave its pid).
    Reaped(i32),
    /// `waitpid` failed.
    Lost,
    /// Still running at the deadline: killed and reaped.
    TimedOut,
}

/// The waiting part of `stopEncoder` once its input is closed: up to `timeout_ms` of
/// `waitpid(WNOHANG)` 10 ms apart, reading its standard error meanwhile; one still running then
/// gets SIGKILL and is reaped.
pub fn wait_for_encoder(encoder: &mut Encoder, timeout_ms: u64) -> EncoderExit {
    let mut status = 0;
    let started = Instant::now();
    let mut result: libc::pid_t = 0;
    while started.elapsed() < Duration::from_millis(timeout_ms) {
        encoder.read_error();
        // SAFETY: the encoder is this process's child.
        result = unsafe { libc::waitpid(encoder.pid, &mut status, libc::WNOHANG) };
        if result == encoder.pid || result < 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if result == 0 {
        // SAFETY: as above.
        unsafe {
            libc::kill(encoder.pid, libc::SIGKILL);
            libc::waitpid(encoder.pid, &mut status, 0);
        }
        return EncoderExit::TimedOut;
    }
    if result < 0 { EncoderExit::Lost } else { EncoderExit::Reaped(status) }
}
