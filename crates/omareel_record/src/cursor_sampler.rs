//! `CursorSampler` (`src/record/CursorSampler.cpp` at the tag): where the pointer is, asked of
//! Hyprland 240 times a second (`j/cursorpos` over its IPC socket) on a thread of its own while
//! a recording runs. A sample is kept when the pointer moved, and at least every quarter second
//! while it stays still, so `input.jsonl` knows where it was at every click.

use crate::capture::RawCursorSample;
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qdir;
use omareel_core::compat::qjson;
use omareel_core::compat::qround::{q_fuzzy_compare, q_fuzzy_is_null};
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use crate::recorder::monotonic_us;

/// `qEnvironmentVariable(name)`: the value decoded as UTF-8, empty when unset.
fn environment_variable(name: &str) -> String {
    std::env::var_os(name).map(|value| omareel_core::compat::qstring::from_utf8(value.as_bytes())).unwrap_or_default()
}

/// Hyprland's request socket: `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`.
fn socket_path() -> String {
    format!(
        "{}/hypr/{}/.socket.sock",
        environment_variable("XDG_RUNTIME_DIR"),
        environment_variable("HYPRLAND_INSTANCE_SIGNATURE")
    )
}

/// `QtPrivate::fuzzyCompare`, which `QPointF`'s `==` applies to each coordinate: an exact zero
/// on either side compares the difference with `qFuzzyIsNull`, anything else `qFuzzyCompare`s.
fn fuzzy_compare(lhs: f64, rhs: f64) -> bool {
    if lhs == 0.0 || rhs == 0.0 { q_fuzzy_is_null(lhs - rhs) } else { q_fuzzy_compare(lhs, rhs) }
}

/// `QPointF::operator==`.
fn same_point(a: PointF, b: PointF) -> bool {
    fuzzy_compare(a.x, b.x) && fuzzy_compare(a.y, b.y)
}

/// The file-static `queryCursor`: one `j/cursorpos` request on a new connection (20 ms send and
/// receive timeouts), the reply read in up to 16 reads of 256 bytes, and its `x` and `y`.
/// `None` when the socket can't be reached, the request can't be written whole, or the reply is
/// not an object with both.
pub fn query_cursor(path: &[u8]) -> Option<PointF> {
    // SAFETY: socket returns a new descriptor or -1.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return None;
    }
    let close = |fd| {
        // SAFETY: closes the descriptor made above, once.
        unsafe { libc::close(fd) };
    };
    let timeout = libc::timeval { tv_sec: 0, tv_usec: 20000 };
    for option in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        // SAFETY: a timeval of its size.
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                option,
                (&timeout as *const libc::timeval).cast(),
                size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
    }
    // SAFETY: an all-zero sockaddr_un is valid.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    if path.len() >= address.sun_path.len() {
        close(fd);
        return None;
    }
    for (slot, &byte) in address.sun_path.iter_mut().zip(path) {
        *slot = byte as libc::c_char;
    }
    // SAFETY: the address is a sockaddr_un of its size.
    let connected = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            size_of::<libc::sockaddr_un>() as libc::socklen_t,
        )
    };
    if connected != 0 {
        close(fd);
        return None;
    }
    let request = b"j/cursorpos";
    // SAFETY: writes the request's bytes.
    if unsafe { libc::write(fd, request.as_ptr().cast(), request.len()) } != request.len() as isize {
        close(fd);
        return None;
    }
    let mut reply = Vec::new();
    let mut buffer = [0u8; 256];
    for _ in 0..16 {
        // SAFETY: reads into the local buffer.
        let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count <= 0 {
            break;
        }
        reply.extend_from_slice(&buffer[..count as usize]);
    }
    close(fd);
    let object = qjson::object_from_json(&reply).unwrap_or_default();
    if !object.contains("x") || !object.contains("y") {
        return None;
    }
    Some(PointF::new(object.value("x").to_double(0.0), object.value("y").to_double(0.0)))
}

/// `CursorSampler::cursorPosition`: where the pointer is now, asked once. The recording bar's
/// self-view drag asks it too (`globalCursorPos`).
pub fn cursor_position() -> Option<PointF> {
    query_cursor(socket_path().as_bytes())
}

/// `CursorSampler`: the sampling thread and the samples it kept.
#[derive(Default)]
pub struct CursorSampler {
    running: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<RawCursorSample>>>,
    thread: Option<JoinHandle<()>>,
}

impl CursorSampler {
    pub fn new() -> CursorSampler {
        CursorSampler::default()
    }

    /// `start(&error)`: starts sampling, or says that the socket is unavailable (no runtime
    /// directory, no Hyprland instance, or no socket file).
    pub fn start(&mut self) -> Result<(), String> {
        let runtime = environment_variable("XDG_RUNTIME_DIR");
        let signature = environment_variable("HYPRLAND_INSTANCE_SIGNATURE");
        let socket_path = format!("{runtime}/hypr/{signature}/.socket.sock");
        if runtime.is_empty() || signature.is_empty() || !qdir::file_exists(&socket_path) {
            return Err(format!("Hyprland IPC socket is unavailable: {socket_path}"));
        }
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let samples = Arc::clone(&self.samples);
        self.thread = Some(std::thread::spawn(move || run(socket_path.as_bytes(), &running, &samples)));
        Ok(())
    }

    /// `stop()`: ends sampling and waits for the thread.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// `samples()`: a copy of what was kept, in time order.
    pub fn samples(&self) -> Vec<RawCursorSample> {
        self.samples.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for CursorSampler {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `CursorSampler::run`: a query every 1/240 s (4166 µs, on a fixed schedule from the start),
/// keeping it when the pointer moved or a quarter second passed since the last kept one.
fn run(path: &[u8], running: &AtomicBool, samples: &Mutex<Vec<RawCursorSample>>) {
    let mut previous = PointF::default();
    let mut have_previous = false;
    let mut last_stored = 0i64;
    let mut deadline = Instant::now();
    while running.load(Ordering::SeqCst) {
        let timestamp = monotonic_us();
        if let Some(current) = query_cursor(path) {
            if !have_previous || !same_point(current, previous) || timestamp - last_stored >= 250000 {
                samples
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(RawCursorSample { monotonic_us: timestamp, logical_position: current });
                previous = current;
                have_previous = true;
                last_stored = timestamp;
            }
        }
        deadline += Duration::from_micros(1_000_000 / 240);
        // std::this_thread::sleep_until: nothing when the deadline has passed.
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_compare_as_qpointf_does() {
        assert!(same_point(PointF::new(0.0, 1.0), PointF::new(1e-13, 1.0 + 1e-13)));
        assert!(!same_point(PointF::new(0.0, 1.0), PointF::new(1e-11, 1.0)));
        assert!(!same_point(PointF::new(100.0, 1.0), PointF::new(100.5, 1.0)));
        assert!(same_point(PointF::new(-0.0, 5.0), PointF::new(0.0, 5.0)));
    }

    #[test]
    fn a_reply_needs_both_coordinates() {
        let dir = std::env::temp_dir().join(format!("omareel-cursor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut requests = Vec::new();
            for reply in [&b"{\"x\": 12.5, \"y\": 7}"[..], b"{\"x\": 3}", b"[1, 2]", b"not json"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 11];
                stream.read_exact(&mut request).unwrap();
                requests.push(request.to_vec());
                stream.write_all(reply).unwrap();
            }
            requests
        });
        let path = socket.as_os_str().as_bytes();
        assert_eq!(query_cursor(path), Some(PointF::new(12.5, 7.0)));
        assert_eq!(query_cursor(path), None);
        assert_eq!(query_cursor(path), None);
        assert_eq!(query_cursor(path), None);
        assert!(server.join().unwrap().iter().all(|request| request == b"j/cursorpos"));
        assert_eq!(query_cursor(dir.join("missing").as_os_str().as_bytes()), None);
        assert_eq!(query_cursor(&[b'a'; 108]), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
