//! Feeding raw frames to the encoder and finishing it (`EncoderPipe`). Both block, for the
//! encode thread. A frame is only taken as sent once the process's write buffer is empty, so the
//! next one never piles up behind it; a write that merely made progress is not enough.

use crate::compat::qprocess::{ExitStatus, Process, ProcessError, ProcessState};
use crate::compat::qstring;
use std::time::Instant;

const POLL_MS: i32 = 50;

/// How long a frame may make no progress into the pipe.
pub const STALL_TIMEOUT_MS: i32 = 30000;
/// How long the encoder may take to finish once its input is closed.
pub const FINISH_TIMEOUT_MS: i32 = 120000;

/// `reason`, with what the encoder printed on its standard error so far.
fn fail(process: &mut Process, reason: &str) -> String {
    let details = qstring::from_utf8(&process.read_all_standard_error());
    let details = qstring::trimmed(&details);
    if details.is_empty() { reason.to_owned() } else { format!("{reason}: {details}") }
}

fn elapsed_ms(since: Instant) -> i64 {
    i64::try_from(since.elapsed().as_millis()).unwrap_or(i64::MAX)
}

/// Writes one frame. The deadline is for lack of progress, not for the total time a slow
/// machine takes to encode.
pub fn write_encoder_frame(
    process: &mut Process,
    frame: &[u8],
    cancelled: &dyn Fn() -> bool,
    stall_timeout_ms: i32,
) -> Result<(), String> {
    let mut stalled = Instant::now();
    let mut written = 0usize;
    while written < frame.len() || process.bytes_to_write() > 0 {
        if cancelled() {
            return Err(fail(process, "Export cancelled"));
        }
        if process.state() == ProcessState::NotRunning {
            return Err(fail(process, "Encoder exited before receiving all frames"));
        }
        if process.bytes_to_write() == 0 {
            let amount = process.write(&frame[written..]);
            if amount <= 0 {
                return Err(fail(process, "Could not write a frame to the encoder"));
            }
            written += amount as usize;
        }
        // A successful wait drains only part of the buffer, often just one pipe page. Keep
        // draining before accepting the next frame.
        if process.wait_for_bytes_written(POLL_MS.min(stall_timeout_ms)) {
            stalled = Instant::now();
        } else if process.error() == ProcessError::WriteError {
            return Err(fail(process, "Could not write a frame to the encoder"));
        }
        if elapsed_ms(stalled) >= i64::from(stall_timeout_ms) && process.bytes_to_write() > 0 {
            return Err(fail(process, "Encoder stopped accepting frames (write timed out)"));
        }
    }
    Ok(())
}

/// Closes the encoder's input and waits for it to write the file and exit successfully.
pub fn finish_encoder_pipe(process: &mut Process, cancelled: &dyn Fn() -> bool, timeout_ms: i32) -> Result<(), String> {
    process.close_write_channel();
    let timer = Instant::now();
    while process.state() != ProcessState::NotRunning {
        if cancelled() {
            return Err(fail(process, "Export cancelled"));
        }
        let remaining = i64::from(timeout_ms) - elapsed_ms(timer);
        if remaining <= 0 {
            return Err(fail(process, "Encoder timed out while finishing the video"));
        }
        process.wait_for_finished(remaining.min(i64::from(POLL_MS)) as i32);
    }
    if cancelled() {
        return Err(fail(process, "Export cancelled"));
    }
    if process.exit_status() != ExitStatus::NormalExit || process.exit_code() != 0 {
        return Err(fail(process, &format!("Encoder failed (exit code {})", process.exit_code())));
    }
    Ok(())
}

// tests/test_encoderpipe.cpp, with the test binary itself as the child it started.
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Duration;

    const CHILD_MODE: &str = "OMAREEL_PIPE_CHILD";

    /// The child side: run with `OMAREEL_PIPE_CHILD` set, it never returns. "failed" exits 7,
    /// "killed" kills itself, "stalled" sleeps a minute without reading, "slow" reads all of its
    /// input 4096 bytes at a time with a millisecond's pause, checks it is all "x", waits 150 ms
    /// and prints how many bytes it got.
    #[test]
    fn pipe_child() {
        let Ok(mode) = std::env::var(CHILD_MODE) else { return };
        let code = match mode.as_str() {
            "failed" => 7,
            "killed" => {
                // SAFETY: raising a signal has no preconditions.
                unsafe { libc::raise(libc::SIGKILL) };
                1
            }
            "stalled" => {
                std::thread::sleep(Duration::from_secs(60));
                0
            }
            _ => {
                let mut buffer = [0u8; 4096];
                let mut total: u64 = 0;
                let mut stdin = std::io::stdin().lock();
                let result = loop {
                    match stdin.read(&mut buffer) {
                        Ok(0) => break 0,
                        Ok(amount) => {
                            if buffer[..amount].iter().any(|&b| b != b'x') {
                                break 8;
                            }
                            total += amount as u64;
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => break 9,
                    }
                };
                if result == 0 {
                    std::thread::sleep(Duration::from_millis(150));
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{total}");
                    let _ = out.flush();
                }
                result
            }
        };
        std::process::exit(code);
    }

    /// A child in `mode`, killed when the test is done as the C++ test's `Child` did.
    struct Child(Process);

    impl Child {
        fn launch(mode: &str) -> Child {
            let exe = std::env::current_exe().expect("test binary");
            let mut process = Process::new();
            let script = format!(
                "{CHILD_MODE}={mode} exec \"$0\" --exact export::encoder_pipe::tests::pipe_child --nocapture --test-threads=1 -q"
            );
            process.start("sh", &["-c", &script, exe.to_str().expect("UTF-8 path")]);
            assert!(process.wait_for_started(2000));
            Child(process)
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            if self.0.state() != ProcessState::NotRunning {
                self.0.kill();
                self.0.wait_for_finished(2000);
            }
        }
    }

    fn never_cancelled() -> bool {
        false
    }

    /// What the "slow" child printed last (the test harness prints a line before it).
    fn printed_total(process: &mut Process) -> u64 {
        let out = String::from_utf8_lossy(&process.read_all_standard_output()).into_owned();
        out.lines().rfind(|l| !l.trim().is_empty()).and_then(|l| l.trim().parse().ok()).unwrap_or(0)
    }

    #[test]
    fn slow_consumer_keeps_buffer_bounded() {
        let mut child = Child::launch("slow");
        let frame = vec![b'x'; 512 * 1024];
        let started = Instant::now();
        for _ in 0..8 {
            write_encoder_frame(&mut child.0, &frame, &never_cancelled, 1000).expect("write");
            assert_eq!(child.0.bytes_to_write(), 0);
        }
        assert!(started.elapsed() > Duration::from_millis(500), "writes must wait for the deliberately slow consumer");
        finish_encoder_pipe(&mut child.0, &never_cancelled, FINISH_TIMEOUT_MS).expect("finish");
        assert_eq!(printed_total(&mut child.0), frame.len() as u64 * 8);
    }

    #[test]
    fn progressing_frame_does_not_time_out() {
        let mut child = Child::launch("slow");
        let started = Instant::now();
        write_encoder_frame(&mut child.0, &vec![b'x'; 2 * 1024 * 1024], &never_cancelled, 250).expect("write");
        assert!(started.elapsed() > Duration::from_millis(250));
        assert_eq!(child.0.bytes_to_write(), 0);
        finish_encoder_pipe(&mut child.0, &never_cancelled, FINISH_TIMEOUT_MS).expect("finish");
    }

    #[test]
    fn cancels_blocked_write() {
        let mut child = Child::launch("stalled");
        let started = Instant::now();
        let cancelled = || started.elapsed() >= Duration::from_millis(100);
        let error = write_encoder_frame(&mut child.0, &vec![b'x'; 1024 * 1024], &cancelled, STALL_TIMEOUT_MS).unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stalled_write_has_diagnostic() {
        let mut child = Child::launch("stalled");
        let error = write_encoder_frame(&mut child.0, &vec![b'x'; 1024 * 1024], &never_cancelled, 100).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
    }

    #[test]
    fn early_exit_has_diagnostic() {
        let mut child = Child::launch("failed");
        let error =
            write_encoder_frame(&mut child.0, &vec![b'x'; 1024 * 1024], &never_cancelled, STALL_TIMEOUT_MS).unwrap_err();
        assert!(!error.is_empty());
    }

    #[test]
    fn finalization_failure() {
        for mode in ["failed", "killed"] {
            let mut child = Child::launch(mode);
            let error = finish_encoder_pipe(&mut child.0, &never_cancelled, FINISH_TIMEOUT_MS).unwrap_err();
            assert!(error.contains("Encoder failed"), "{mode}: {error}");
        }
    }

    #[test]
    fn finalization_timeout_has_diagnostic() {
        let mut child = Child::launch("stalled");
        let error = finish_encoder_pipe(&mut child.0, &never_cancelled, 100).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
    }

    #[test]
    fn cancels_finalization() {
        let mut child = Child::launch("stalled");
        let started = Instant::now();
        let cancelled = || started.elapsed() >= Duration::from_millis(100);
        let error = finish_encoder_pipe(&mut child.0, &cancelled, FINISH_TIMEOUT_MS).unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn waits_for_finalization() {
        let mut child = Child::launch("slow");
        write_encoder_frame(&mut child.0, &[b'x'; 16384], &never_cancelled, STALL_TIMEOUT_MS).expect("write");
        let started = Instant::now();
        finish_encoder_pipe(&mut child.0, &never_cancelled, FINISH_TIMEOUT_MS).expect("finish");
        assert!(started.elapsed() >= Duration::from_millis(100));
        assert_eq!((child.0.exit_status(), child.0.exit_code()), (ExitStatus::NormalExit, 0));
    }
}
