//! `CursorSampler` (`src/record/CursorSampler.cpp` at the tag): where the pointer is while a
//! recording runs, followed on a thread of its own. A sample is kept when the pointer moved, and
//! at least every quarter second while it stays still, so `input.jsonl` knows where it was at
//! every click. Samples hold [`monotonic_us`] times and global logical coordinates.
//!
//! The C++ asked Hyprland 240 times a second (`j/cursorpos` on its IPC socket, a new connection
//! each time). Hyprland serves IPC on its main thread, the one that renders, so the sampler now
//! listens instead: an `ext-image-copy-capture-v1` pointer cursor session on the recorded output,
//! on a Wayland connection of its own, is told by the compositor where the pointer is. It never
//! asks for cursor images (no `get_capture_session`). Hyprland 0.56
//! (`CImageCopyCaptureCursorSession::sendCursorEvents`) sends, on each commit of the output's
//! monitor:
//!
//! - `enter` and `leave` when the cursor image's box starts or stops overlapping the output;
//! - `position` when the pointer moved while it overlaps, in logical coordinates relative to the
//!   output's logical box, truncated to integers (`j/cursorpos` floors, so this is as precise),
//!   and never one equal to the last it sent (it starts at 0,0).
//!
//! The monitor commits when the pointer moves with a software cursor (its damage schedules a
//! frame) and, on DRM, with a hardware cursor too: aquamarine's `moveCursor` schedules a frame
//! (`AQ_SCHEDULE_CURSOR_MOVE`) and the cursor plane moves with the next commit. Only a hidden
//! cursor, or a fullscreen VRR window under `cursor:no_break_fs_vrr`, skips that frame. (A nested
//! Hyprland's Wayland backend has no cursor plane: its `moveCursor` does nothing, so there a
//! hardware cursor's moves are only seen when something else redraws the output.)
//!
//! While the pointer is off the recorded output (after `leave`) the session is silent, so where it
//! went is asked of IPC once, and kept like a still pointer's position until it comes back: the
//! editor's cursor leaves the frame instead of sticking at its edge.
//!
//! Since moves the output never commits are moves the session never reports, a silent session is
//! checked against IPC: after a second without events, one `j/cursorpos` (and one a second after
//! that while the silence lasts). Should it disagree twice in a row, 100 ms apart, with nothing
//! from the session in between, the session missed moves and the sampler polls for the rest of
//! the recording. A still pointer costs one request a second instead of 240. (A hidden cursor has
//! no image to overlap the output, so Hyprland says `leave` with the pointer on it: that too
//! ends in polling, which still knows where the pointer is.)
//!
//! Hyprland only sends positions to clients with the `cursorpos` permission. Every client has it
//! unless `ecosystem:enforce_permissions` is on; then an unknown client would make Hyprland ask
//! the user, in a dialog the recording would capture, so the session is not tried. Without the
//! protocol, or when the session fails or stays silent with the pointer on the output, the C++'s
//! polling is the fallback, at the recording's frame rate (60 to 240 queries a second): the
//! editor keeps at most one move per captured frame (`cursor_path::decimate`), so more would be
//! thrown away.

use crate::capture::RawCursorSample;
use crate::recorder::debug_start_stage;
use crate::recorder::self_view::monitor_layouts;
use omareel_core::compat::geometry::{PointF, RectF};
use omareel_core::compat::qdir;
use omareel_core::compat::qjson;
use omareel_core::compat::qround::{q_fuzzy_compare, q_fuzzy_is_null};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wayland_client::backend::WaylandError;
use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_cursor_session_v1::{self, ExtImageCopyCaptureCursorSessionV1},
    ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
};

pub use crate::recorder::monotonic_us;

/// How long a still pointer goes without a sample: a quarter second.
const STILL_INTERVAL_US: i64 = 250_000;

/// How long the session may stay silent before IPC is asked whether it missed a move.
const SILENCE_CHECK_US: i64 = 1_000_000;

/// How soon a disagreement is checked again before the session is given up: long enough for a
/// move IPC saw first to reach the session with the output's next commit.
const RECHECK_US: i64 = 100_000;

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

/// How many requests [`ipc_request`] has sent Hyprland in this process.
static IPC_REQUESTS: AtomicUsize = AtomicUsize::new(0);

/// How many IPC requests this process's cursor sampling (and [`cursor_position`]) has made: what
/// `examples/cursor_probe` reports, to show the session leaves Hyprland's IPC alone.
pub fn ipc_request_count() -> usize {
    IPC_REQUESTS.load(Ordering::Relaxed)
}

/// One request to Hyprland's IPC socket at `path` on a new connection (20 ms send and receive
/// timeouts), the reply read in up to 16 reads of 256 bytes. `None` when the socket can't be
/// reached or the request can't be written whole.
fn ipc_request(path: &[u8], request: &[u8]) -> Option<Vec<u8>> {
    IPC_REQUESTS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: socket returns a new descriptor or -1.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return None;
    }
    // SAFETY: the descriptor was just made and is owned here, closed when this drops.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let timeout = libc::timeval { tv_sec: 0, tv_usec: 20000 };
    for option in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        // SAFETY: a timeval of its size.
        unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
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
        return None;
    }
    for (slot, &byte) in address.sun_path.iter_mut().zip(path) {
        *slot = byte as libc::c_char;
    }
    // SAFETY: the address is a sockaddr_un of its size.
    let connected = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            size_of::<libc::sockaddr_un>() as libc::socklen_t,
        )
    };
    if connected != 0 {
        return None;
    }
    // SAFETY: writes the request's bytes.
    if unsafe { libc::write(fd.as_raw_fd(), request.as_ptr().cast(), request.len()) } != request.len() as isize {
        return None;
    }
    let mut reply = Vec::new();
    let mut buffer = [0u8; 256];
    for _ in 0..16 {
        // SAFETY: reads into the local buffer.
        let count = unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if count <= 0 {
            break;
        }
        reply.extend_from_slice(&buffer[..count as usize]);
    }
    Some(reply)
}

/// The file-static `queryCursor`: one `j/cursorpos` request ([`ipc_request`]) and the reply's
/// `x` and `y`. `None` when the socket can't be reached, the request can't be written whole, or
/// the reply is not an object with both.
pub fn query_cursor(path: &[u8]) -> Option<PointF> {
    let reply = ipc_request(path, b"j/cursorpos")?;
    let object = qjson::object_from_json(&reply).unwrap_or_default();
    if !object.contains("x") || !object.contains("y") {
        return None;
    }
    Some(PointF::new(object.value("x").to_double(0.0), object.value("y").to_double(0.0)))
}

/// Whether `j/getoption ecosystem:enforce_permissions` says Hyprland enforces permissions
/// (a `bool`, or an `int` other than 0, set). An unanswered request counts as not enforcing:
/// the session's own silence then sends the sampler back to polling.
fn permissions_enforced(path: &[u8]) -> bool {
    let reply = ipc_request(path, b"j/getoption ecosystem:enforce_permissions").unwrap_or_default();
    let object = qjson::object_from_json(&reply).unwrap_or_default();
    object.value("bool").to_bool(false) || object.value("int").to_int(0) != 0
}

/// `CursorSampler::cursorPosition`: where the pointer is now, asked once. The recording bar's
/// self-view drag asks it too (`globalCursorPos`).
pub fn cursor_position() -> Option<PointF> {
    query_cursor(socket_path().as_bytes())
}

/// Where a sampler's samples come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorSource {
    /// Not started, or still choosing.
    #[default]
    None,
    /// The compositor's cursor session.
    Session,
    /// Hyprland's IPC, polled.
    Polling,
}

impl CursorSource {
    fn from_u8(value: u8) -> CursorSource {
        match value {
            1 => CursorSource::Session,
            2 => CursorSource::Polling,
            _ => CursorSource::None,
        }
    }
}

/// `CursorSampler`: the sampling thread and the samples it kept.
#[derive(Default)]
pub struct CursorSampler {
    stop: Option<Arc<StopSignal>>,
    source: Arc<AtomicU8>,
    samples: Arc<Mutex<Vec<RawCursorSample>>>,
    thread: Option<JoinHandle<()>>,
}

impl CursorSampler {
    pub fn new() -> CursorSampler {
        CursorSampler::default()
    }

    /// `start(&error)`: starts following the pointer over `monitor` (a recording at `fps`), or
    /// says that the IPC socket is unavailable (no runtime directory, no Hyprland instance, or no
    /// socket file). The thread chooses the cursor session or polling itself ([`source`]).
    ///
    /// [`source`]: CursorSampler::source
    pub fn start(&mut self, monitor: &str, fps: i32) -> Result<(), String> {
        let runtime = environment_variable("XDG_RUNTIME_DIR");
        let signature = environment_variable("HYPRLAND_INSTANCE_SIGNATURE");
        let socket_path = format!("{runtime}/hypr/{signature}/.socket.sock");
        if runtime.is_empty() || signature.is_empty() || !qdir::file_exists(&socket_path) {
            return Err(format!("Hyprland IPC socket is unavailable: {socket_path}"));
        }
        let stop = Arc::new(StopSignal::new().map_err(|error| format!("Cursor sampling could not start: {error}"))?);
        self.stop = Some(Arc::clone(&stop));
        self.source.store(CursorSource::None as u8, Ordering::SeqCst);
        let source = Arc::clone(&self.source);
        let keeper = Keeper::new(Arc::clone(&self.samples));
        let monitor = monitor.to_owned();
        let interval = polling_interval(fps);
        self.thread =
            Some(std::thread::spawn(move || run(socket_path.as_bytes(), &monitor, interval, &stop, &source, keeper)));
        Ok(())
    }

    /// `stop()`: ends sampling and waits for the thread.
    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.raise();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Where the samples come from: the session, or polling once the sampler fell back (at the
    /// start or later in the run). It stays as it ended after [`stop`](CursorSampler::stop), so
    /// it can be read with the samples.
    pub fn source(&self) -> CursorSource {
        CursorSource::from_u8(self.source.load(Ordering::SeqCst))
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

/// How often polling asks: once per captured frame, 60 to 240 times a second.
fn polling_interval(fps: i32) -> Duration {
    Duration::from_micros(1_000_000 / fps.clamp(60, 240) as u64)
}

/// `CursorSampler::run`'s condition, which both sources share: a position is kept when it is the
/// first, the pointer moved, or a quarter second passed since the last kept one.
struct Keeper {
    samples: Arc<Mutex<Vec<RawCursorSample>>>,
    previous: Option<PointF>,
    last_stored: i64,
}

impl Keeper {
    fn new(samples: Arc<Mutex<Vec<RawCursorSample>>>) -> Keeper {
        Keeper { samples, previous: None, last_stored: 0 }
    }

    /// Keeps `current` at `timestamp` when the condition holds.
    fn offer(&mut self, timestamp: i64, current: PointF) {
        let keep = match self.previous {
            None => true,
            Some(previous) => !same_point(current, previous) || timestamp - self.last_stored >= STILL_INTERVAL_US,
        };
        if keep {
            self.samples
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(RawCursorSample { monotonic_us: timestamp, logical_position: current });
            self.previous = Some(current);
            self.last_stored = timestamp;
        }
    }

    /// When a still pointer's position is next due again; none before the first sample.
    fn repeat_due(&self) -> Option<i64> {
        self.previous.map(|_| self.last_stored + STILL_INTERVAL_US)
    }

    /// The last position again at `timestamp`, when it is due (the session only speaks when the
    /// pointer moves, so a still pointer's samples are made up here).
    fn repeat(&mut self, timestamp: i64) {
        if let Some(previous) = self.previous {
            self.offer(timestamp, previous);
        }
    }
}

/// An eventfd the sampling thread waits on beside its work; [`StopSignal::raise`] wakes it.
struct StopSignal(OwnedFd);

impl StopSignal {
    fn new() -> std::io::Result<StopSignal> {
        // SAFETY: eventfd returns a new descriptor or -1.
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: the descriptor was just made and is owned here.
        Ok(StopSignal(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    /// Makes the descriptor readable for good (nothing reads it).
    fn raise(&self) {
        let one: u64 = 1;
        // SAFETY: writes eight bytes from a u64.
        unsafe { libc::write(self.0.as_raw_fd(), (&one as *const u64).cast(), 8) };
    }
}

/// Waits up to `timeout_us` (forever without one) for the stop signal or for `fd` to be
/// readable. Says whether to stop and whether `fd` is readable; an interrupted wait is neither.
fn wait(stop: &StopSignal, fd: Option<RawFd>, timeout_us: Option<i64>) -> (bool, bool) {
    let mut descriptors = [
        libc::pollfd { fd: stop.0.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: fd.unwrap_or(-1), events: libc::POLLIN, revents: 0 },
    ];
    let timeout = timeout_us.map(|us| {
        let us = us.max(0);
        libc::timespec { tv_sec: us / 1_000_000, tv_nsec: (us % 1_000_000) * 1000 }
    });
    let count = if fd.is_some() { 2 } else { 1 };
    // SAFETY: `count` pollfds and an optional timespec, no signal mask.
    let result = unsafe {
        libc::ppoll(
            descriptors.as_mut_ptr(),
            count,
            timeout.as_ref().map_or(std::ptr::null(), |timeout| timeout as *const libc::timespec),
            std::ptr::null(),
        )
    };
    if result <= 0 {
        return (false, false);
    }
    let stop = descriptors[0].revents != 0;
    let readable = count == 2 && descriptors[1].revents != 0;
    (stop, readable)
}

/// The sampling thread: the cursor session when the compositor offers one that works, else
/// polling, until stopped.
fn run(
    ipc: &[u8],
    monitor: &str,
    interval: Duration,
    stop: &StopSignal,
    source: &AtomicU8,
    mut keeper: Keeper,
) {
    let start_us = monotonic_us();
    let reason = match open_session(ipc, monitor) {
        Ok(mut session) => {
            source.store(CursorSource::Session as u8, Ordering::SeqCst);
            debug_start_stage("cursor_session", start_us, &format!("monitor={monitor}"));
            match session.listen(ipc, stop, &mut keeper) {
                Ok(()) => return,
                Err(error) => error,
            }
        }
        Err(error) => error,
    };
    source.store(CursorSource::Polling as u8, Ordering::SeqCst);
    debug_start_stage("cursor_polling", start_us, &format!("reason={reason:?}"));
    poll(ipc, interval, stop, &mut keeper);
}

/// The C++'s loop (`CursorSampler::run`): a query every `interval`, on a fixed schedule from the
/// start, offered to the keeper.
fn poll(ipc: &[u8], interval: Duration, stop: &StopSignal, keeper: &mut Keeper) {
    let mut deadline = Instant::now();
    loop {
        let timestamp = monotonic_us();
        if let Some(current) = query_cursor(ipc) {
            keeper.offer(timestamp, current);
        }
        deadline += interval;
        // Nothing to wait for when the deadline has passed, as std::this_thread::sleep_until.
        let remaining = deadline.saturating_duration_since(Instant::now()).as_micros() as i64;
        if wait(stop, None, Some(remaining)).0 {
            return;
        }
    }
}

/// What the cursor session said, in order, for the sampling loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionEvent {
    Enter,
    Leave,
    Position(i32, i32),
}

/// What the session connection's handlers write: the globals, the outputs' names, whether the
/// seat has a pointer, and the cursor session's events not yet taken.
#[derive(Default)]
struct SessionState {
    capture_manager: Option<ExtImageCopyCaptureManagerV1>,
    source_manager: Option<ExtOutputImageCaptureSourceManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    pointer_capability: bool,
    outputs: Vec<(wl_output::WlOutput, String)>,
    events: Vec<SessionEvent>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for SessionState {
    fn event(
        state: &mut SessionState,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<SessionState>,
    ) {
        let wl_registry::Event::Global { name, interface, version } = event else { return };
        match interface.as_str() {
            "ext_image_copy_capture_manager_v1" => state.capture_manager = Some(registry.bind(name, 1, queue, ())),
            "ext_output_image_capture_source_manager_v1" => {
                state.source_manager = Some(registry.bind(name, 1, queue, ()));
            }
            "wl_seat" if state.seat.is_none() => state.seat = Some(registry.bind(name, 1, queue, ())),
            "wl_output" => {
                let object = registry.bind(name, version.min(4), queue, ());
                state.outputs.push((object, String::new()));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for SessionState {
    fn event(
        state: &mut SessionState,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<SessionState>,
    ) {
        if let wl_output::Event::Name { name } = event {
            for entry in state.outputs.iter_mut().filter(|entry| entry.0 == *output) {
                entry.1 = name.clone();
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for SessionState {
    fn event(
        state: &mut SessionState,
        _: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<SessionState>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(capabilities) } = event {
            state.pointer_capability = capabilities.contains(wl_seat::Capability::Pointer);
        }
    }
}

impl Dispatch<ExtImageCopyCaptureCursorSessionV1, ()> for SessionState {
    fn event(
        state: &mut SessionState,
        _: &ExtImageCopyCaptureCursorSessionV1,
        event: ext_image_copy_capture_cursor_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<SessionState>,
    ) {
        use ext_image_copy_capture_cursor_session_v1::Event;
        match event {
            Event::Enter => state.events.push(SessionEvent::Enter),
            Event::Leave => state.events.push(SessionEvent::Leave),
            Event::Position { x, y } => state.events.push(SessionEvent::Position(x, y)),
            _ => {}
        }
    }
}

// The pointer gets no events of its own: it has no surface to enter.
delegate_noop!(SessionState: ignore wl_pointer::WlPointer);
delegate_noop!(SessionState: ExtImageCopyCaptureManagerV1);
delegate_noop!(SessionState: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(SessionState: ExtImageCaptureSourceV1);

/// A connection of the sampler's own to the display `WAYLAND_DISPLAY` names ("wayland-0" when
/// unset), inside `XDG_RUNTIME_DIR` unless it is absolute. Unlike `screen_capture`'s, it never
/// takes a `WAYLAND_SOCKET` descriptor: that is single-use, and the capture's.
fn connect_display() -> Option<Connection> {
    let name = std::env::var_os("WAYLAND_DISPLAY").map(|value| value.as_bytes().to_vec()).unwrap_or(b"wayland-0".into());
    let path = if name.first() == Some(&b'/') {
        name
    } else {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
        if runtime.as_bytes().first() != Some(&b'/') {
            return None;
        }
        [runtime.as_bytes(), b"/", &name].concat()
    };
    let stream = UnixStream::connect(std::ffi::OsStr::from_bytes(&path)).ok()?;
    Connection::from_socket(stream).ok()
}

/// Whether a cursor session that has had its first roundtrip can be believed: it said the pointer
/// entered the output, or IPC puts the pointer off the output, where the session is rightly
/// silent. A session that says nothing with the pointer on the output lacks the permission.
fn session_answered(entered: bool, pointer: Option<PointF>, output: RectF) -> bool {
    let Some(pointer) = pointer else { return entered };
    let inside = pointer.x >= output.x
        && pointer.y >= output.y
        && pointer.x < output.x + output.width
        && pointer.y < output.y + output.height;
    entered || !inside
}

/// Where the session's events put the pointer.
struct Tracker {
    /// The output's logical box; its origin is added to the session's relative positions.
    output: RectF,
    /// The pointer, relative to the output, as the session last put it (Hyprland starts at 0,0
    /// and never repeats itself, so an `enter` without a `position` means this one).
    relative: (i32, i32),
    entered: bool,
}

impl Tracker {
    fn new(output: RectF) -> Tracker {
        Tracker { output, relative: (0, 0), entered: false }
    }

    /// Where the session last put the pointer, globally.
    fn position(&self) -> PointF {
        PointF::new(self.output.x + f64::from(self.relative.0), self.output.y + f64::from(self.relative.1))
    }

    /// Whether `pointer` (from IPC, floored) agrees with what the session said: within a pixel
    /// of its position while the pointer is on the output (the session truncates), off the
    /// output after a `leave`.
    fn agrees(&self, pointer: PointF) -> bool {
        if self.entered {
            let position = self.position();
            (pointer.x - position.x).abs() <= 1.0 && (pointer.y - position.y).abs() <= 1.0
        } else {
            session_answered(false, Some(pointer), self.output)
        }
    }

    /// Turns events from the session into samples at `now`: an `enter` or a `position` keeps
    /// where the pointer is on the output, a `leave` asks IPC where it went.
    fn take(&mut self, events: &[SessionEvent], ipc: &[u8], now: i64, keeper: &mut Keeper) {
        let mut moved = false;
        for event in events {
            match *event {
                SessionEvent::Enter => {
                    self.entered = true;
                    moved = true;
                }
                SessionEvent::Leave => {
                    self.entered = false;
                    moved = false;
                    if let Some(position) = query_cursor(ipc) {
                        keeper.offer(now, position);
                    }
                }
                SessionEvent::Position(x, y) => {
                    self.relative = (x, y);
                    moved = self.entered;
                }
            }
        }
        if moved {
            keeper.offer(now, self.position());
        }
    }
}

/// When to ask IPC whether a silent session missed a move ([`Tracker::agrees`]).
struct SilenceCheck {
    /// When the next question is due.
    due: i64,
    /// Whether the last answer disagreed, with nothing from the session since.
    suspect: bool,
}

impl SilenceCheck {
    fn new(now: i64) -> SilenceCheck {
        SilenceCheck { due: now + SILENCE_CHECK_US, suspect: false }
    }

    /// The session spoke at `now`: it is not silent.
    fn heard(&mut self, now: i64) {
        *self = SilenceCheck::new(now);
    }

    /// What IPC said at `now` (`None`: no answer, which proves nothing). `Err` once it disagreed
    /// twice in a row: the session missed a move.
    fn answer(&mut self, now: i64, agrees: Option<bool>) -> Result<(), ()> {
        match agrees {
            Some(false) if self.suspect => return Err(()),
            Some(false) => {
                self.suspect = true;
                self.due = now + RECHECK_US;
            }
            _ => *self = SilenceCheck::new(now),
        }
        Ok(())
    }
}

/// An open cursor session on the recorded output, with what the sampling loop needs.
struct Session {
    connection: Connection,
    queue: EventQueue<SessionState>,
    state: SessionState,
    source: ExtImageCaptureSourceV1,
    session: ExtImageCopyCaptureCursorSessionV1,
    tracker: Tracker,
    /// Where IPC put the pointer when it was off the output as the session opened.
    initial: Option<PointF>,
}

/// Opens a cursor session on `monitor`: connects, binds the globals and the seat's pointer,
/// creates the output's source and the session, and checks its first answer
/// ([`session_answered`]). `Err` says why the sampler polls instead.
fn open_session(ipc: &[u8], monitor: &str) -> Result<Session, String> {
    if permissions_enforced(ipc) {
        return Err("Hyprland enforces permissions".into());
    }
    let Some(layout) = monitor_layouts().into_iter().find(|layout| layout.name == monitor) else {
        return Err(format!("monitor {monitor} is not in hyprctl monitors"));
    };
    let connection = connect_display().ok_or("no Wayland display")?;
    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    let _registry = connection.display().get_registry(&handle, ());
    let mut state = SessionState::default();
    // The second roundtrip brings the outputs' names and the seat's capabilities.
    for _ in 0..2 {
        queue.roundtrip(&mut state).map_err(|error| format!("registry roundtrip failed: {error}"))?;
    }
    let (Some(capture_manager), Some(source_manager), Some(seat)) =
        (state.capture_manager.clone(), state.source_manager.clone(), state.seat.clone())
    else {
        return Err("ext-image-copy-capture cursor sessions are unavailable".into());
    };
    if !state.pointer_capability {
        return Err("the seat has no pointer".into());
    }
    let Some(output) = state.outputs.iter().find(|output| output.1 == monitor).map(|output| output.0.clone()) else {
        return Err(format!("output {monitor} is not on the display"));
    };
    let pointer = seat.get_pointer(&handle, ());
    let source = source_manager.create_source(&output, &handle, ());
    let session = capture_manager.create_pointer_cursor_session(&source, &pointer, &handle, ());
    // Hyprland answers a new session at once when the pointer is over the output.
    queue.roundtrip(&mut state).map_err(|error| format!("session roundtrip failed: {error}"))?;
    let entered = state.events.contains(&SessionEvent::Enter);
    let initial = if entered { None } else { query_cursor(ipc) };
    let mut opened = Session {
        connection,
        queue,
        state,
        source,
        session,
        tracker: Tracker::new(layout.geometry),
        initial,
    };
    if !session_answered(entered, initial, layout.geometry) {
        opened.close();
        return Err("the cursor session is silent with the pointer on the output".into());
    }
    Ok(opened)
}

impl Session {
    /// Follows the session until stopped (`Ok`), or until the connection fails or the session
    /// is caught missing moves ([`SilenceCheck`]); then `Err`, and the sampler polls from then
    /// on. It blocks on the socket and the stop signal, waking for a still pointer's repeat and
    /// for the silence check.
    fn listen(&mut self, ipc: &[u8], stop: &StopSignal, keeper: &mut Keeper) -> Result<(), String> {
        // Off the output as it opened: where IPC put it, until the session speaks.
        if let Some(initial) = self.initial.take() {
            keeper.offer(monotonic_us(), initial);
        }
        let mut silence = SilenceCheck::new(monotonic_us());
        let result = loop {
            let now = monotonic_us();
            let events = std::mem::take(&mut self.state.events);
            if !events.is_empty() {
                silence.heard(now);
            }
            self.tracker.take(&events, ipc, now, keeper);
            if silence.due <= now {
                let pointer = query_cursor(ipc);
                if silence.answer(now, pointer.map(|pointer| self.tracker.agrees(pointer))).is_err() {
                    if let Some(pointer) = pointer {
                        keeper.offer(now, pointer);
                    }
                    break Err("the cursor session missed moves IPC saw".into());
                }
            }
            if keeper.repeat_due().is_some_and(|due| due <= now) {
                keeper.repeat(now);
            }
            match self.connection.flush() {
                Ok(()) => {}
                Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => break Err(format!("cursor session connection failed: {error}")),
            }
            let Some(guard) = self.queue.prepare_read() else {
                if let Err(error) = self.queue.dispatch_pending(&mut self.state) {
                    break Err(format!("cursor session dispatch failed: {error}"));
                }
                continue;
            };
            let due = keeper.repeat_due().map_or(silence.due, |repeat| repeat.min(silence.due));
            let timeout = Some(due - monotonic_us());
            let (stopped, readable) = wait(stop, Some(guard.connection_fd().as_raw_fd()), timeout);
            if stopped {
                break Ok(());
            }
            if readable {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => break Err(format!("cursor session connection failed: {error}")),
                }
            }
            if let Err(error) = self.queue.dispatch_pending(&mut self.state) {
                break Err(format!("cursor session dispatch failed: {error}"));
            }
        };
        self.close();
        result
    }

    /// Destroys the session, the source and the managers (the pointer and the seat, bound at
    /// version 1, have no destructor requests; the connection closing ends them).
    fn close(&mut self) {
        self.session.destroy();
        self.source.destroy();
        if let Some(manager) = self.state.capture_manager.take() {
            manager.destroy();
        }
        if let Some(manager) = self.state.source_manager.take() {
            manager.destroy();
        }
        let _ = self.connection.flush();
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

    fn kept(keeper: &Keeper) -> Vec<(i64, f64, f64)> {
        let samples = keeper.samples.lock().unwrap();
        samples.iter().map(|sample| (sample.monotonic_us, sample.logical_position.x, sample.logical_position.y)).collect()
    }

    #[test]
    fn moves_are_kept_and_a_still_pointer_every_quarter_second() {
        let mut keeper = Keeper::new(Arc::default());
        assert_eq!(keeper.repeat_due(), None);
        keeper.repeat(10);
        assert!(kept(&keeper).is_empty());
        keeper.offer(1000, PointF::new(5.0, 6.0));
        keeper.offer(2000, PointF::new(5.0, 6.0));
        keeper.offer(3000, PointF::new(7.0, 6.0));
        assert_eq!(keeper.repeat_due(), Some(253_000));
        keeper.repeat(252_999);
        keeper.repeat(253_000);
        keeper.offer(253_001, PointF::new(7.0, 6.0));
        assert_eq!(kept(&keeper), [(1000, 5.0, 6.0), (3000, 7.0, 6.0), (253_000, 7.0, 6.0)]);
    }

    #[test]
    fn session_events_become_global_positions() {
        let ipc = b"/nonexistent/omareel-cursor-test.sock";
        let mut keeper = Keeper::new(Arc::default());
        let mut tracker = Tracker::new(RectF::new(1920.0, 0.0, 1280.0, 720.0));
        // An enter without a position: Hyprland's starting 0,0.
        tracker.take(&[SessionEvent::Enter], ipc, 10, &mut keeper);
        tracker.take(&[SessionEvent::Position(100, 50), SessionEvent::Position(101, 52)], ipc, 20, &mut keeper);
        // A position before the leave is dropped; IPC (unreachable here) gives nothing to keep.
        tracker.take(&[SessionEvent::Position(5, 5), SessionEvent::Leave], ipc, 30, &mut keeper);
        assert!(!tracker.entered);
        // Back where it left: Hyprland sends no position equal to its last.
        tracker.take(&[SessionEvent::Enter], ipc, 300_000, &mut keeper);
        assert_eq!(kept(&keeper), [(10, 1920.0, 0.0), (20, 2021.0, 52.0), (300_000, 1925.0, 5.0)]);
        assert!(tracker.entered);
    }

    #[test]
    fn ipc_agrees_with_the_session_within_a_pixel_or_off_the_output() {
        let mut tracker = Tracker::new(RectF::new(1920.0, 0.0, 1280.0, 720.0));
        let mut keeper = Keeper::new(Arc::default());
        let ipc = b"/nonexistent/omareel-cursor-test.sock";
        tracker.take(&[SessionEvent::Enter, SessionEvent::Position(100, 50)], ipc, 10, &mut keeper);
        assert!(tracker.agrees(PointF::new(2020.0, 50.0)));
        assert!(tracker.agrees(PointF::new(2021.0, 49.0)));
        // A move the output never committed.
        assert!(!tracker.agrees(PointF::new(2300.0, 50.0)));
        tracker.take(&[SessionEvent::Leave], ipc, 20, &mut keeper);
        assert!(tracker.agrees(PointF::new(100.0, 100.0)));
        // Back on the output without an enter.
        assert!(!tracker.agrees(PointF::new(2000.0, 100.0)));
    }

    #[test]
    fn a_session_is_given_up_after_two_disagreements_in_a_row() {
        let mut check = SilenceCheck::new(0);
        assert_eq!(check.due, SILENCE_CHECK_US);
        // Agreement, or no answer, waits another second.
        assert_eq!(check.answer(1_000_000, Some(true)), Ok(()));
        assert_eq!(check.due, 2_000_000);
        assert_eq!(check.answer(2_000_000, None), Ok(()));
        assert_eq!(check.due, 3_000_000);
        // A disagreement is asked again soon; the session speaking in between clears it.
        assert_eq!(check.answer(3_000_000, Some(false)), Ok(()));
        assert_eq!(check.due, 3_000_000 + RECHECK_US);
        check.heard(3_050_000);
        assert!(!check.suspect);
        assert_eq!(check.due, 4_050_000);
        assert_eq!(check.answer(4_050_000, Some(false)), Ok(()));
        assert_eq!(check.answer(4_150_000, Some(false)), Err(()));
    }

    #[test]
    fn a_silent_session_is_believed_only_with_the_pointer_elsewhere() {
        let output = RectF::new(1920.0, 0.0, 1280.0, 720.0);
        assert!(session_answered(true, None, output));
        assert!(!session_answered(false, None, output));
        assert!(!session_answered(false, Some(PointF::new(1920.0, 0.0)), output));
        assert!(!session_answered(false, Some(PointF::new(3199.0, 719.0)), output));
        assert!(session_answered(false, Some(PointF::new(3200.0, 100.0)), output));
        assert!(session_answered(false, Some(PointF::new(100.0, 100.0)), output));
    }

    #[test]
    fn polling_follows_the_frame_rate() {
        assert_eq!(polling_interval(30), Duration::from_micros(16_666));
        assert_eq!(polling_interval(60), Duration::from_micros(16_666));
        assert_eq!(polling_interval(120), Duration::from_micros(8_333));
        assert_eq!(polling_interval(1000), Duration::from_micros(4_166));
    }

    #[test]
    fn stopping_wakes_a_waiting_thread() {
        let stop = Arc::new(StopSignal::new().unwrap());
        assert_eq!(wait(&stop, None, Some(0)), (false, false));
        let waiter = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || wait(&stop, None, None))
        };
        std::thread::sleep(Duration::from_millis(20));
        stop.raise();
        assert_eq!(waiter.join().unwrap(), (true, false));
    }
}
