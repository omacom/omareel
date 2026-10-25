//! What `RecordingBar.cpp` computes: when the standby self-view host quits
//! (`selfViewHostShouldExit`), where a dragged bubble goes (`clampedSelfViewDragPosition`), the
//! elapsed time and countdown `poll` shows, the camera shape cycle, the bubble's corners,
//! clamping and the position it saves, and the standby host's first placement. The
//! `RecordingBar` QObject owns the camera, the file watcher and the timers, and does the saving,
//! state-file writes and signals around these.

use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qjson::{self, Object, Value};
use omareel_core::compat::qprocess::Process;
use omareel_core::compat::qround::{q_bound, q_max, q_round};
use omareel_core::recording_preferences::RecordingPreferences;

/// `QPoint`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Point {
        Point { x, y }
    }
}

/// `QSize`. `QSize()` is -1 x -1 ([`Size::INVALID`], also the default), and a size is empty
/// when either side is below 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub const INVALID: Size = Size::new(-1, -1);

    pub const fn new(width: i32, height: i32) -> Size {
        Size { width, height }
    }

    /// `QSize::isEmpty`.
    pub fn is_empty(self) -> bool {
        self.width < 1 || self.height < 1
    }
}

impl Default for Size {
    fn default() -> Size {
        Size::INVALID
    }
}

/// `selfViewHostShouldExit` (inline in `RecordingBar.h`): the standby host quits when no
/// recording adopted it and it was told to quit, has no owner, or its owner died.
pub fn self_view_host_should_exit(
    recording_active: bool,
    quit_requested: bool,
    owner_pid: i64,
    owner_alive: bool,
) -> bool {
    !recording_active && (quit_requested || owner_pid <= 0 || !owner_alive)
}

/// `clampedSelfViewDragPosition` (inline in `RecordingBar.h`): the bubble moved by the pointer's
/// travel since the press (rounded with `qRound`), kept on the screen.
pub fn clamped_self_view_drag_position(
    press_pointer: PointF,
    press_bubble: Point,
    pointer: PointF,
    screen_size: Size,
    bubble_pixels: i32,
) -> Point {
    let delta = Point::new(q_round(pointer.x - press_pointer.x), q_round(pointer.y - press_pointer.y));
    Point::new(
        q_bound(0, press_bubble.x + delta.x, q_max(0, screen_size.width - bubble_pixels)),
        q_bound(0, press_bubble.y + delta.y, q_max(0, screen_size.height - bubble_pixels)),
    )
}

/// The elapsed time `poll` shows, `MM:SS` (minutes growing past two digits), from the bar's
/// monotonic clock and the recording's `started_us`.
pub fn elapsed_text(now_us: i64, started_us: i64) -> String {
    let seconds = (now_us - started_us).max(0) / 1_000_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// The countdown digit `poll` shows: the seconds left until `countdown_end_us`, rounded up and
/// kept between 1 and 3.
pub fn countdown_value(countdown_end_us: i64, now_us: i64) -> i32 {
    // int(...) of the 64-bit quotient, as the C++ converts it, then std::clamp.
    ((((countdown_end_us - now_us).max(0) + 999_999) / 1_000_000) as i32).clamp(1, 3)
}

/// The camera's shape in the project's vocabulary: `round`, or `square` with a corner radius
/// (0 for sharp corners). `RecordingBar`'s `m_cameraShape` and `m_cameraRadius`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraShape {
    pub shape: String,
    pub radius: i32,
}

impl Default for CameraShape {
    fn default() -> CameraShape {
        CameraShape { shape: "round".into(), radius: 16 }
    }
}

impl CameraShape {
    /// The shape the bar starts with (the constructor): the preferences' `webcam.shape` when it
    /// is `square`, else round, and `webcam.radius` (16 when it is not an int) kept in `[0, 80]`.
    pub fn from_preferences(preferences: &RecordingPreferences) -> CameraShape {
        let shape = if preferences.webcam.value("shape").to_string_or("") == "square" { "square" } else { "round" };
        CameraShape { shape: shape.into(), radius: q_bound(0, preferences.webcam.value("radius").to_int(16), 80) }
    }

    /// The start of `cycleCameraShape`: round, then square with rounded corners, then square
    /// with sharp corners. (The bar then saves the shape and tells the recorder.)
    pub fn cycle(&mut self) {
        if self.shape == "round" {
            self.shape = "square".into();
            if self.radius == 0 {
                self.radius = 16;
            }
        } else if self.radius > 0 {
            self.radius = 0;
        } else {
            self.shape = "round".into();
            self.radius = 16;
        }
    }
}

/// Where the self-view bubble sits on its screen: `RecordingBar`'s `m_selfViewScreenSize`,
/// `m_selfViewPixels`, `m_selfViewX`, `m_selfViewY` and `m_corner`, with the arithmetic of the
/// methods that move it. The bar adds the timers, the state-file writes and the signals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfViewGeometry {
    pub screen_size: Size,
    pub pixels: i32,
    pub x: i32,
    pub y: i32,
    /// The corner `snap_to_next_corner` went to last: 0 top left, 1 top right, 2 bottom left,
    /// 3 bottom right.
    pub corner: i32,
}

impl Default for SelfViewGeometry {
    fn default() -> SelfViewGeometry {
        SelfViewGeometry { screen_size: Size::INVALID, pixels: 160, x: 16, y: 16, corner: 3 }
    }
}

impl SelfViewGeometry {
    fn max_x(&self) -> i32 {
        (self.screen_size.width - self.pixels).max(0)
    }

    fn max_y(&self) -> i32 {
        (self.screen_size.height - self.pixels).max(0)
    }

    /// `setSelfViewScreenSize`: the bubble kept on the new screen. (The bar then announces the
    /// placement.)
    pub fn set_screen_size(&mut self, size: Size) {
        self.screen_size = size;
        self.x = self.x.clamp(0, self.max_x());
        self.y = self.y.clamp(0, self.max_y());
    }

    /// `setSelfViewPosition`: moves the bubble there, kept on the screen; true when it moved,
    /// which is when the bar starts its write and save timers. Nothing happens before the
    /// screen's size is known.
    pub fn set_position(&mut self, x: i32, y: i32) -> bool {
        if self.screen_size.is_empty() {
            return false;
        }
        let next_x = x.clamp(0, self.max_x());
        let next_y = y.clamp(0, self.max_y());
        if next_x == self.x && next_y == self.y {
            return false;
        }
        self.x = next_x;
        self.y = next_y;
        true
    }

    /// The position `poll` takes from the recorder's state (`selfview_x`, `selfview_y`): kept
    /// on the screen once its size is known.
    pub fn clamped_requested_position(&self, requested_x: i32, requested_y: i32) -> Point {
        if self.screen_size.is_empty() {
            return Point::new(requested_x, requested_y);
        }
        Point::new(requested_x.clamp(0, self.max_x()), requested_y.clamp(0, self.max_y()))
    }

    /// `snapSelfViewToNextCorner`: the next corner clockwise from the top left, 24 pixels in; the
    /// result of [`set_position`](Self::set_position). Nothing happens before the screen's size
    /// is known.
    pub fn snap_to_next_corner(&mut self) -> bool {
        if self.screen_size.is_empty() {
            return false;
        }
        self.corner = (self.corner + 1) % 4;
        let margin = 24;
        let max_x = margin.max(self.screen_size.width - self.pixels - margin);
        let max_y = margin.max(self.screen_size.height - self.pixels - margin);
        let x = if self.corner == 0 || self.corner == 2 { margin } else { max_x };
        let y = if self.corner == 0 || self.corner == 1 { margin } else { max_y };
        self.set_position(x, y)
    }

    /// What `saveSelfViewPosition` stores as `selfViewX` and `selfViewY`: the position as a
    /// fraction of the travel 16 pixels in from each edge, in `[0, 1]`; `None` before the
    /// screen's size is known. (The bar also skips saving while an adopted host shows the
    /// fallback placement.)
    pub fn saved_position(&self) -> Option<(f64, f64)> {
        if self.screen_size.is_empty() {
            return None;
        }
        let x_range = (self.screen_size.width - self.pixels - 32).max(1);
        let y_range = (self.screen_size.height - self.pixels - 32).max(1);
        Some((
            ((f64::from(self.x) - 16.0) / f64::from(x_range)).clamp(0.0, 1.0),
            ((f64::from(self.y) - 16.0) / f64::from(y_range)).clamp(0.0, 1.0),
        ))
    }
}

/// The bubble's size for a preference size, as the constructor and `hostCommand` spell it out
/// inline: the recorder's `selfViewPixels` (120 for `S`, 220 for `L`, 160 otherwise).
pub use crate::recorder::self_view::self_view_pixels;

/// The standby host's first placement (the constructor with `standby`): the monitor it shows
/// on, and the state it starts from in place of the recorder's.
#[derive(Clone, Debug, PartialEq)]
pub struct StandbyPlacement {
    /// `m_recordedMonitor`: the focused monitor (or the first), "" without one.
    pub recorded_monitor: String,
    pub state: Object,
}

/// The standby placement from what `hyprctl -j monitors` printed (`None` when it did not
/// finish): the focused monitor, or the first one; the bubble placed by the preferences' saved
/// fractions on its logical size.
pub fn standby_placement_from(monitors_output: Option<&[u8]>, preferences: &RecordingPreferences) -> StandbyPlacement {
    let mut monitor = Object::new();
    if let Some(output) = monitors_output {
        if let Ok(Value::Array(values)) = qjson::from_json(output) {
            for value in &values {
                let focused = value.to_object().value("focused").to_bool(false);
                if monitor.is_empty() || focused {
                    monitor = value.to_object();
                }
                if focused {
                    break;
                }
            }
        }
    }
    let scale = q_max(0.01, monitor.value("scale").to_double(1.0));
    let pixels = self_view_pixels(&preferences.self_view_size);
    let recorded_monitor = monitor.value("name").to_string_or("");
    let travel = |extent: &str| q_max(0, q_round(monitor.value(extent).to_double(0.0) / scale) - pixels - 32);
    let mut state = Object::new();
    state.insert("camera_rotation", preferences.webcam_rotation);
    state.insert("camera_flip_horizontal", preferences.webcam_flip_horizontal);
    state.insert("selfview", preferences.self_view_enabled);
    state.insert("selfview_safe", true);
    state.insert("selfview_monitor", recorded_monitor.as_str());
    state.insert("selfview_pixels", pixels);
    state.insert("camera_device", preferences.webcam_device.as_str());
    state.insert("camera_height", preferences.webcam_height);
    state.insert("selfview_x", 16 + q_round(preferences.self_view_x * f64::from(travel("width"))));
    state.insert("selfview_y", 16 + q_round(preferences.self_view_y * f64::from(travel("height"))));
    StandbyPlacement { recorded_monitor, state }
}

/// The standby placement, asking `hyprctl -j monitors` (one second; the exit status is not
/// looked at).
pub fn standby_placement(preferences: &RecordingPreferences) -> StandbyPlacement {
    let mut monitors = Process::new();
    monitors.start("hyprctl", &["-j", "monitors"]);
    let output = monitors.wait_for_finished(1000).then(|| monitors.read_all_standard_output());
    standby_placement_from(output.as_deref(), preferences)
}

/// `CursorSampler::cursorPosition` (`src/record/CursorSampler.cpp`), which `globalCursorPos`
/// asks: the pointer in global logical coordinates from Hyprland's `j/cursorpos` on
/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`; `None` when the socket does
/// not answer with an object holding `x` and `y`.
pub fn global_cursor_position() -> Option<PointF> {
    let variable = |name: &str| std::env::var_os(name).unwrap_or_default();
    let mut path = variable("XDG_RUNTIME_DIR");
    path.push("/hypr/");
    path.push(variable("HYPRLAND_INSTANCE_SIGNATURE"));
    path.push("/.socket.sock");
    query_cursor(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()))
}

/// `queryCursor(path, &point)`: one request with 20 ms send and receive timeouts, and at most 16
/// reads of 256 bytes for the reply.
pub fn query_cursor(path: &[u8]) -> Option<PointF> {
    // SAFETY: socket returns a new descriptor or -1.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return None;
    }
    // SAFETY: a new descriptor nothing else owns; it is closed when this returns.
    let socket = unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd) };
    let timeout = libc::timeval { tv_sec: 0, tv_usec: 20_000 };
    let timeout_size = std::mem::size_of::<libc::timeval>() as libc::socklen_t;
    // SAFETY: the option value is a timeval of the size given.
    unsafe {
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO, (&raw const timeout).cast(), timeout_size);
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_SNDTIMEO, (&raw const timeout).cast(), timeout_size);
    }
    // SAFETY: sockaddr_un is plain data, valid zeroed.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    if path.len() >= address.sun_path.len() {
        return None;
    }
    for (slot, &byte) in address.sun_path.iter_mut().zip(path) {
        *slot = byte as libc::c_char;
    }
    let address_size = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    // SAFETY: the address is a sockaddr_un of the size given.
    if unsafe { libc::connect(fd, (&raw const address).cast(), address_size) } != 0 {
        return None;
    }
    let request = b"j/cursorpos";
    // SAFETY: the buffer and its length are passed together.
    if unsafe { libc::write(fd, request.as_ptr().cast(), request.len()) } != request.len() as isize {
        return None;
    }
    let mut reply = Vec::new();
    let mut buffer = [0u8; 256];
    for _ in 0..16 {
        // SAFETY: the buffer and its length are passed together.
        let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count <= 0 {
            break;
        }
        reply.extend_from_slice(&buffer[..count as usize]);
    }
    drop(socket);
    let object = match qjson::from_json(&reply) {
        Ok(Value::Object(object)) => object,
        _ => Object::new(),
    };
    if !object.contains("x") || !object.contains("y") {
        return None;
    }
    Some(PointF::new(object.value("x").to_double(0.0), object.value("y").to_double(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // test_recorder.cpp's selfViewHostLifetime.
    #[test]
    fn self_view_host_lifetime() {
        for active in [false, true] {
            for quit in [false, true] {
                for owner in [-1, 0, 42] {
                    for alive in [false, true] {
                        let expected = if active { false } else { quit || owner <= 0 || !alive };
                        assert_eq!(self_view_host_should_exit(active, quit, owner, alive), expected);
                    }
                }
            }
        }
        // Completing an adoption changes only active: a dead owner must exit,
        // a live owner returns to standby, and quit waits for recording to finish.
        assert!(!self_view_host_should_exit(true, false, 42, false));
        assert!(self_view_host_should_exit(false, false, 42, false));
        assert!(!self_view_host_should_exit(false, false, 42, true));
        assert!(!self_view_host_should_exit(true, true, 42, true));
        assert!(self_view_host_should_exit(false, true, 42, true));
    }

    // test_recorder.cpp's selfViewDragUsesStableGlobalCoordinates.
    #[test]
    fn self_view_drag_uses_stable_global_coordinates() {
        let screen = Size::new(2400, 1350);
        let press = PointF::new(100.0, 100.0);
        assert_eq!(
            clamped_self_view_drag_position(press, Point::new(400, 300), PointF::new(160.0, 140.0), screen, 160),
            Point::new(460, 340)
        );
        assert_eq!(
            clamped_self_view_drag_position(press, Point::new(2200, 1200), PointF::new(500.0, 500.0), screen, 160),
            Point::new(2240, 1190)
        );
        assert_eq!(
            clamped_self_view_drag_position(press, Point::new(20, 20), PointF::new(-100.0, -100.0), screen, 160),
            Point::new(0, 0)
        );
    }

    #[test]
    fn cursor_query_without_a_socket() {
        assert_eq!(query_cursor(b"/nonexistent/omareel/.socket.sock"), None);
        assert_eq!(query_cursor(&[b'x'; 200]), None);
    }
}
