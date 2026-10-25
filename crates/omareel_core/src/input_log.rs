//! The recorder's input log, `input.jsonl` (`InputLog`).

use crate::compat::geometry::PointF;
use crate::compat::{qfile, qjson};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputKind {
    #[default]
    Move,
    ButtonDown,
    ButtonUp,
    Scroll,
    KeyDown,
    KeyUp,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputEvent {
    pub monotonic_us: i64,
    /// Seconds since the first captured frame.
    pub time: f64,
    pub kind: InputKind,
    pub position: PointF,
    pub button: String,
    pub dx: f64,
    pub dy: f64,
    pub key_code: i32,
    pub key_name: String,
    pub modifiers: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputLog {
    events: Vec<InputEvent>,
}

fn kind_for(kind: &str) -> InputKind {
    match kind {
        "d" => InputKind::ButtonDown,
        "u" => InputKind::ButtonUp,
        "s" => InputKind::Scroll,
        "kd" => InputKind::KeyDown,
        "ku" => InputKind::KeyUp,
        _ => InputKind::Move,
    }
}

impl InputLog {
    /// Reads `input.jsonl`, keeping events from `first_frame_us` to `duration` seconds later
    /// (no end when `duration` is negative). An unreadable file or an invalid line gives an
    /// error, and no events.
    pub fn load(input_path: impl AsRef<Path>, first_frame_us: i64, duration: f64) -> Result<InputLog, String> {
        let bytes = qfile::read_all(input_path)?;
        let mut result = InputLog::default();
        for (index, line) in qfile::lines(&bytes).enumerate() {
            let line = qfile::trimmed(line);
            if line.is_empty() {
                continue;
            }
            let object = match qjson::from_json(line) {
                Ok(qjson::Value::Object(object)) => object,
                Ok(_) => return Err(format!("Invalid JSON on line {}: no error occurred", index + 1)),
                Err(error) => return Err(format!("Invalid JSON on line {}: {}", index + 1, error.error_string())),
            };
            let monotonic_us = object.value("t").to_variant_long_long();
            let time = monotonic_us.wrapping_sub(first_frame_us) as f64 / 1_000_000.0;
            if time < 0.0 || (duration >= 0.0 && time > duration) {
                continue;
            }
            result.events.push(InputEvent {
                monotonic_us,
                time,
                kind: kind_for(&object.value("k").to_string_or("")),
                position: PointF::new(object.value("x").to_double(0.0), object.value("y").to_double(0.0)),
                button: object.value("b").to_string_or(""),
                dx: object.value("dx").to_double(0.0),
                dy: object.value("dy").to_double(0.0),
                key_code: object.value("code").to_int(0),
                key_name: object.value("name").to_string_or(""),
                modifiers: object.value("mods").to_array().iter().map(|m| m.to_string_or("")).collect(),
            });
        }
        Ok(result)
    }

    /// Reads a bundle's input log, timed from `capture.json`'s first frame. Without a
    /// `duration`, events after the recording stopped are dropped.
    pub fn load_bundle(bundle_path: &str, duration: f64) -> Result<InputLog, String> {
        let capture = qfile::read_all(format!("{bundle_path}/capture.json"))?;
        let object = qjson::object_from_json(&capture).unwrap_or_default();
        let first = object.value("first_frame_us").to_variant_long_long();
        let mut duration = duration;
        if duration < 0.0 {
            let stopped = object.value("stopped_us").to_variant_long_long();
            if stopped > first {
                duration = stopped.wrapping_sub(first) as f64 / 1_000_000.0;
            }
        }
        InputLog::load(format!("{bundle_path}/input.jsonl"), first, duration)
    }

    pub fn events(&self) -> &[InputEvent] {
        &self.events
    }

    pub fn moves(&self) -> Vec<InputEvent> {
        self.events.iter().filter(|e| e.kind == InputKind::Move).cloned().collect()
    }

    /// Button presses, optionally only the left and right buttons.
    pub fn click_downs(&self, left_right_only: bool) -> Vec<InputEvent> {
        self.events
            .iter()
            .filter(|e| e.kind == InputKind::ButtonDown)
            .filter(|e| !left_right_only || e.button == "left" || e.button == "right")
            .cloned()
            .collect()
    }

    pub fn key_count(&self) -> usize {
        self.events.iter().filter(|e| e.kind == InputKind::KeyDown).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");

    // tests/test_inputlog.cpp
    #[test]
    fn parses_converts_and_trims() {
        let log = InputLog::load(format!("{FIXTURES}/input.jsonl"), 1_000_000, 1.0).unwrap();
        assert_eq!(log.events().len(), 4);
        assert_eq!(log.moves().len(), 1);
        assert_eq!(log.events()[0].time, 0.0);
        assert_eq!(log.click_downs(false).len(), 1);
        assert_eq!(log.key_count(), 1);
        assert_eq!(log.events()[2].modifiers, ["ctrl"]);
    }

    #[test]
    fn a_bad_line_or_missing_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("omareel-inputlog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input.jsonl");
        std::fs::write(&path, "{\"t\":1}\n\n{\"t\":2\n").unwrap();
        assert_eq!(InputLog::load(&path, 0, -1.0).unwrap_err(), "Invalid JSON on line 3: unterminated object");
        assert_eq!(InputLog::load(dir.join("missing"), 0, -1.0).unwrap_err(), "No such file or directory");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
