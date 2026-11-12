//! Running the commands speech work needs (ffmpeg, the speech engines) so they can always be
//! stopped: each runs in a process group of its own, and stopping it (cancelled, timed out, or
//! held back for a recording or an export) kills the whole group, whatever the command started,
//! without waiting for anything that still holds its pipes.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often a running command checks for cancelling and holding.
const POLL: Duration = Duration::from_millis(20);
/// How often the hold callback is asked while a command runs.
const HOLD_POLL: Duration = Duration::from_millis(500);
/// How long the pipes may stay open after the command's group is gone (something that left the
/// group may hold them); after that their output is given up on.
const DRAIN: Duration = Duration::from_secs(2);

/// The process groups of the commands running for a job, so they can be killed from any thread
/// (the editor kills them when it closes, without waiting for the job's thread).
#[derive(Clone, Default)]
pub struct Children(Arc<Mutex<Vec<i32>>>);

impl Children {
    /// Kills every running command's group at once.
    pub fn kill_all(&self) {
        let groups = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for &group in groups.iter() {
            kill_group(group);
        }
    }

    fn add(&self, group: i32) {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(group);
    }

    /// Kills what is left of `group` (the leader has exited but is not reaped yet, so the group
    /// id is still its own), forgets it, and only then reaps the leader.
    fn finish(&self, group: i32, child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
        {
            let mut groups = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            kill_group(group);
            groups.retain(|&g| g != group);
        }
        child.wait().ok()
    }
}

fn kill_group(group: i32) {
    if group > 0 {
        // SAFETY: kill takes a process group id and a signal; the group is one of ours (its
        // leader is not reaped yet, so the id cannot have been reused).
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
}

/// Whether `pid` has exited, without reaping it.
fn exited(pid: i32) -> bool {
    // SAFETY: waitid with WNOWAIT only reports the child's state; siginfo is plain data.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let result = libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT);
        result == 0 && info.si_pid() != 0 || result != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
    }
}

/// Cancelling, holding back, and following a job.
#[derive(Clone, Default)]
pub struct Control {
    pub cancel: Arc<AtomicBool>,
    /// While it says yes the work waits, and a command running then is stopped and run again
    /// later (a recording or an export is running).
    pub hold: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    /// Told the share done, 0 to 1.
    pub progress: Option<Arc<dyn Fn(f64) + Send + Sync>>,
    /// The commands running now.
    pub children: Children,
}

impl Control {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn held(&self) -> bool {
        self.hold.as_ref().is_some_and(|hold| hold())
    }

    /// Waits while held; false once cancelled.
    pub fn proceed(&self) -> bool {
        while !self.cancelled() && self.held() {
            std::thread::sleep(Duration::from_millis(500));
        }
        !self.cancelled()
    }

    pub fn report(&self, fraction: f64) {
        if let Some(progress) = &self.progress {
            progress(fraction.clamp(0.0, 1.0));
        }
    }

    /// Cancels the job and kills what it runs, at once.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.children.kill_all();
    }
}

/// Why a command did not run to its end.
#[derive(Clone, Debug, PartialEq)]
pub enum Stopped {
    Cancelled,
    /// Held back for a recording or an export: run it again once [`Control::proceed`] says so.
    Held,
    TimedOut,
    /// It could not start, or `on_output` stopped it with this message.
    Failed(String),
}

pub struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

fn reader(pipe: Option<impl Read + Send + 'static>, chunks: mpsc::Sender<Vec<u8>>) {
    let Some(mut pipe) = pipe else { return };
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if chunks.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Runs `command` (in a process group of its own, stdin closed) to its end and returns its
/// output, or why it stopped. `on_output` sees standard output as it comes; returning false
/// stops the command early, which still counts as a normal end (the output so far is kept).
/// The group is killed once the command ends too, so nothing it started outlives it.
pub fn run_streaming(
    mut command: Command,
    timeout: Option<Duration>,
    control: &Control,
    mut on_output: impl FnMut(&[u8]) -> bool,
) -> Result<Output, Stopped> {
    use std::os::unix::process::CommandExt;
    let program = command.get_program().to_string_lossy().into_owned();
    command.process_group(0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| Stopped::Failed(format!("could not run {program}: {error}")))?;
    let group = child.id() as i32;
    control.children.add(group);
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    reader(child.stdout.take(), out_tx);
    reader(child.stderr.take(), err_tx);
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut stdout = Vec::new();
    let mut wanted = true;
    let mut next_hold_check = Instant::now() + HOLD_POLL;
    let mut stopped: Option<Stopped> = None;
    let mut out_open = true;
    loop {
        if out_open {
            match out_rx.recv_timeout(POLL) {
                Ok(chunk) => {
                    if wanted {
                        stdout.extend_from_slice(&chunk);
                        wanted = on_output(&chunk);
                    }
                    if wanted {
                        continue;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => out_open = false,
            }
        } else {
            std::thread::sleep(POLL);
        }
        // Cancelling first: `Control::stop` may have killed the group already.
        if control.cancelled() {
            stopped = Some(Stopped::Cancelled);
            break;
        }
        if !wanted || exited(group) {
            break;
        }
        if deadline.is_some_and(|deadline| Instant::now() > deadline) {
            stopped = Some(Stopped::TimedOut);
            break;
        }
        if Instant::now() >= next_hold_check {
            next_hold_check = Instant::now() + HOLD_POLL;
            if control.held() {
                stopped = Some(Stopped::Held);
                break;
            }
        }
    }
    let status = control.children.finish(group, &mut child);
    if let Some(stopped) = stopped {
        return Err(stopped);
    }
    // The group is gone, so the pipes close; wait a little for the rest of the output, but not
    // for something that left the group and still holds them.
    let drain_until = Instant::now() + DRAIN;
    while wanted && out_open {
        match out_rx.recv_timeout(drain_until.saturating_duration_since(Instant::now())) {
            Ok(chunk) => {
                stdout.extend_from_slice(&chunk);
                wanted = on_output(&chunk);
            }
            Err(_) => break,
        }
    }
    let mut stderr = Vec::new();
    while let Ok(chunk) = err_rx.recv_timeout(drain_until.saturating_duration_since(Instant::now())) {
        stderr.extend_from_slice(&chunk);
    }
    let code = if wanted { status.and_then(|status| status.code()).unwrap_or(-1) } else { 0 };
    Ok(Output { code, stdout, stderr: String::from_utf8_lossy(&stderr).into_owned() })
}

/// [`run_streaming`] keeping all the output.
pub fn run(command: Command, timeout: Option<Duration>, control: &Control) -> Result<Output, Stopped> {
    run_streaming(command, timeout, control, |_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// A command that leaves a sleeper holding its pipes ends with its own end, and the sleeper
    /// goes with it.
    #[test]
    fn leftover_children_neither_hold_up_nor_outlive_the_command() {
        let started = Instant::now();
        let output = run(sh("sleep 30 & echo $!; echo hello"), Some(Duration::from_secs(20)), &Control::default()).unwrap();
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let sleeper: i32 = text.lines().next().unwrap().trim().parse().unwrap();
        assert!(text.contains("hello"));
        assert_eq!(output.code, 0);
        std::thread::sleep(Duration::from_millis(100));
        assert!(!alive(sleeper));
    }

    /// The timeout kills the whole group, even when the command's child holds the pipes.
    #[test]
    fn timeouts_kill_the_group() {
        let dir = std::env::temp_dir().join(format!("omareel-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("sleeper");
        let script = format!("sleep 30 & echo $! > {}; exec sleep 40", pidfile.display());
        let started = Instant::now();
        let result = run(sh(&script), Some(Duration::from_millis(500)), &Control::default());
        assert!(matches!(result, Err(Stopped::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
        let sleeper: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!alive(sleeper));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Stopping from another thread (the editor closing) kills the group at once.
    #[test]
    fn stop_kills_from_another_thread() {
        let control = Control::default();
        let stopper = control.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stopper.stop();
        });
        let started = Instant::now();
        let result = run(sh("sleep 30 & sleep 40"), None, &control);
        assert!(matches!(result, Err(Stopped::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    /// Holding stops the command; it can run again afterwards.
    #[test]
    fn holding_stops_the_command() {
        let held = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&held);
        let control = Control { hold: Some(Arc::new(move || flag.load(Ordering::Relaxed))), ..Control::default() };
        let setter = Arc::clone(&held);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            setter.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        assert!(matches!(run(sh("sleep 30"), None, &control), Err(Stopped::Held)));
        assert!(started.elapsed() < Duration::from_secs(3));
        held.store(false, Ordering::Relaxed);
        assert!(control.proceed());
        assert_eq!(run(sh("echo again"), None, &control).unwrap().stdout, b"again\n");
    }

    #[test]
    fn output_can_stop_the_command_early() {
        let started = Instant::now();
        let output = run_streaming(sh("echo first; sleep 30; echo second"), None, &Control::default(), |chunk| {
            !chunk.starts_with(b"first")
        })
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(output.stdout, b"first\n");
    }
}
