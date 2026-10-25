//! The keystroke pills shown over the recording (`KeystrokeTrack`).
//!
//! Key names are `QString`s in the C++, so lengths count UTF-16 code units (`qstring::size`),
//! lowercasing is `QString::toLower` and capitalising a word changes its first UTF-16 code unit
//! with `QChar::toUpper`, which leaves a word starting outside the Basic Multilingual Plane
//! alone. `KEY_*` names from evdev are ASCII, where all of this is plain ASCII.

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

fn is_modifier(name: &str) -> bool {
    name.contains("CTRL") || name.contains("SHIFT") || name.contains("ALT") || name.contains("META")
}

fn modifier_label(modifier: &str) -> &'static str {
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

fn is_plain_typing(event: &InputEvent) -> bool {
    if !event.modifiers.is_empty() {
        return false;
    }
    let name = event.key_name.strip_prefix("KEY_").unwrap_or(&event.key_name);
    qstring::size(name) == 1 && name.encode_utf16().next().is_some_and(qchar::is_letter_or_number)
}

impl KeystrokeTrack {
    /// Key presses grouped into pills: a press joins the last pill when it comes within 0.6 s of
    /// that pill's last press and the pill stays at six keys or fewer.
    pub fn build(events: &[InputEvent], settings: &Keystrokes) -> KeystrokeTrack {
        let mut track = KeystrokeTrack { settings: settings.clone(), groups: Vec::new() };
        if !settings.enabled {
            return track;
        }
        for event in events {
            if event.kind != InputKind::KeyDown
                || event.key_name.is_empty()
                || is_modifier(&event.key_name)
                || (settings.show_only_shortcuts && is_plain_typing(event))
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
            let label = key_label(&event.key_name);
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

    // tests/test_keystroketrack.cpp
    #[test]
    fn groups_consecutive_keys_and_fades_from_last_key() {
        let settings = Keystrokes { enabled: true, show_only_shortcuts: false, hold_ms: 900, ..Keystrokes::default() };
        let events = [key_down(1.0, 25, "KEY_P", &["ctrl", "shift"]), key_down(1.5, 37, "KEY_K", &[])];
        let track = KeystrokeTrack::build(&events, &settings);
        let grouped = track.sample(1.5);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].keys, ["Ctrl", "Shift", "P", "K"]);
        assert_eq!(grouped[0].text, "Ctrl + Shift + P + K");
        assert!(qcompare(grouped[0].opacity, 1.0));

        let fading = track.sample(2.525);
        assert_eq!(fading.len(), 1);
        assert!((fading[0].opacity - 0.5).abs() < 0.001);
        assert!(track.sample(2.76).is_empty());
    }

    #[test]
    fn shortcut_filter_and_six_key_limit() {
        let settings = Keystrokes { enabled: true, show_only_shortcuts: true, ..Keystrokes::default() };
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
        assert!(is_modifier("KEY_LEFTCTRL") && is_modifier("KEY_ALTERASE") && !is_modifier("KEY_LEFT"));
    }

    #[test]
    fn plain_typing_is_one_letter_or_digit_without_modifiers() {
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_A", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "KEY_1", &[])));
        assert!(is_plain_typing(&key_down(0.0, 0, "ª", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_A", &["shift"])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_ENTER", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "KEY_-", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "Ⓐ", &[])));
        assert!(!is_plain_typing(&key_down(0.0, 0, "𐐀", &[])));
    }

    #[test]
    fn disabled_track_shows_nothing() {
        let track = KeystrokeTrack::build(&[key_down(1.0, 25, "KEY_P", &["ctrl"])], &Keystrokes::default());
        assert!(track.sample(1.0).is_empty());
    }
}
