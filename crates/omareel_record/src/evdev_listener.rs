//! `EvdevListener` (`src/record/EvdevListener.cpp` at the tag): the mouse buttons, scroll wheels
//! and keys pressed while a recording runs, read from `/dev/input/event*` through libevdev on a
//! thread of its own, for the click zooms and the keystroke overlay. Devices this user may not
//! read are skipped; without any the recording has no clicks or keys.
//!
//! Key names are libevdev's (`libevdev_event_code_get_name`), and the decisions per event are
//! [`device_event`], so they can be checked without a device.

use crate::capture::{DeviceEvent, DeviceEventKind};
use crate::recorder::monotonic_us;
use omareel_core::compat::qdir::{self, Filters, SortFlags};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

/// libevdev's device handle.
#[repr(C)]
struct Libevdev {
    _private: [u8; 0],
}

#[link(name = "evdev")]
unsafe extern "C" {
    fn libevdev_new_from_fd(fd: c_int, dev: *mut *mut Libevdev) -> c_int;
    fn libevdev_free(dev: *mut Libevdev);
    fn libevdev_has_event_code(dev: *const Libevdev, kind: c_uint, code: c_uint) -> c_int;
    fn libevdev_next_event(dev: *mut Libevdev, flags: c_uint, event: *mut libc::input_event) -> c_int;
    fn libevdev_event_code_get_name(kind: c_uint, code: c_uint) -> *const c_char;
}

const LIBEVDEV_READ_FLAG_NORMAL: c_uint = 2;

/// The event types and codes of `linux/input-event-codes.h` the listener asks about.
pub mod codes {
    pub const EV_KEY: u16 = 0x01;
    pub const EV_REL: u16 = 0x02;
    pub const KEY_ENTER: u16 = 28;
    pub const KEY_LEFTCTRL: u16 = 29;
    pub const KEY_A: u16 = 30;
    pub const KEY_LEFTSHIFT: u16 = 42;
    pub const KEY_RIGHTSHIFT: u16 = 54;
    pub const KEY_LEFTALT: u16 = 56;
    pub const KEY_SPACE: u16 = 57;
    pub const KEY_RIGHTCTRL: u16 = 97;
    pub const KEY_RIGHTALT: u16 = 100;
    pub const KEY_LEFTMETA: u16 = 125;
    pub const KEY_RIGHTMETA: u16 = 126;
    pub const BTN_MISC: u16 = 0x100;
    pub const BTN_LEFT: u16 = 0x110;
    pub const BTN_RIGHT: u16 = 0x111;
    pub const BTN_MIDDLE: u16 = 0x112;
    pub const REL_HWHEEL: u16 = 0x06;
    pub const REL_WHEEL: u16 = 0x08;
    pub const REL_WHEEL_HI_RES: u16 = 0x0b;
    pub const REL_HWHEEL_HI_RES: u16 = 0x0c;
}
use codes::*;

/// `libevdev_event_code_get_name(EV_KEY, code)` as `QString::fromLatin1` reads it; empty when
/// libevdev has no name for the code.
pub fn key_name(code: u16) -> String {
    // SAFETY: libevdev returns a static string or null.
    let name = unsafe { libevdev_event_code_get_name(c_uint::from(EV_KEY), c_uint::from(code)) };
    if name.is_null() {
        return String::new();
    }
    // SAFETY: a NUL-terminated static string.
    unsafe { CStr::from_ptr(name) }.to_bytes().iter().map(|&b| char::from(b)).collect()
}

/// The file-static `buttonName`.
fn button_name(code: u16) -> &'static str {
    match code {
        BTN_LEFT => "left",
        BTN_RIGHT => "right",
        BTN_MIDDLE => "middle",
        _ => "",
    }
}

/// The file-static `modifierName`.
fn modifier_name(code: u16) -> &'static str {
    match code {
        KEY_LEFTCTRL | KEY_RIGHTCTRL => "ctrl",
        KEY_LEFTSHIFT | KEY_RIGHTSHIFT => "shift",
        KEY_LEFTALT | KEY_RIGHTALT => "alt",
        KEY_LEFTMETA | KEY_RIGHTMETA => "meta",
        _ => "",
    }
}

/// What `EvdevListener::run` makes of one event from a device (`keyboard` when the device has
/// A, Enter or Space): a vertical or horizontal wheel step is a scroll (`dy` the negated
/// wheel value, `dx` the horizontal one); a press or release (not a repeat) of the left, right
/// or middle button is a button event from any device; any other key below `BTN_MISC` on a
/// keyboard is a key event, named by libevdev, whose press carries the modifiers held then.
/// `modifiers` follows ctrl, shift, alt and meta as they go down and up, across devices.
/// `None` for everything else.
pub fn device_event(
    keyboard: bool,
    kind: u16,
    code: u16,
    value: i32,
    monotonic_us: i64,
    modifiers: &mut Vec<String>,
) -> Option<DeviceEvent> {
    let mut output = DeviceEvent { monotonic_us, ..DeviceEvent::default() };
    if kind == EV_REL && (code == REL_WHEEL || code == REL_HWHEEL) {
        output.kind = DeviceEventKind::Scroll;
        if code == REL_WHEEL {
            output.dy = value.wrapping_neg();
        } else {
            output.dx = value;
        }
        return Some(output);
    }
    if kind != EV_KEY || value == 2 {
        return None;
    }
    output.button = button_name(code).to_owned();
    if !output.button.is_empty() {
        output.kind = if value != 0 { DeviceEventKind::ButtonDown } else { DeviceEventKind::ButtonUp };
        return Some(output);
    }
    if !keyboard || code >= BTN_MISC {
        return None;
    }
    let modifier = modifier_name(code);
    if !modifier.is_empty() {
        if value != 0 && !modifiers.iter().any(|held| held == modifier) {
            modifiers.push(modifier.to_owned());
        }
        if value == 0 {
            modifiers.retain(|held| held != modifier);
        }
    }
    output.kind = if value != 0 { DeviceEventKind::KeyDown } else { DeviceEventKind::KeyUp };
    output.key_code = i32::from(code);
    output.key_name = key_name(code);
    if value != 0 {
        output.modifiers = modifiers.clone();
    }
    Some(output)
}

/// An open device.
struct Device {
    fd: c_int,
    evdev: *mut Libevdev,
    keyboard: bool,
}

// SAFETY: a device is used by one thread at a time: the listener's while it runs, then the
// owner's to free it.
unsafe impl Send for Device {}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: both were made by `start` and are freed once.
        unsafe {
            libevdev_free(self.evdev);
            libc::close(self.fd);
        }
    }
}

fn has_code(evdev: *const Libevdev, kind: u16, code: u16) -> bool {
    // SAFETY: a device libevdev made.
    unsafe { libevdev_has_event_code(evdev, c_uint::from(kind), c_uint::from(code)) != 0 }
}

/// The file-static `isKeyboard`.
fn is_keyboard(evdev: *const Libevdev) -> bool {
    has_code(evdev, EV_KEY, KEY_A) || has_code(evdev, EV_KEY, KEY_ENTER) || has_code(evdev, EV_KEY, KEY_SPACE)
}

/// `EvdevListener`: the devices it reads and the events it kept.
#[derive(Default)]
pub struct EvdevListener {
    running: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<DeviceEvent>>>,
    devices: Vec<Device>,
    thread: Option<JoinHandle<Vec<Device>>>,
}

impl EvdevListener {
    pub fn new() -> EvdevListener {
        EvdevListener::default()
    }

    /// `start(&warning)`: opens every readable `/dev/input/event*` (in `QDir`'s order) that is a
    /// keyboard or has a left, right or middle button or a wheel, and starts reading them; the
    /// warning when there is none.
    pub fn start(&mut self) -> Result<(), String> {
        let input = "/dev/input";
        for name in qdir::entry_list(input, &["event*"], Filters::SYSTEM | Filters::FILES, SortFlags::NO_SORT) {
            let Ok(path) = CString::new(qdir::file_path(input, &name)) else { continue };
            // SAFETY: a NUL-terminated path.
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
            if fd < 0 {
                continue;
            }
            let mut evdev = std::ptr::null_mut();
            // SAFETY: an open descriptor and a place for the handle.
            if unsafe { libevdev_new_from_fd(fd, &mut evdev) } < 0 {
                // SAFETY: closes the descriptor opened above.
                unsafe { libc::close(fd) };
                continue;
            }
            let device = Device { fd, evdev, keyboard: is_keyboard(evdev) };
            let useful = device.keyboard
                || has_code(evdev, EV_KEY, BTN_LEFT)
                || has_code(evdev, EV_KEY, BTN_RIGHT)
                || has_code(evdev, EV_KEY, BTN_MIDDLE)
                || has_code(evdev, EV_REL, REL_WHEEL)
                || has_code(evdev, EV_REL, REL_HWHEEL)
                || has_code(evdev, EV_REL, REL_WHEEL_HI_RES)
                || has_code(evdev, EV_REL, REL_HWHEEL_HI_RES);
            if useful {
                self.devices.push(device);
            }
        }
        if self.devices.is_empty() {
            return Err("No readable keyboard or pointer evdev devices; recording without clicks/keys".into());
        }
        self.running.store(true, Ordering::SeqCst);
        let devices = std::mem::take(&mut self.devices);
        let running = Arc::clone(&self.running);
        let events = Arc::clone(&self.events);
        self.thread = Some(std::thread::spawn(move || run(devices, &running, &events)));
        Ok(())
    }

    /// `stop()`: ends reading, waits for the thread and closes the devices.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            if let Ok(devices) = thread.join() {
                drop(devices);
            }
        }
        self.devices.clear();
    }

    /// `events()`: a copy of what was kept, in the order read.
    pub fn events(&self) -> Vec<DeviceEvent> {
        self.events.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for EvdevListener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `EvdevListener::run`: polls every device (100 ms at a time) and drains each that is ready
/// while libevdev hands out events normally; gives the devices back when stopped.
fn run(devices: Vec<Device>, running: &AtomicBool, events: &Mutex<Vec<DeviceEvent>>) -> Vec<Device> {
    let mut descriptors: Vec<libc::pollfd> =
        devices.iter().map(|device| libc::pollfd { fd: device.fd, events: libc::POLLIN, revents: 0 }).collect();
    let mut modifiers = Vec::new();
    while running.load(Ordering::SeqCst) {
        // SAFETY: the pollfds of open devices.
        let ready = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as libc::nfds_t, 100) };
        if ready <= 0 {
            continue;
        }
        for (descriptor, device) in descriptors.iter().zip(&devices) {
            if descriptor.revents & libc::POLLIN == 0 {
                continue;
            }
            // SAFETY: an all-zero input_event is valid.
            let mut event: libc::input_event = unsafe { std::mem::zeroed() };
            // SAFETY: a device libevdev made and a place for the event.
            while unsafe { libevdev_next_event(device.evdev, LIBEVDEV_READ_FLAG_NORMAL, &mut event) } == 0 {
                let output =
                    device_event(device.keyboard, event.type_, event.code, event.value, monotonic_us(), &mut modifiers);
                if let Some(output) = output {
                    events.lock().unwrap_or_else(PoisonError::into_inner).push(output);
                }
            }
        }
    }
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_libevdevs() {
        assert_eq!(key_name(KEY_A), "KEY_A");
        assert_eq!(key_name(KEY_LEFTMETA), "KEY_LEFTMETA");
        assert_eq!(key_name(KEY_ENTER), "KEY_ENTER");
        assert_eq!(key_name(0x2ff), "KEY_MAX");
        assert_eq!(key_name(0x300), "");
    }

    #[test]
    fn events_follow_the_listener() {
        let mut modifiers = Vec::new();
        let mut event = |keyboard, kind, code, value| device_event(keyboard, kind, code, value, 7, &mut modifiers);
        let scroll = event(false, EV_REL, REL_WHEEL, 1).unwrap();
        assert_eq!((scroll.kind, scroll.dx, scroll.dy, scroll.monotonic_us), (DeviceEventKind::Scroll, 0, -1, 7));
        let horizontal = event(false, EV_REL, REL_HWHEEL, -2).unwrap();
        assert_eq!((horizontal.dx, horizontal.dy), (-2, 0));
        assert!(event(false, EV_REL, REL_WHEEL_HI_RES, 120).is_none());
        let click = event(false, EV_KEY, BTN_LEFT, 1).unwrap();
        assert_eq!((click.kind, click.button.as_str()), (DeviceEventKind::ButtonDown, "left"));
        assert_eq!(event(true, EV_KEY, BTN_MIDDLE, 0).unwrap().kind, DeviceEventKind::ButtonUp);
        assert!(event(false, EV_KEY, KEY_A, 1).is_none(), "keys only from keyboards");
        assert!(event(true, EV_KEY, KEY_A, 2).is_none(), "no repeats");
        assert!(event(true, EV_KEY, 0x113, 1).is_none(), "side buttons are not keys");
        let ctrl = event(true, EV_KEY, KEY_LEFTCTRL, 1).unwrap();
        assert_eq!((ctrl.kind, ctrl.key_code, ctrl.key_name.as_str()), (DeviceEventKind::KeyDown, 29, "KEY_LEFTCTRL"));
        assert_eq!(ctrl.modifiers, ["ctrl"]);
        event(true, EV_KEY, KEY_RIGHTCTRL, 1);
        event(true, EV_KEY, KEY_LEFTSHIFT, 1);
        let a = event(true, EV_KEY, KEY_A, 1).unwrap();
        assert_eq!(a.modifiers, ["ctrl", "shift"]);
        // Either ctrl going up lets go of ctrl.
        let up = event(true, EV_KEY, KEY_RIGHTCTRL, 0).unwrap();
        assert_eq!((up.kind, up.modifiers.len()), (DeviceEventKind::KeyUp, 0));
        assert_eq!(event(true, EV_KEY, KEY_A, 1).unwrap().modifiers, ["shift"]);
        assert!(event(true, 0x03, 0, 5).is_none());
    }
}
