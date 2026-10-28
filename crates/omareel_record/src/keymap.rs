//! The keyboard layout the recorded keys are named and filtered with: the keymap Hyprland gives
//! its main keyboard (rules, model, layout, variant and options from `hyprctl devices -j`, else
//! `hyprctl getoption input:kb_*`), compiled with libxkbcommon and followed key by key. A key is
//! then named by what the user's layout prints on it rather than by its US scancode, and typing
//! is told from other keys by the characters the key can produce, or by its choosing them (Shift,
//! AltGr, Caps Lock).
//!
//! The keysym decisions ([`keysym_label`], [`keysym_is_typing`], [`keysym_modifier`]) need no
//! keymap; [`Layout`] needs xkeyboard-config's data, which libxkbcommon depends on.

use omareel_core::compat::qjson::{self, Object, Value};
use omareel_core::compat::qprocess;
use omareel_core::keystroke_track::modifier_label;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

/// libxkbcommon's `struct xkb_context`.
#[repr(C)]
struct XkbContext {
    _private: [u8; 0],
}

/// libxkbcommon's `struct xkb_keymap`.
#[repr(C)]
struct XkbKeymap {
    _private: [u8; 0],
}

/// libxkbcommon's `struct xkb_state`.
#[repr(C)]
struct XkbState {
    _private: [u8; 0],
}

/// libxkbcommon's `struct xkb_rule_names`: empty strings take libxkbcommon's defaults (or the
/// `XKB_DEFAULT_*` variables).
#[repr(C)]
struct XkbRuleNames {
    rules: *const c_char,
    model: *const c_char,
    layout: *const c_char,
    variant: *const c_char,
    options: *const c_char,
}

#[link(name = "xkbcommon")]
unsafe extern "C" {
    fn xkb_context_new(flags: c_int) -> *mut XkbContext;
    fn xkb_context_unref(context: *mut XkbContext);
    fn xkb_keymap_new_from_names(context: *mut XkbContext, names: *const XkbRuleNames, flags: c_int) -> *mut XkbKeymap;
    fn xkb_keymap_unref(keymap: *mut XkbKeymap);
    fn xkb_keymap_num_layouts(keymap: *mut XkbKeymap) -> u32;
    fn xkb_keymap_layout_get_name(keymap: *mut XkbKeymap, layout: u32) -> *const c_char;
    fn xkb_keymap_mod_get_index(keymap: *mut XkbKeymap, name: *const c_char) -> u32;
    fn xkb_keymap_num_levels_for_key(keymap: *mut XkbKeymap, key: u32, layout: u32) -> u32;
    fn xkb_keymap_key_get_syms_by_level(
        keymap: *mut XkbKeymap,
        key: u32,
        layout: u32,
        level: u32,
        syms: *mut *const u32,
    ) -> c_int;
    fn xkb_state_new(keymap: *mut XkbKeymap) -> *mut XkbState;
    fn xkb_state_unref(state: *mut XkbState);
    fn xkb_state_update_key(state: *mut XkbState, key: u32, direction: c_int) -> c_int;
    fn xkb_state_update_mask(
        state: *mut XkbState,
        depressed_mods: u32,
        latched_mods: u32,
        locked_mods: u32,
        depressed_layout: u32,
        latched_layout: u32,
        locked_layout: u32,
    ) -> c_int;
    fn xkb_state_key_get_layout(state: *mut XkbState, key: u32) -> u32;
    fn xkb_state_key_get_one_sym(state: *mut XkbState, key: u32) -> u32;
    fn xkb_keysym_to_utf32(keysym: u32) -> u32;
    fn xkb_keysym_get_name(keysym: u32, buffer: *mut c_char, size: usize) -> c_int;
}

const XKB_KEY_UP: c_int = 0;
const XKB_KEY_DOWN: c_int = 1;
/// What `xkb_state_key_get_layout` and `xkb_keymap_mod_get_index` answer for none.
const XKB_INVALID: u32 = 0xffff_ffff;
/// An xkb keycode is the evdev code plus 8.
const EVDEV_OFFSET: u32 = 8;

/// The keysyms the decisions below single out (`xkbcommon-keysyms.h`).
mod keysyms {
    pub const NO_SYMBOL: u32 = 0;
    /// `ISO_Lock` to `ISO_Level5_Lock`: the level and group keys (AltGr is `ISO_Level3_Shift`).
    pub const ISO_LEVEL_FIRST: u32 = 0xfe01;
    pub const ISO_LEVEL3_SHIFT: u32 = 0xfe03;
    pub const ISO_LEVEL_LAST: u32 = 0xfe13;
    pub const DEAD_FIRST: u32 = 0xfe50;
    pub const DEAD_GRAVE: u32 = 0xfe50;
    pub const DEAD_ACUTE: u32 = 0xfe51;
    pub const DEAD_CIRCUMFLEX: u32 = 0xfe52;
    pub const DEAD_TILDE: u32 = 0xfe53;
    pub const DEAD_DIAERESIS: u32 = 0xfe57;
    pub const DEAD_ABOVERING: u32 = 0xfe58;
    pub const DEAD_CARON: u32 = 0xfe5a;
    pub const DEAD_CEDILLA: u32 = 0xfe5b;
    pub const DEAD_LAST: u32 = 0xfe93;
    pub const MULTI_KEY: u32 = 0xff20;
    pub const MODE_SWITCH: u32 = 0xff7e;
    pub const NUM_LOCK: u32 = 0xff7f;
    pub const KP_FIRST: u32 = 0xff80;
    pub const KP_LAST: u32 = 0xffbd;
    pub const SHIFT_L: u32 = 0xffe1;
    pub const SHIFT_R: u32 = 0xffe2;
    pub const CONTROL_L: u32 = 0xffe3;
    pub const CONTROL_R: u32 = 0xffe4;
    pub const CAPS_LOCK: u32 = 0xffe5;
    pub const SHIFT_LOCK: u32 = 0xffe6;
    pub const META_L: u32 = 0xffe7;
    pub const META_R: u32 = 0xffe8;
    pub const ALT_L: u32 = 0xffe9;
    pub const ALT_R: u32 = 0xffea;
    pub const SUPER_L: u32 = 0xffeb;
    pub const SUPER_R: u32 = 0xffec;
    pub const HYPER_L: u32 = 0xffed;
    pub const HYPER_R: u32 = 0xffee;
}
use keysyms::*;

/// The keymap names of a keyboard, and which of its layouts is active and which locks are on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyboardNames {
    pub rules: String,
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: String,
    /// `active_layout_index`, when Hyprland gives it.
    pub active_layout: Option<u32>,
    /// `active_keymap`: the active layout's description, such as "French (AZERTY)".
    pub active_keymap: String,
    pub caps_lock: bool,
    pub num_lock: bool,
}

/// The main keyboard in `hyprctl devices -j`'s answer, or its first keyboard when none is
/// marked main. `None` when the answer has no keyboards.
pub fn names_from_devices(json: &[u8]) -> Option<KeyboardNames> {
    let Ok(Value::Object(devices)) = qjson::from_json(json) else { return None };
    let keyboards: Vec<Object> = devices.value("keyboards").to_array().iter().map(Value::to_object).collect();
    let keyboard = keyboards.iter().find(|keyboard| keyboard.value("main").to_bool(false)).or(keyboards.first())?;
    let text = |key: &str| keyboard.value(key).to_string_or("");
    let index = keyboard.value("active_layout_index").to_int(-1);
    Some(KeyboardNames {
        rules: text("rules"),
        model: text("model"),
        layout: text("layout"),
        variant: text("variant"),
        options: text("options"),
        active_layout: u32::try_from(index).ok(),
        active_keymap: text("active_keymap"),
        caps_lock: keyboard.value("capsLock").to_bool(false),
        num_lock: keyboard.value("numLock").to_bool(false),
    })
}

/// The `str` of a `hyprctl getoption <option> -j` answer.
fn option_from_json(json: &[u8]) -> Option<String> {
    let Ok(Value::Object(option)) = qjson::from_json(json) else { return None };
    option.contains("str").then(|| option.value("str").to_string_or(""))
}

/// What `hyprctl` printed when it succeeded within a second.
fn hyprctl(arguments: &[&str]) -> Option<Vec<u8>> {
    qprocess::run("hyprctl", arguments, 1000).filter(qprocess::Finished::succeeded).map(|finished| finished.stdout)
}

/// The names Hyprland gives its main keyboard ([`names_from_devices`]), else its `input:kb_*`
/// options (without the active layout or the locks). `None` without an answer from Hyprland.
pub fn hyprland_keyboard() -> Option<KeyboardNames> {
    if let Some(names) = hyprctl(&["devices", "-j"]).and_then(|json| names_from_devices(&json)) {
        return Some(names);
    }
    let option = |name: &str| hyprctl(&["getoption", &format!("input:kb_{name}"), "-j"]).and_then(|json| option_from_json(&json));
    let layout = option("layout")?;
    Some(KeyboardNames {
        rules: option("rules").unwrap_or_default(),
        model: option("model").unwrap_or_default(),
        layout,
        variant: option("variant").unwrap_or_default(),
        options: option("options").unwrap_or_default(),
        ..KeyboardNames::default()
    })
}

/// What a key is under the layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Key {
    /// [`keysym_label`] of the key's first keysym at its base level; empty when it has none.
    pub label: String,
    /// Whether the key is part of typing: some keysym of it, at any level of the active layout or
    /// in the current state, is ([`keysym_is_typing`]). Keypad keys are, whether Num Lock is on
    /// or not.
    pub typing: bool,
    /// [`keysym_modifier`] of its base keysym: "ctrl", "shift", "alt", "meta" or empty.
    pub modifier: &'static str,
}

/// A compiled keymap and the state of the keyboard through it.
pub struct Layout {
    context: *mut XkbContext,
    keymap: *mut XkbKeymap,
    state: *mut XkbState,
}

impl Drop for Layout {
    fn drop(&mut self) {
        // SAFETY: each was made by `Layout::new` and is released once.
        unsafe {
            xkb_state_unref(self.state);
            xkb_keymap_unref(self.keymap);
            xkb_context_unref(self.context);
        }
    }
}

impl Layout {
    /// Compiles the keymap `names` describe, starting at its active layout (by index, else by
    /// description) with Caps Lock and Num Lock as they are. `None` when libxkbcommon cannot
    /// compile it.
    pub fn new(names: &KeyboardNames) -> Option<Layout> {
        let text = |value: &str| CString::new(value).unwrap_or_default();
        let (rules, model, layout, variant, options) =
            (text(&names.rules), text(&names.model), text(&names.layout), text(&names.variant), text(&names.options));
        let rule_names = XkbRuleNames {
            rules: rules.as_ptr(),
            model: model.as_ptr(),
            layout: layout.as_ptr(),
            variant: variant.as_ptr(),
            options: options.as_ptr(),
        };
        // SAFETY: no flags.
        let context = unsafe { xkb_context_new(0) };
        if context.is_null() {
            return None;
        }
        // SAFETY: a live context and names whose strings outlive the call.
        let keymap = unsafe { xkb_keymap_new_from_names(context, &rule_names, 0) };
        if keymap.is_null() {
            // SAFETY: the context made above.
            unsafe { xkb_context_unref(context) };
            return None;
        }
        // SAFETY: a live keymap.
        let state = unsafe { xkb_state_new(keymap) };
        if state.is_null() {
            // SAFETY: the keymap and context made above.
            unsafe {
                xkb_keymap_unref(keymap);
                xkb_context_unref(context);
            }
            return None;
        }
        let result = Layout { context, keymap, state };
        let active = names.active_layout.or_else(|| result.layout_named(&names.active_keymap)).unwrap_or(0);
        // SAFETY: a live keymap.
        let active = if active < unsafe { xkb_keymap_num_layouts(keymap) } { active } else { 0 };
        let mut locked = 0;
        for (on, name) in [(names.caps_lock, c"Lock"), (names.num_lock, c"Mod2")] {
            // SAFETY: a live keymap and a NUL-terminated name.
            let index = unsafe { xkb_keymap_mod_get_index(keymap, name.as_ptr()) };
            if on && index < 32 {
                locked |= 1 << index;
            }
        }
        // SAFETY: a live state.
        unsafe { xkb_state_update_mask(state, 0, 0, locked, 0, 0, active) };
        Some(result)
    }

    /// The index of the layout whose description is `name`.
    fn layout_named(&self, name: &str) -> Option<u32> {
        if name.is_empty() {
            return None;
        }
        // SAFETY: a live keymap.
        (0..unsafe { xkb_keymap_num_layouts(self.keymap) }).find(|&index| {
            // SAFETY: a live keymap and a layout index it has; the name is static to the keymap.
            let layout = unsafe { xkb_keymap_layout_get_name(self.keymap, index) };
            // SAFETY: a NUL-terminated string when not null.
            !layout.is_null() && unsafe { CStr::from_ptr(layout) }.to_bytes() == name.as_bytes()
        })
    }

    /// The keysyms of `keycode` at `level` of `layout`.
    fn syms(&self, keycode: u32, layout: u32, level: u32) -> &[u32] {
        let mut syms: *const u32 = std::ptr::null();
        // SAFETY: a live keymap and a place for the keymap's own array.
        let count = unsafe { xkb_keymap_key_get_syms_by_level(self.keymap, keycode, layout, level, &mut syms) };
        if count <= 0 || syms.is_null() {
            return &[];
        }
        // SAFETY: the keymap's array of `count` keysyms, alive as long as the keymap.
        unsafe { std::slice::from_raw_parts(syms, count as usize) }
    }

    /// What the key with evdev code `code` is in the current state ([`Key`]), which its press
    /// (`down`) or release then updates.
    pub fn key(&mut self, code: u16, down: bool) -> Key {
        let keycode = u32::from(code) + EVDEV_OFFSET;
        // SAFETY: a live state; any keycode may be asked about.
        let layout = unsafe { xkb_state_key_get_layout(self.state, keycode) };
        // SAFETY: as above.
        let current = unsafe { xkb_state_key_get_one_sym(self.state, keycode) };
        let mut base = NO_SYMBOL;
        let mut typing = keysym_is_typing(current);
        if layout != XKB_INVALID {
            // SAFETY: a live keymap.
            let levels = unsafe { xkb_keymap_num_levels_for_key(self.keymap, keycode, layout) };
            for level in 0..levels {
                let syms = self.syms(keycode, layout, level);
                if level == 0 {
                    base = syms.first().copied().unwrap_or(NO_SYMBOL);
                }
                typing |= syms.iter().any(|&sym| keysym_is_typing(sym));
            }
        }
        // Keypad keys are named by what they do now: a digit with Num Lock on, Home without.
        let label = if (KP_FIRST..=KP_LAST).contains(&base) && current != NO_SYMBOL {
            keysym_label(current)
        } else {
            keysym_label(base)
        };
        // SAFETY: a live state.
        unsafe { xkb_state_update_key(self.state, keycode, if down { XKB_KEY_DOWN } else { XKB_KEY_UP }) };
        Key { label, typing, modifier: keysym_modifier(base) }
    }
}

/// The character a keysym types, if any.
fn keysym_char(keysym: u32) -> Option<char> {
    // SAFETY: a pure function of its argument.
    char::from_u32(unsafe { xkb_keysym_to_utf32(keysym) }).filter(|&c| c != '\0')
}

/// libxkbcommon's name of a keysym, such as "Prior" or "XF86AudioPlay"; empty for none.
fn keysym_name(keysym: u32) -> String {
    let mut buffer = [0 as c_char; 64];
    // SAFETY: a buffer of the size given.
    let length = unsafe { xkb_keysym_get_name(keysym, buffer.as_mut_ptr(), buffer.len()) };
    if length <= 0 {
        return String::new();
    }
    // SAFETY: NUL-terminated within the buffer (truncated names are too).
    unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned()
}

/// Whether a keysym is part of typing text: a printable character (space included); a dead key
/// or the Compose key, which start characters; or a key that chooses what the others type
/// (Shift, AltGr and the other level and group keys, Caps Lock, Shift Lock, Num Lock), whose
/// presses would show where a password has capitals or symbols.
pub fn keysym_is_typing(keysym: u32) -> bool {
    (DEAD_FIRST..=DEAD_LAST).contains(&keysym)
        || (ISO_LEVEL_FIRST..=ISO_LEVEL_LAST).contains(&keysym)
        || matches!(keysym, MULTI_KEY | MODE_SWITCH | NUM_LOCK | SHIFT_L | SHIFT_R | CAPS_LOCK | SHIFT_LOCK)
        || keysym_char(keysym).is_some_and(|c| !c.is_control())
}

/// The `mods` name of a modifier keysym: "ctrl", "shift", "alt" (Alt and Meta), "meta" (Super
/// and Hyper); empty for other keysyms, AltGr's included.
pub fn keysym_modifier(keysym: u32) -> &'static str {
    match keysym {
        CONTROL_L | CONTROL_R => "ctrl",
        SHIFT_L | SHIFT_R => "shift",
        ALT_L | ALT_R | META_L | META_R => "alt",
        SUPER_L | SUPER_R | HYPER_L | HYPER_R => "meta",
        _ => "",
    }
}

/// How a pill shows a keysym's key: modifiers as the track names them, AltGr, a character in
/// upper case (when that is one character), "Space", the spacing accent of the common dead keys,
/// keypad keys as "Num …", names for the editing, lock and media keys, and otherwise the keysym's
/// name in words ("XF86Calculator" as "Calculator", "Hangul_Hanja" as "Hangul Hanja"). Empty for
/// no keysym.
pub fn keysym_label(keysym: u32) -> String {
    if keysym == NO_SYMBOL {
        return String::new();
    }
    let modifier = keysym_modifier(keysym);
    if !modifier.is_empty() {
        return modifier_label(modifier).to_owned();
    }
    let accent = match keysym {
        ISO_LEVEL3_SHIFT => Some("AltGr"),
        DEAD_GRAVE => Some("`"),
        DEAD_ACUTE => Some("´"),
        DEAD_CIRCUMFLEX => Some("^"),
        DEAD_TILDE => Some("~"),
        DEAD_DIAERESIS => Some("¨"),
        DEAD_ABOVERING => Some("°"),
        DEAD_CARON => Some("ˇ"),
        DEAD_CEDILLA => Some("¸"),
        _ => None,
    };
    if let Some(accent) = accent {
        return accent.to_owned();
    }
    let keypad = (KP_FIRST..=KP_LAST).contains(&keysym);
    let prefix = if keypad { "Num " } else { "" };
    if let Some(c) = keysym_char(keysym).filter(|c| !c.is_control()) {
        if c == ' ' {
            return format!("{prefix}Space");
        }
        let mut upper = c.to_uppercase();
        let c = match (upper.next(), upper.next()) {
            (Some(only), None) => only,
            _ => c,
        };
        return format!("{prefix}{c}");
    }
    let name = keysym_name(keysym);
    let name = name.strip_prefix("KP_").unwrap_or(&name);
    let named = match name {
        "Return" | "Enter" => "Enter",
        "Escape" => "Esc",
        "Tab" | "ISO_Left_Tab" => "Tab",
        "BackSpace" => "Backspace",
        "Delete" => "Delete",
        "Insert" => "Insert",
        "Prior" | "Page_Up" => "Page Up",
        "Next" | "Page_Down" => "Page Down",
        "Caps_Lock" => "Caps Lock",
        "Num_Lock" => "Num Lock",
        "Scroll_Lock" => "Scroll Lock",
        "Multi_key" => "Compose",
        "XF86AudioRaiseVolume" => "Volume Up",
        "XF86AudioLowerVolume" => "Volume Down",
        "XF86AudioMute" => "Mute",
        "XF86AudioMicMute" => "Mic Mute",
        "XF86AudioPlay" => "Play",
        "XF86AudioPause" => "Pause",
        "XF86AudioStop" => "Stop",
        "XF86AudioNext" => "Next",
        "XF86AudioPrev" => "Previous",
        "XF86MonBrightnessUp" => "Brightness Up",
        "XF86MonBrightnessDown" => "Brightness Down",
        _ => "",
    };
    let label = if named.is_empty() { name_in_words(name.strip_prefix("XF86").unwrap_or(name)) } else { named.to_owned() };
    if label.is_empty() { label } else { format!("{prefix}{label}") }
}

/// A keysym name in words: underscores as spaces, and a space where a lower-case letter meets an
/// upper-case one ("AudioMedia" as "Audio Media", "F12" and "Home" as they are).
fn name_in_words(name: &str) -> String {
    let mut words = String::new();
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if c == '_' {
            if !words.is_empty() && !words.ends_with(' ') {
                words.push(' ');
            }
            previous = None;
            continue;
        }
        if previous.is_some_and(char::is_lowercase) && c.is_uppercase() {
            words.push(' ');
        }
        words.push(c);
        previous = Some(c);
    }
    words.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// evdev codes the tests press.
    const KEY_1: u16 = 2;
    const KEY_Q: u16 = 16;
    const KEY_ENTER: u16 = 28;
    const KEY_LEFTCTRL: u16 = 29;
    const KEY_A: u16 = 30;
    const KEY_LEFTSHIFT: u16 = 42;
    const KEY_M: u16 = 50;
    const KEY_SPACE: u16 = 57;
    const KEY_CAPSLOCK: u16 = 58;
    const KEY_KP7: u16 = 71;
    const KEY_RIGHTALT: u16 = 100;
    const KEY_HOME: u16 = 102;
    const KEY_LEFTMETA: u16 = 125;
    const KEY_LEFTBRACE: u16 = 26;
    const KEY_VOLUMEUP: u16 = 115;

    fn layout(layout: &str, variant: &str, options: &str) -> Layout {
        let names = KeyboardNames {
            layout: layout.into(),
            variant: variant.into(),
            options: options.into(),
            ..KeyboardNames::default()
        };
        Layout::new(&names).expect("libxkbcommon compiles the keymap (xkeyboard-config is installed)")
    }

    /// A press and release of `code`, what the press gave.
    fn tap(layout: &mut Layout, code: u16) -> Key {
        let key = layout.key(code, true);
        layout.key(code, false);
        key
    }

    #[test]
    fn keysym_labels() {
        assert_eq!(keysym_label(0x61), "A");
        assert_eq!(keysym_label(0xe9), "É");
        assert_eq!(keysym_label(0xdf), "ß");
        assert_eq!(keysym_label(0x20), "Space");
        assert_eq!(keysym_label(0x2c), ",");
        assert_eq!(keysym_label(0xff0d), "Enter");
        assert_eq!(keysym_label(0xff1b), "Esc");
        assert_eq!(keysym_label(0xfe20), "Tab");
        assert_eq!(keysym_label(0xff55), "Page Up");
        assert_eq!(keysym_label(0xff50), "Home");
        assert_eq!(keysym_label(0xffc9), "F12");
        assert_eq!(keysym_label(0xffb7), "Num 7");
        assert_eq!(keysym_label(0xff95), "Num Home");
        assert_eq!(keysym_label(0xff8d), "Num Enter");
        assert_eq!(keysym_label(0x1008ff13), "Volume Up");
        assert_eq!(keysym_label(0x1008ff1d), "Calculator");
        assert_eq!(keysym_label(0xff34), "Hangul Hanja");
        assert_eq!(keysym_label(DEAD_CIRCUMFLEX), "^");
        assert_eq!(keysym_label(0xfe5c), "dead ogonek");
        assert_eq!(keysym_label(CONTROL_R), "Ctrl");
        assert_eq!(keysym_label(SUPER_L), "Super");
        assert_eq!(keysym_label(ISO_LEVEL3_SHIFT), "AltGr");
        assert_eq!(keysym_label(NO_SYMBOL), "");
        assert_eq!(name_in_words("AudioMedia"), "Audio Media");
        assert_eq!(name_in_words("_a__b_"), "a b");
    }

    #[test]
    fn text_keysyms() {
        for keysym in [0x61, 0x20, 0xe9, 0xffb1, 0xff80, DEAD_ACUTE, 0xfe5c, MULTI_KEY, 0x10020ac] {
            assert!(keysym_is_typing(keysym), "{keysym:#x}");
        }
        for keysym in [SHIFT_R, CAPS_LOCK, NUM_LOCK, ISO_LEVEL3_SHIFT, 0xfe08, MODE_SWITCH] {
            assert!(keysym_is_typing(keysym), "{keysym:#x}");
        }
        for keysym in [0xff0d, 0xff09, 0xff08, 0xffff, 0xff1b, 0xff95, 0xffbe, CONTROL_L, ALT_L, SUPER_R, 0] {
            assert!(!keysym_is_typing(keysym), "{keysym:#x}");
        }
        assert_eq!(keysym_modifier(META_L), "alt");
        assert_eq!(keysym_modifier(HYPER_R), "meta");
        assert_eq!(keysym_modifier(ISO_LEVEL3_SHIFT), "");
    }

    #[test]
    fn us_keys() {
        let mut us = layout("us", "", "");
        assert_eq!(tap(&mut us, KEY_A), Key { label: "A".into(), typing: true, modifier: "" });
        assert_eq!(tap(&mut us, KEY_SPACE), Key { label: "Space".into(), typing: true, modifier: "" });
        assert_eq!(tap(&mut us, KEY_ENTER), Key { label: "Enter".into(), typing: false, modifier: "" });
        assert_eq!(tap(&mut us, KEY_HOME), Key { label: "Home".into(), typing: false, modifier: "" });
        assert_eq!(tap(&mut us, KEY_VOLUMEUP), Key { label: "Volume Up".into(), typing: false, modifier: "" });
        assert_eq!(tap(&mut us, KEY_LEFTMETA).modifier, "meta");
        assert_eq!(tap(&mut us, KEY_LEFTSHIFT), Key { label: "Shift".into(), typing: true, modifier: "shift" });
        assert_eq!(tap(&mut us, KEY_CAPSLOCK), Key { label: "Caps Lock".into(), typing: true, modifier: "" });
        assert_eq!(tap(&mut us, KEY_LEFTCTRL), Key { label: "Ctrl".into(), typing: false, modifier: "ctrl" });
        assert_eq!(tap(&mut us, KEY_RIGHTALT), Key { label: "Alt".into(), typing: false, modifier: "alt" });
        // Keypad keys type digits with Num Lock on, so they are text either way.
        let kp7 = tap(&mut us, KEY_KP7);
        assert!(kp7.typing);
        assert_eq!(kp7.label, "Num Home");
        let mut locked = Layout::new(&KeyboardNames { layout: "us".into(), num_lock: true, ..KeyboardNames::default() })
            .expect("a us keymap");
        assert_eq!(tap(&mut locked, KEY_KP7).label, "Num 7");
    }

    #[test]
    fn labels_follow_the_layout() {
        let mut fr = layout("fr", "", "");
        // AZERTY: A where US has Q, "," where US has M, "&" on the 1 key, ^ a dead key.
        assert_eq!(tap(&mut fr, KEY_Q).label, "A");
        assert_eq!(tap(&mut fr, KEY_A).label, "Q");
        assert_eq!(tap(&mut fr, KEY_M).label, ",");
        assert_eq!(tap(&mut fr, KEY_1).label, "&");
        assert_eq!(tap(&mut fr, KEY_LEFTBRACE), Key { label: "^".into(), typing: true, modifier: "" });
        // Right Alt is AltGr there, not Alt: AltGr and AltGr+key are typing, not shortcuts.
        assert_eq!(tap(&mut fr, KEY_RIGHTALT), Key { label: "AltGr".into(), typing: true, modifier: "" });
        // Shift held: the label stays the base level's.
        fr.key(KEY_LEFTSHIFT, true);
        assert_eq!(tap(&mut fr, KEY_1).label, "&");
        fr.key(KEY_LEFTSHIFT, false);
        let mut dvorak = layout("us", "dvorak", "");
        assert_eq!(tap(&mut dvorak, KEY_Q).label, "'");
        assert_eq!(tap(&mut dvorak, 47).label, "K");
    }

    #[test]
    fn options_move_modifiers() {
        let mut nocaps = layout("us", "", "ctrl:nocaps");
        assert_eq!(tap(&mut nocaps, KEY_CAPSLOCK), Key { label: "Ctrl".into(), typing: false, modifier: "ctrl" });
        let mut caps = layout("us", "", "compose:caps");
        assert_eq!(tap(&mut caps, KEY_CAPSLOCK), Key { label: "Compose".into(), typing: true, modifier: "" });
        assert_eq!(tap(&mut caps, KEY_LEFTCTRL).modifier, "ctrl");
    }

    #[test]
    fn the_active_layout_is_hyprlands() {
        let names = |active_layout, active_keymap: &str| KeyboardNames {
            layout: "us,fr".into(),
            active_layout,
            active_keymap: active_keymap.into(),
            ..KeyboardNames::default()
        };
        assert_eq!(tap(&mut Layout::new(&names(None, "")).unwrap(), KEY_Q).label, "Q");
        assert_eq!(tap(&mut Layout::new(&names(Some(1), "")).unwrap(), KEY_Q).label, "A");
        assert_eq!(tap(&mut Layout::new(&names(None, "French")).unwrap(), KEY_Q).label, "A");
        assert_eq!(tap(&mut Layout::new(&names(Some(7), "")).unwrap(), KEY_Q).label, "Q");
    }

    #[test]
    fn hyprland_names() {
        let devices = br#"{"mice": [], "keyboards": [
            {"name": "power-button", "rules": "", "model": "", "layout": "us", "variant": "", "options": "",
             "active_layout_index": 0, "active_keymap": "English (US)", "capsLock": false, "numLock": false, "main": false},
            {"name": "kbd", "rules": "evdev", "model": "pc105", "layout": "us,fr", "variant": ",azerty",
             "options": "compose:caps", "active_layout_index": 1, "active_keymap": "French (AZERTY)",
             "capsLock": true, "numLock": true, "main": true}]}"#;
        assert_eq!(
            names_from_devices(devices),
            Some(KeyboardNames {
                rules: "evdev".into(),
                model: "pc105".into(),
                layout: "us,fr".into(),
                variant: ",azerty".into(),
                options: "compose:caps".into(),
                active_layout: Some(1),
                active_keymap: "French (AZERTY)".into(),
                caps_lock: true,
                num_lock: true,
            })
        );
        let unmarked = br#"{"keyboards": [{"layout": "de", "active_keymap": "German"}, {"layout": "us"}]}"#;
        let first = names_from_devices(unmarked).unwrap();
        assert_eq!((first.layout.as_str(), first.active_layout, first.active_keymap.as_str()), ("de", None, "German"));
        assert_eq!(names_from_devices(br#"{"keyboards": []}"#), None);
        assert_eq!(names_from_devices(b"not json"), None);
        assert_eq!(option_from_json(br#"{"option": "input:kb_layout", "str": "us", "set": true }"#), Some("us".into()));
        assert_eq!(option_from_json(br#"{"option": "input:kb_layout", "int": 0}"#), None);
    }
}
