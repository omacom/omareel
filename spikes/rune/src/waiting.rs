//! The editor the recorder starts while it records (`edit --wait <recorder pid> <bundle>`) and the
//! bar `omareel record` starts before its region picker (`__record-bar --wait <starter pid>`), as
//! the Qt app's main.cpp has them: the process starts, warms what it can, and waits for SIGUSR1
//! (the bar's sent with `sigqueue`, its value the bar's flags). One whose starter exits without
//! signalling quits.

use std::os::fd::RawFd;
use std::sync::atomic::{AtomicI32, Ordering};

static WRITE_END: AtomicI32 = AtomicI32::new(-1);
static READ_END: AtomicI32 = AtomicI32::new(-1);

extern "C" fn requested(_: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
    // SAFETY: the kernel passes a valid siginfo with SA_SIGINFO; write is async-signal-safe.
    let byte: u8 = unsafe {
        if !info.is_null() && (*info).si_code == libc::SI_QUEUE {
            ((*info).si_value().sival_ptr as usize & 0x7f) as u8
        } else {
            0
        }
    };
    let fd = WRITE_END.load(Ordering::Relaxed);
    // SAFETY: as above; the pipe is open for the process's life.
    let _ = unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
}

/// First thing in main: the recorder looks for the handler (SigCgt, or the bar's pid file)
/// before it signals.
pub fn install() {
    let mut fds = [-1; 2];
    // SAFETY: pipe2 fills two descriptors; sigaction installs a handler that only writes to one.
    unsafe {
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) != 0 {
            return;
        }
        READ_END.store(fds[0], Ordering::Relaxed);
        WRITE_END.store(fds[1], Ordering::Relaxed);
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = requested as usize;
        action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGUSR1, &action, std::ptr::null_mut());
    }
}

fn take(fd: RawFd) -> Option<i32> {
    let mut byte = 0u8;
    // SAFETY: a non-blocking read of one byte into a local.
    (unsafe { libc::read(fd, (&mut byte as *mut u8).cast(), 1) } == 1).then_some(i32::from(byte))
}

/// Waits for the signal, calling `tick` about every `tick_ms` meanwhile: its flags, or `None` when
/// `starter` exited without sending it (`grace_ms` after, for a signal on its way).
pub fn wait(starter: i32, grace_ms: i32, tick_ms: i32, mut tick: impl FnMut()) -> Option<i32> {
    let read_end = READ_END.load(Ordering::Relaxed);
    if read_end < 0 {
        return None;
    }
    // SAFETY: pidfd_open takes a pid and flags; a negative result is an error.
    let mut starter_fd = unsafe { libc::syscall(libc::SYS_pidfd_open, starter, 0) } as RawFd;
    let mut deadline: Option<std::time::Instant> = None;
    loop {
        if let Some(flags) = take(read_end) {
            if starter_fd >= 0 {
                // SAFETY: the pidfd is this function's.
                unsafe { libc::close(starter_fd) };
            }
            return Some(flags);
        }
        if starter_fd < 0 && deadline.is_none() {
            deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(grace_ms as u64));
        }
        if let Some(at) = deadline
            && std::time::Instant::now() >= at
        {
            return take(read_end);
        }
        let mut fds = [
            libc::pollfd { fd: read_end, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: if deadline.is_none() { starter_fd } else { -1 }, events: libc::POLLIN, revents: 0 },
        ];
        let timeout = deadline.map_or(tick_ms, |at| {
            (at.saturating_duration_since(std::time::Instant::now()).as_millis() as i32).min(tick_ms)
        });
        // SAFETY: two pollfds on the stack.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if ready == 0 {
            tick();
        }
        if fds[1].revents != 0 && deadline.is_none() {
            deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(grace_ms as u64));
            // SAFETY: as above.
            unsafe { libc::close(starter_fd) };
            starter_fd = -1;
        }
    }
}
