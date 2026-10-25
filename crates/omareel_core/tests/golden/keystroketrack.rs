//! keystroketrack: `keystroke_track::KeystrokeTrack` against `Omareel::KeystrokeTrack` (README
//! "keystroketrack").

use crate::support::*;
use omareel_core::compat::cpp;
use omareel_core::compat::qjson::Value;
use omareel_core::input_log::{InputKind, InputLog};
use omareel_core::keystroke_track::KeystrokeTrack;
use omareel_core::project::Keystrokes;

/// The oracle's `keystrokesFromJson`: each key given replaces the struct's default, written
/// straight in (no clamping of `holdMs`).
fn keystrokes_from_json(o: &Value) -> Keystrokes {
    let o = o.to_object();
    let mut k = Keystrokes::default();
    if o.contains("enabled") {
        k.enabled = o.value("enabled").to_bool(false);
    }
    if o.contains("position") {
        k.position = o.value("position").to_string_or("");
    }
    if o.contains("size") {
        k.size = o.value("size").to_double(0.0);
    }
    if o.contains("showOnlyShortcuts") {
        k.show_only_shortcuts = o.value("showOnlyShortcuts").to_bool(false);
    }
    if o.contains("holdMs") {
        k.hold_ms = o.value("holdMs").to_int(0);
    }
    k
}

/// `single-characters.jsonl` names keys `"\ud800"` and `"KEY_\udc00"`, which the port's JSON
/// parser reads as U+FFFD (a Rust string cannot hold a lone surrogate). Both are single UTF-16
/// units that are not letters or digits, so they are kept and labelled the same way, and the
/// golden's `\u{d800}` and `\u{dc00}` cells (in 124 rows) are compared as U+FFFD.
#[test]
fn sample_txt() {
    let cases = read_json("keystroketrack/cases.json");
    let settings = cases.get("settings").to_object();
    let mut out = Text::new();
    for run in cases.get("runs").to_array() {
        let run = run.to_array();
        let log = run[0].to_string_or("");
        let input = InputLog::load(corpus_path(&log), 1_000_000, -1.0).unwrap_or_else(|e| panic!("{log}: {e}"));
        let key_times: Vec<f64> =
            input.events().iter().filter(|e| e.kind == InputKind::KeyDown).map(|e| e.time).collect();
        let last = key_times.iter().copied().reduce(cpp::max).unwrap_or(0.0);
        for name in run[1].to_array() {
            let name = name.to_string_or("");
            assert!(settings.contains(&name), "keystroketrack: no settings {name}");
            let keystrokes = keystrokes_from_json(settings.value(&name));
            let track = KeystrokeTrack::build(input.events(), &keystrokes);
            out.section(&format!("{log} settings={name}"));
            let hold = f64::from(keystrokes.hold_ms) / 1000.0;
            let end = last + cpp::min(cpp::max(hold, 0.0), 2.0) + 0.5;
            let mut times = uniform(10, -5, (10.0 * end).ceil() as i32);
            for &e in &key_times {
                times.extend([before(e), e, after(e)]);
                let fade_end = e + hold + 0.25;
                if fade_end >= -0.5 && fade_end <= end {
                    times.extend([e + hold, before(fade_end), fade_end, after(fade_end)]);
                }
            }
            for t in sorted_unique(times) {
                let pills = track.sample(t);
                let mut cells = vec![num(t), pills.len().to_string()];
                for pill in &pills {
                    cells.extend([quote_list(&pill.keys), quote(&pill.text), num(pill.opacity)]);
                }
                out.row(cells);
            }
        }
    }
    let expected = lone_surrogates_as_replacement(&read_expected("keystroketrack/sample.txt"));
    assert_same("keystroketrack/sample.txt (lone surrogates as U+FFFD)", &expected, out.into_string().as_bytes());
}
