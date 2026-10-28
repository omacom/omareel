//! `EvdevListener` (`src/record/EvdevListener.cpp` at the tag): the mouse buttons, scroll wheels
//! and keys pressed while a recording runs, read from `/dev/input/event*` through libevdev on a
//! thread of its own, for the click zooms and the keystroke overlay. Devices this user may not
//! read are skipped; without any the recording has no clicks or keys.
//!
//! Plain typing is never kept, so passwords typed while recording stay out of the bundle: only
//! shortcuts (keys pressed with Ctrl, Alt or Super held, and those modifiers themselves) and keys
//! that type no text (Enter, Escape, Tab, arrows, function and media keys...) are, Shift with
//! any of these included. Shift, AltGr and Caps Lock alone count as typing, since they choose
//! what is typed. What is typing is decided by the user's layout ([`crate::keymap`]), read from
//! Hyprland when the listener starts, and a key kept carries what that layout prints on it as
//! its label next to libevdev's name of its scancode (`libevdev_event_code_get_name`). Without
//! a layout the listener decides by those names, as for a US keyboard.
//!
//! Events are timed by the kernel when it queued them (`CLOCK_MONOTONIC`, the recorder's clock),
//! not when they were read, unless a device will not take that clock.
//!
//! The decisions per event are [`device_event`], so they can be checked without a device.

use crate::capture::{DeviceEvent, DeviceEventKind};
use crate::keymap::{self, Key, Layout};
use crate::recorder::monotonic_us;
use omareel_core::compat::qdir::{self, Filters, SortFlags};
use omareel_core::compat::qlog;
use omareel_core::keystroke_track::{is_typing_key_name, makes_shortcut};
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
    fn libevdev_set_clock_id(dev: *mut Libevdev, clock: c_int) -> c_int;
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

/// The file-static `modifierName`, for when there is no layout. Right Alt is left out: on many
/// layouts it is AltGr, which types characters rather than making shortcuts.
fn modifier_name(code: u16) -> &'static str {
    match code {
        KEY_LEFTCTRL | KEY_RIGHTCTRL => "ctrl",
        KEY_LEFTSHIFT | KEY_RIGHTSHIFT => "shift",
        KEY_LEFTALT => "alt",
        KEY_LEFTMETA | KEY_RIGHTMETA => "meta",
        _ => "",
    }
}

/// The keyboard as the listener follows it, across devices: the layout when there is one, the
/// modifiers held (ctrl, shift, alt, meta, in the order they went down) and the keys whose press
/// was kept, so that only their releases are.
#[derive(Default)]
pub struct Keyboard {
    layout: Option<Layout>,
    modifiers: Vec<String>,
    kept: Vec<u16>,
}

impl Keyboard {
    /// A keyboard with nothing held, read through `layout`, or by US key names without one.
    pub fn new(layout: Option<Layout>) -> Keyboard {
        Keyboard { layout, ..Keyboard::default() }
    }

    /// What the key `code` is ([`Layout::key`], which the press or release then updates); without
    /// a layout, no label, text by its libevdev name ([`is_typing_key_name`]) and the modifier by
    /// its code.
    fn key(&mut self, code: u16, down: bool) -> Key {
        match &mut self.layout {
            Some(layout) => layout.key(code, down),
            None => Key { label: String::new(), typing: is_typing_key_name(&key_name(code)), modifier: modifier_name(code) },
        }
    }
}

/// What `EvdevListener::run` makes of one event from a device (`keyboard_device` when the device
/// has A, Enter or Space): a vertical or horizontal wheel step is a scroll (`dy` the negated
/// wheel value, `dx` the horizontal one); a press or release (not a repeat) of the left, right
/// or middle button is a button event from any device; any other key below `BTN_MISC` on a
/// keyboard is a key event, named by libevdev and labelled by the layout, whose press carries the
/// modifiers held then. Of those, a press of a typing key ([`Key::typing`]) without Ctrl, Alt or
/// Super held is dropped, and so is its release: only shortcuts and keys that type nothing are
/// kept.
/// `keyboard` follows the layout and the modifiers as keys go down and up. `None` for everything
/// else.
pub fn device_event(
    keyboard_device: bool,
    kind: u16,
    code: u16,
    value: i32,
    monotonic_us: i64,
    keyboard: &mut Keyboard,
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
    if !keyboard_device || code >= BTN_MISC {
        return None;
    }
    let down = value != 0;
    let key = keyboard.key(code, down);
    let modifiers = &mut keyboard.modifiers;
    if !key.modifier.is_empty() {
        if down && !modifiers.iter().any(|held| held == key.modifier) {
            modifiers.push(key.modifier.to_owned());
        }
        if !down {
            modifiers.retain(|held| held != key.modifier);
        }
    }
    if down {
        if key.typing && !makes_shortcut(modifiers) {
            return None;
        }
        if !keyboard.kept.contains(&code) {
            keyboard.kept.push(code);
        }
        output.modifiers = modifiers.clone();
        output.key_label = key.label;
    } else {
        let index = keyboard.kept.iter().position(|&kept| kept == code)?;
        keyboard.kept.remove(index);
    }
    output.kind = if down { DeviceEventKind::KeyDown } else { DeviceEventKind::KeyUp };
    output.key_code = i32::from(code);
    output.key_name = key_name(code);
    Some(output)
}

/// An open device.
struct Device {
    fd: c_int,
    evdev: *mut Libevdev,
    keyboard: bool,
    /// Whether its events are stamped with `CLOCK_MONOTONIC`, so their own times can be used.
    monotonic_clock: bool,
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
            // SAFETY: a device libevdev made.
            let monotonic_clock = unsafe { libevdev_set_clock_id(evdev, libc::CLOCK_MONOTONIC) } == 0;
            let device = Device { fd, evdev, keyboard: is_keyboard(evdev), monotonic_clock };
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

/// The keyboard layout Hyprland has (an empty set of names, libxkbcommon's defaults, when it
/// does not answer); `None`, with a warning, when libxkbcommon cannot compile it.
fn hyprland_layout() -> Option<Layout> {
    let names = keymap::hyprland_keyboard().unwrap_or_default();
    let layout = Layout::new(&names);
    if layout.is_none() {
        qlog::warning(&format!(
            "Could not compile the keyboard layout \"{}\" (variant \"{}\", options \"{}\"); recording keys by their US names",
            names.layout, names.variant, names.options
        ));
    }
    layout
}

/// When an event happened, in `CLOCK_MONOTONIC` microseconds: its own time when the device
/// stamps it with that clock, else now.
fn event_time(device: &Device, event: &libc::input_event) -> i64 {
    if device.monotonic_clock {
        event.time.tv_sec * 1_000_000 + event.time.tv_usec
    } else {
        monotonic_us()
    }
}

/// `EvdevListener::run`: reads the keyboard layout, then polls every device (100 ms at a time)
/// and drains each that is ready while libevdev hands out events normally; gives the devices
/// back when stopped. Events queued while the layout is read wait in the devices.
fn run(devices: Vec<Device>, running: &AtomicBool, events: &Mutex<Vec<DeviceEvent>>) -> Vec<Device> {
    let mut descriptors: Vec<libc::pollfd> =
        devices.iter().map(|device| libc::pollfd { fd: device.fd, events: libc::POLLIN, revents: 0 }).collect();
    let mut keyboard = Keyboard::new(if devices.iter().any(|device| device.keyboard) { hyprland_layout() } else { None });
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
                let time = event_time(device, &event);
                let output = device_event(device.keyboard, event.type_, event.code, event.value, time, &mut keyboard);
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
    use crate::keymap::KeyboardNames;

    /// More evdev codes the tests press.
    const KEY_1: u16 = 2;
    const KEY_0: u16 = 11;
    const KEY_MINUS: u16 = 12;
    const KEY_TAB: u16 = 15;
    const KEY_Q: u16 = 16;
    const KEY_C: u16 = 46;
    const KEY_CAPSLOCK: u16 = 58;
    const KEY_F5: u16 = 63;
    const KEY_KP1: u16 = 79;
    const KEY_UP: u16 = 103;

    fn layout(layout: &str, options: &str) -> Option<Layout> {
        let names = KeyboardNames { layout: layout.into(), options: options.into(), ..KeyboardNames::default() };
        Some(Layout::new(&names).expect("libxkbcommon compiles the keymap (xkeyboard-config is installed)"))
    }

    /// A keyboard pressed and released key by key through [`device_event`].
    struct Typist(Keyboard);

    impl Typist {
        fn new(layout: Option<Layout>) -> Typist {
            Typist(Keyboard::new(layout))
        }

        fn key(&mut self, code: u16, value: i32) -> Option<DeviceEvent> {
            device_event(true, EV_KEY, code, value, 7, &mut self.0)
        }

        /// Taps `code` with `held` down: the press and release kept, if any.
        fn chord(&mut self, held: &[u16], code: u16) -> (Option<DeviceEvent>, Option<DeviceEvent>) {
            for &modifier in held {
                self.key(modifier, 1);
            }
            let tapped = (self.key(code, 1), self.key(code, 0));
            for &modifier in held.iter().rev() {
                self.key(modifier, 0);
            }
            tapped
        }

        /// Whether a tap of `code` with `held` down is kept, press and release alike.
        fn kept(&mut self, held: &[u16], code: u16) -> bool {
            let (down, up) = self.chord(held, code);
            assert_eq!(down.is_some(), up.is_some(), "a release is kept with its press ({held:?} {code})");
            down.is_some()
        }

        /// The label and modifiers of a kept press of `code` with `held` down.
        fn shown(&mut self, held: &[u16], code: u16) -> (String, Vec<String>) {
            let down = self.chord(held, code).0.expect("kept");
            (down.key_label, down.modifiers)
        }
    }

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
        let mut keyboard = Keyboard::new(None);
        let mut event = |keyboard_device, kind, code, value| device_event(keyboard_device, kind, code, value, 7, &mut keyboard);
        let scroll = event(false, EV_REL, REL_WHEEL, 1).unwrap();
        assert_eq!((scroll.kind, scroll.dx, scroll.dy, scroll.monotonic_us), (DeviceEventKind::Scroll, 0, -1, 7));
        let horizontal = event(false, EV_REL, REL_HWHEEL, -2).unwrap();
        assert_eq!((horizontal.dx, horizontal.dy), (-2, 0));
        assert!(event(false, EV_REL, REL_WHEEL_HI_RES, 120).is_none());
        let click = event(false, EV_KEY, BTN_LEFT, 1).unwrap();
        assert_eq!((click.kind, click.button.as_str()), (DeviceEventKind::ButtonDown, "left"));
        assert_eq!(event(true, EV_KEY, BTN_MIDDLE, 0).unwrap().kind, DeviceEventKind::ButtonUp);
        assert!(event(false, EV_KEY, KEY_ENTER, 1).is_none(), "keys only from keyboards");
        assert!(event(true, EV_KEY, KEY_ENTER, 2).is_none(), "no repeats");
        assert!(event(true, EV_KEY, 0x113, 1).is_none(), "side buttons are not keys");
        let ctrl = event(true, EV_KEY, KEY_LEFTCTRL, 1).unwrap();
        assert_eq!((ctrl.kind, ctrl.key_code, ctrl.key_name.as_str()), (DeviceEventKind::KeyDown, 29, "KEY_LEFTCTRL"));
        assert_eq!((ctrl.modifiers.as_slice(), ctrl.key_label.as_str()), (["ctrl".to_owned()].as_slice(), ""));
        event(true, EV_KEY, KEY_RIGHTCTRL, 1);
        event(true, EV_KEY, KEY_LEFTSHIFT, 1);
        let a = event(true, EV_KEY, KEY_A, 1).unwrap();
        assert_eq!((a.key_name.as_str(), a.modifiers.as_slice()), ("KEY_A", ["ctrl".to_owned(), "shift".to_owned()].as_slice()));
        // Either ctrl going up lets go of ctrl.
        let up = event(true, EV_KEY, KEY_RIGHTCTRL, 0).unwrap();
        assert_eq!((up.kind, up.modifiers.len()), (DeviceEventKind::KeyUp, 0));
        assert_eq!(event(true, EV_KEY, KEY_A, 0).unwrap().kind, DeviceEventKind::KeyUp, "a kept press's release");
        assert!(event(true, EV_KEY, KEY_A, 1).is_none(), "Shift+A is typing");
        assert!(event(true, EV_KEY, KEY_A, 0).is_none(), "and so is its release");
        assert_eq!(event(true, EV_KEY, KEY_TAB, 1).unwrap().modifiers, ["shift"]);
        assert!(event(true, 0x03, 0, 5).is_none());
    }

    /// What is kept on any keyboard, with or without its layout.
    fn keeps_shortcuts_only(typist: &mut Typist) {
        for code in [KEY_A, KEY_Q, KEY_1, KEY_MINUS, KEY_SPACE, KEY_KP1] {
            assert!(!typist.kept(&[], code), "{code} alone");
            assert!(!typist.kept(&[KEY_LEFTSHIFT], code), "Shift+{code}");
            assert!(typist.kept(&[KEY_LEFTCTRL], code), "Ctrl+{code}");
            assert!(typist.kept(&[KEY_LEFTALT], code), "Alt+{code}");
            assert!(typist.kept(&[KEY_LEFTMETA], code), "Super+{code}");
            assert!(typist.kept(&[KEY_LEFTSHIFT, KEY_RIGHTCTRL], code), "Ctrl+Shift+{code}");
        }
        for code in [KEY_ENTER, KEY_TAB, KEY_F5, KEY_UP] {
            assert!(typist.kept(&[], code), "{code} alone");
            assert!(typist.kept(&[KEY_LEFTSHIFT], code), "Shift+{code}");
        }
        for code in [KEY_LEFTCTRL, KEY_RIGHTCTRL, KEY_LEFTALT, KEY_LEFTMETA] {
            assert!(typist.kept(&[], code), "modifier {code}");
        }
        for code in [KEY_LEFTSHIFT, KEY_RIGHTSHIFT, KEY_CAPSLOCK] {
            assert!(!typist.kept(&[], code), "{code} alone");
            assert!(typist.kept(&[KEY_LEFTCTRL], code), "Ctrl+{code}");
        }
    }

    #[test]
    fn typing_is_never_kept_without_a_layout() {
        let mut typist = Typist::new(None);
        keeps_shortcuts_only(&mut typist);
        // Right Alt may be AltGr: it makes no shortcut.
        assert!(!typist.kept(&[KEY_RIGHTALT], KEY_Q));
        assert_eq!(typist.shown(&[KEY_LEFTCTRL], KEY_Q), (String::new(), vec!["ctrl".to_owned()]));
    }

    #[test]
    fn typing_is_never_kept_on_a_us_layout() {
        let mut typist = Typist::new(layout("us", ""));
        keeps_shortcuts_only(&mut typist);
        // Right Alt is Alt on a US layout.
        assert!(typist.kept(&[KEY_RIGHTALT], KEY_Q));
        assert_eq!(typist.shown(&[KEY_LEFTCTRL, KEY_LEFTSHIFT], KEY_A), ("A".to_owned(), vec!["ctrl".to_owned(), "shift".to_owned()]));
        assert_eq!(typist.shown(&[KEY_LEFTSHIFT], KEY_TAB).0, "Tab");
        assert_eq!(typist.shown(&[], KEY_LEFTMETA), ("Super".to_owned(), vec!["meta".to_owned()]));
        assert_eq!(typist.shown(&[KEY_LEFTMETA], KEY_SPACE).0, "Space");
    }

    #[test]
    fn typing_is_never_kept_on_an_azerty_layout() {
        let mut typist = Typist::new(layout("fr", ""));
        keeps_shortcuts_only(&mut typist);
        // AltGr types "@" and "€" there; they stay out.
        assert!(!typist.kept(&[KEY_RIGHTALT], KEY_0));
        assert!(!typist.kept(&[KEY_RIGHTALT], 18));
        assert!(!typist.kept(&[], KEY_RIGHTALT));
        assert_eq!(typist.shown(&[KEY_LEFTCTRL], KEY_RIGHTALT), ("AltGr".to_owned(), vec!["ctrl".to_owned()]));
        // Keys are named by what AZERTY prints on them.
        assert_eq!(typist.shown(&[KEY_LEFTCTRL, KEY_LEFTSHIFT], KEY_Q).0, "A");
        assert_eq!(typist.shown(&[KEY_LEFTCTRL], KEY_A).0, "Q");
        assert_eq!(typist.shown(&[KEY_LEFTCTRL], KEY_1).0, "&");
    }

    #[test]
    fn modifiers_follow_the_layout_options() {
        let mut typist = Typist::new(layout("us", "ctrl:nocaps"));
        assert!(typist.kept(&[KEY_CAPSLOCK], KEY_C), "Caps Lock is Ctrl");
        assert_eq!(typist.shown(&[KEY_CAPSLOCK], KEY_C), ("C".to_owned(), vec!["ctrl".to_owned()]));
        let mut compose = Typist::new(layout("us", "compose:caps"));
        assert!(!compose.kept(&[], KEY_CAPSLOCK), "Compose starts typing");
    }
}
