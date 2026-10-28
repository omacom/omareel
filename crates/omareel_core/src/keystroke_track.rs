//! The keystroke pills shown over the recording (`KeystrokeTrack`).
//!
//! Key names are `QString`s in the C++, so lengths count UTF-16 code units (`qstring::size`),
//! lowercasing is `QString::toLower` and capitalising a word changes its first UTF-16 code unit
//! with `QChar::toUpper`, which leaves a word starting outside the Basic Multilingual Plane
//! alone. `KEY_*` names from evdev are ASCII, where all of this is plain ASCII.
//!
//! A key down names its key twice: `name`, libevdev's `KEY_*` name of the scancode (a US layout's
//! key, whatever the layout), and, when the recorder could read the keyboard's layout, `label`,
//! what that layout prints on the key. The pills show the label when there is one.
//!
//! Plain typing is never shown: the recorder keeps only shortcuts (a key with Ctrl, Alt or Super
//! held, and those modifiers) and keys that type no text, and older recordings, which kept every
//! key, have their typing hidden here (`is_plain_typing`).

use crate::compat::cpp;
use crate::compat::{qchar, qstring};
use crate::input_log::{InputEvent, InputKind};
use crate::project::Keystrokes;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeystrokePill {
    pub keys: Vec<String>,
    pub text: String,
    pub opacity: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Group {
    start: f64,
    last: f64,
    keys: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeystrokeTrack {
    settings: Keystrokes,
    groups: Vec<Group>,
}

/// The modifiers (`mods` names) that make a key press a shortcut. Shift alone does not:
/// shifted letters are still typing.
pub const SHORTCUT_MODIFIERS: [&str; 3] = ["ctrl", "alt", "meta"];

/// The labels the recorder gives modifier keys: [`modifier_label`]'s, and AltGr's.
pub const MODIFIER_KEY_LABELS: [&str; 5] = ["Ctrl", "Shift", "Alt", "Super", "AltGr"];

/// Whether `modifiers` (`mods` names) hold a modifier that makes a key a shortcut.
pub fn makes_shortcut(modifiers: &[String]) -> bool {
    modifiers.iter().any(|modifier| SHORTCUT_MODIFIERS.contains(&modifier.as_str()))
}

/// Whether a libevdev key name is part of typing on a US layout: a name of one character (after
/// `KEY_`) that is not a control character, the punctuation, space and keypad keys, and the keys
/// that choose what the others type (Shift, Right Alt as AltGr, Caps Lock, Num Lock). The
/// recorder decides with this when it cannot read the keyboard's layout, and the track hides
/// plain typing in older recordings with it.
pub fn is_typing_key_name(name: &str) -> bool {
    let name = name.strip_prefix("KEY_").unwrap_or(name);
    let mut chars = name.chars();
    if let (Some(only), None) = (chars.next(), chars.next()) {
        return !only.is_control();
    }
    if let Some(digit) = name.strip_prefix("KP") {
        if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() {
            return true;
        }
    }
    matches!(
        name,
        "MINUS"
            | "EQUAL"
            | "LEFTBRACE"
            | "RIGHTBRACE"
            | "SEMICOLON"
            | "APOSTROPHE"
            | "GRAVE"
            | "BACKSLASH"
            | "COMMA"
            | "DOT"
            | "SLASH"
            | "SPACE"
            | "102ND"
            | "KPASTERISK"
            | "KPMINUS"
            | "KPPLUS"
            | "KPDOT"
            | "KPSLASH"
            | "KPEQUAL"
            | "KPCOMMA"
            | "KPJPCOMMA"
            | "KPPLUSMINUS"
            | "KPLEFTPAREN"
            | "KPRIGHTPAREN"
            | "YEN"
            | "RO"
            | "LEFTSHIFT"
            | "RIGHTSHIFT"
            | "RIGHTALT"
            | "CAPSLOCK"
            | "NUMLOCK"
    )
}

/// Whether a layout label is text: one character that is neither a control character nor white
/// space (the space bar's label is "Space").
fn is_text_label(label: &str) -> bool {
    let mut chars = label.chars();
    matches!((chars.next(), chars.next()), (Some(only), None) if !only.is_control() && !only.is_whitespace())
}

/// Whether a libevdev key name is a modifier key's.
fn is_modifier_name(name: &str) -> bool {
    name.contains("CTRL") || name.contains("SHIFT") || name.contains("ALT") || name.contains("META")
}

/// Whether a key down is a modifier key's: by its label when it has one (the layout may have
/// moved modifiers to other keys), else by its name.
fn is_modifier(event: &InputEvent) -> bool {
    if event.key_label.is_empty() {
        is_modifier_name(&event.key_name)
    } else {
        MODIFIER_KEY_LABELS.contains(&event.key_label.as_str())
    }
}

/// How a `mods` name shows in a pill; empty for names it does not know.
pub fn modifier_label(modifier: &str) -> &'static str {
    match modifier {
        "ctrl" => "Ctrl",
        "shift" => "Shift",
        "alt" => "Alt",
        "meta" => "Super",
        _ => "",
    }
}

/// The C++ keeps these in a `QHash`, only ever looked up, so its order does not matter.
fn named_label(name: &str) -> Option<&'static str> {
    Some(match name {
        "ESC" => "Esc",
        "ENTER" => "Enter",
        "SPACE" => "Space",
        "TAB" => "Tab",
        "BACKSPACE" => "Backspace",
        "DELETE" => "Delete",
        "UP" => "Up",
        "DOWN" => "Down",
        "LEFT" => "Left",
        "RIGHT" => "Right",
        "PAGEUP" => "Page Up",
        "PAGEDOWN" => "Page Down",
        "LEFTBRACE" => "[",
        "RIGHTBRACE" => "]",
        "MINUS" => "-",
        "EQUAL" => "=",
        "COMMA" => ",",
        "DOT" => ".",
        "SLASH" => "/",
        "SEMICOLON" => ";",
        "APOSTROPHE" => "'",
        _ => return None,
    })
}

fn key_label(name: &str) -> String {
    let name = name.strip_prefix("KEY_").unwrap_or(name);
    if qstring::size(name) == 1 {
        return name.to_owned();
    }
    if name.starts_with("KP") && qstring::size(name) == 3 {
        // Three UTF-16 code units starting with "KP": the last is a whole character.
        if let Some(last) = name.chars().next_back() {
            return format!("Num {last}");
        }
    }
    if let Some(label) = named_label(name) {
        return label.to_owned();
    }
    let lower = qstring::to_lower(&name.replace('_', " "));
    let words: Vec<String> = lower
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(|word| {
            // word[0] = word[0].toUpper(): one UTF-16 code unit. Surrogates stay as they are and
            // no other unit maps to one, so the result is valid UTF-16 again.
            let mut units: Vec<u16> = word.encode_utf16().collect();
            units[0] = qchar::to_upper(units[0]);
            String::from_utf16_lossy(&units)
        })
        .collect();
    words.join(" ")
}

/// Whether a key down is plain typing, which the pills never show: a typing key
/// ([`is_text_label`] by the label, else [`is_typing_key_name`] by the name) without Ctrl, Alt
/// or Super held. The
/// recorder no longer keeps any; recordings from before it stopped can have them.
fn is_plain_typing(event: &InputEvent) -> bool {
    if makes_shortcut(&event.modifiers) {
        return false;
    }
    if event.key_label.is_empty() { is_typing_key_name(&event.key_name) } else { is_text_label(&event.key_label) }
}

impl KeystrokeTrack {
    /// Key presses grouped into pills: a press joins the last pill when it comes within 0.6 s of
    /// that pill's last press and the pill stays at six keys or fewer. Modifier keys alone and
    /// plain typing (`is_plain_typing`) are left out.
    pub fn build(events: &[InputEvent], settings: &Keystrokes) -> KeystrokeTrack {
        let mut track = KeystrokeTrack { settings: settings.clone(), groups: Vec::new() };
        if !settings.enabled {
            return track;
        }
        for event in events {
            if event.kind != InputKind::KeyDown
                || (event.key_name.is_empty() && event.key_label.is_empty())
                || is_modifier(event)
                || is_plain_typing(event)
            {
                continue;
            }

            let mut keys: Vec<String> = Vec::new();
            for modifier in &event.modifiers {
                let label = modifier_label(modifier);
                if !label.is_empty() && !keys.iter().any(|key| key == label) {
                    keys.push(label.to_owned());
                }
            }
            let label = if event.key_label.is_empty() { key_label(&event.key_name) } else { event.key_label.clone() };
            if !label.is_empty() {
                keys.push(label);
            }
            if keys.is_empty() {
                continue;
            }

            let starts_group = match track.groups.last() {
                None => true,
                Some(last) => event.time - last.last > 0.6 || last.keys.len() + keys.len() > 6,
            };
            if starts_group {
                track.groups.push(Group { start: event.time, last: event.time, keys: Vec::new() });
            }
            let group = track.groups.last_mut().expect("a group was just ensured");
            group.last = event.time;
            for key in keys {
                if group.keys.len() >= 6 {
                    break;
                }
                group.keys.push(key);
            }
        }
        track
    }

    /// The pills showing at `time`: fully visible until `hold_ms` after their last key, then
    /// fading out over 0.25 s.
    pub fn sample(&self, time: f64) -> Vec<KeystrokePill> {
        let mut result = Vec::new();
        let hold = f64::from(self.settings.hold_ms) / 1000.0;
        const FADE: f64 = 0.25;
        for group in &self.groups {
            if time < group.start || time > group.last + hold + FADE {
                continue;
            }
            let opacity = if time <= group.last + hold {
                1.0
            } else {
                cpp::clamp(1.0 - (time - group.last - hold) / FADE, 0.0, 1.0)
            };
            result.push(KeystrokePill { keys: group.keys.clone(), text: group.keys.join(" + "), opacity });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qround::{q_fuzzy_compare, q_fuzzy_is_null};

    /// `QCOMPARE` on doubles: fuzzy, and an expected (fuzzy) zero needs an actual one.
    fn qcompare(actual: f64, expected: f64) -> bool {
        if expected.is_nan() {
            actual.is_nan()
        } else if expected.is_infinite() {
            actual == expected
        } else if q_fuzzy_is_null(expected) {
            q_fuzzy_is_null(actual)
        } else {
            q_fuzzy_compare(actual, expected)
        }
    }

    fn key_down(time: f64, key_code: i32, key_name: &str, modifiers: &[&str]) -> InputEvent {
        InputEvent {
            time,
            kind: InputKind::KeyDown,
            key_code,
            key_name: key_name.into(),
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
            ..InputEvent::default()
        }
    }

    fn labelled(time: f64, key_name: &str, key_label: &str, modifiers: &[&str]) -> InputEvent {
        InputEvent { key_label: key_label.into(), ..key_down(time, 0, key_name, modifiers) }
    }

    // tests/test_keystroketrack.cpp, with F5 where the plain K it had is now hidden as typing.
    #[test]
    fn groups_consecutive_keys_and_fades_from_last_key() {
        let settings = Keystrokes { enabled: true, hold_ms: 900, ..Keystrokes::default() };
        let events = [key_down(1.0, 25, "KEY_P", &["ctrl", "shift"]), key_down(1.5, 63, "KEY_F5", &[])];
        let track = KeystrokeTrack::build(&events, &settings);
        let grouped = track.sample(1.5);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].keys, ["Ctrl", "Shift", "P", "F5"]);
        assert_eq!(grouped[0].text, "Ctrl + Shift + P + F5");
        assert!(qcompare(grouped[0].opacity, 1.0));

        let fading = track.sample(2.525);
        assert_eq!(fading.len(), 1);
        assert!((fading[0].opacity - 0.5).abs() < 0.001);
        assert!(track.sample(2.76).is_empty());
    }

    #[test]
    fn shortcut_filter_and_six_key_limit() {
        let settings = Keystrokes { enabled: true, ..Keystrokes::default() };
        let mut events = vec![
            key_down(0.1, 30, "KEY_A", &[]),
            key_down(0.2, 28, "KEY_ENTER", &[]),
            key_down(0.3, 25, "KEY_P", &["ctrl"]),
        ];
        for i in 0..8 {
            events.push(key_down(0.4 + f64::from(i) * 0.05, 59 + i, &format!("KEY_F{}", i + 1), &[]));
        }
        let track = KeystrokeTrack::build(&events, &settings);
        let pills = track.sample(0.7);
        assert_eq!(pills.len(), 2);
        assert_eq!(pills[0].keys[0], "Enter");
        for pill in &pills {
            assert!(!pill.keys.iter().any(|key| key == "A"));
            assert!(pill.keys.len() <= 6);
        }
    }

    // Not in the C++ tests: older recordings kept every key, and their typing stays hidden.
    #[test]
    fn plain_typing_in_older_recordings_is_hidden() {
        let settings = Keystrokes { enabled: true, ..Keystrokes::default() };
        let events = [
            key_down(1.0, 30, "KEY_A", &["shift"]),
            key_down(1.1, 12, "KEY_MINUS", &[]),
            key_down(1.2, 57, "KEY_SPACE", &["shift"]),
            key_down(1.3, 79, "KEY_KP1", &[]),
            key_down(1.4, 15, "KEY_TAB", &["shift"]),
            key_down(1.5, 30, "KEY_A", &["alt"]),
            key_down(1.6, 57, "KEY_SPACE", &["meta"]),
        ];
        let track = KeystrokeTrack::build(&events, &settings);
        assert_eq!(track.sample(1.6)[0].keys, ["Shift", "Tab", "Alt", "A", "Super", "Space"]);
    }

    // Not in the C++ tests: the layout's label wins over the scancode's name.
    #[test]
    fn labels_from_the_layout() {
        let settings = Keystrokes { enabled: true, ..Keystrokes::default() };
        let events = [
            // AZERTY: the key in US Q's place prints "a", the one in US M's place ",".
            labelled(1.0, "KEY_Q", "A", &["ctrl", "shift"]),
            labelled(1.1, "KEY_M", ",", &["ctrl"]),
            // Caps Lock made a Ctrl key (ctrl:nocaps) is a modifier key, left out on its own.
            labelled(1.2, "KEY_CAPSLOCK", "Ctrl", &["ctrl"]),
            labelled(1.3, "KEY_RIGHTALT", "AltGr", &[]),
            // A label without a name still shows; a text label without a shortcut does not.
            labelled(2.0, "", "Page Up", &[]),
            labelled(2.1, "KEY_F1", "é", &["shift"]),
        ];
        let track = KeystrokeTrack::build(&events, &settings);
        let pills = track.sample(2.1);
        assert_eq!(pills.len(), 2);
        assert_eq!(pills[0].keys, ["Ctrl", "Shift", "A", "Ctrl", ","]);
        assert_eq!(pills[1].keys, ["Page Up"]);
    }

    // Not in the C++ tests: the label rules, including the QString/QChar ones.
    #[test]
    fn key_labels() {
        assert_eq!(key_label("KEY_A"), "A");
        assert_eq!(key_label("KEY_KP7"), "Num 7");
        assert_eq!(key_label("KEY_KPPLUS"), "Kpplus");
        assert_eq!(key_label("KEY_PAGEDOWN"), "Page Down");
        assert_eq!(key_label("KEY_VOLUME_UP"), "Volume Up");
        assert_eq!(key_label("KEY__F__12_"), "F 12");
        assert_eq!(key_label("KEY_"), "");
        assert_eq!(key_label("KEY_é"), "é");
        assert_eq!(key_label("ÉCRAN_ΣΑΣ"), "Écran Σασ");
        assert_eq!(key_label("STRASSE_ßX"), "Strasse ßx");
        assert_eq!(key_label("𐐀X"), "𐐨x");
        assert_eq!(key_label("😀"), "😀");
        assert_eq!(key_label("KP😀"), "Kp😀");
        assert!(is_modifier_name("KEY_LEFTCTRL") && is_modifier_name("KEY_ALTERASE") && !is_modifier_name("KEY_LEFT"));
    }

    #[test]
    fn plain_typing_is_a_text_key_without_ctrl_alt_or_super() {
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_A", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_1", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "ª", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_A", &["shift"])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_-", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "Ⓐ", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "𐐀", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_SEMICOLON", &["shift"])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_KP0", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_A", &["hyper"])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_A", &["ctrl"])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_A", &["shift", "alt"])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_A", &["meta"])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_ENTER", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_KPENTER", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_CAPSLOCK", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_CAPSLOCK", &["ctrl"])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_LEFTALT", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_KP", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "\u{0}", &[])));
        assert!(is_plain_typing(&labelled(0.0, "KEY_Q", "A", &["shift"])));
        assert!(is_plain_typing(&labelled(0.0, "KEY_ENTER", "é", &[])));
        assert!(!is_plain_typing(&labelled(0.0, "KEY_A", "Enter", &[])));
        assert!(!is_plain_typing(&labelled(0.0, "KEY_SPACE", "Space", &[])));
        assert!(!is_plain_typing(&labelled(0.0, "KEY_Q", "A", &["ctrl"])));
    }

    #[test]
    fn disabled_track_shows_nothing() {
        let track = KeystrokeTrack::build(&[key_down(1.0, 25, "KEY_P", &["ctrl"])], &Keystrokes::default());
        assert!(track.sample(1.0).is_empty());
    }
}
