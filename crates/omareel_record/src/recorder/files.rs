//! The files `Recorder.cpp`'s daemon writes and reads besides the state files: the `*.ts`
//! timestamp files (`firstFrameTimestamp`, `writeStartTimestamp`), `input.jsonl`
//! (`writeInputLog`, with `positionObject` and `cursorAt`), the bundle's JSON files
//! (`writeJson`), and the tail of gpu-screen-recorder's output (`drainRecorderOutput`,
//! `lastNonEmptyLine`).
//!
//! Every writer goes through `QSaveFile`, so a file appears whole or not at all, a replaced file
//! keeps its mode, and a failure reports Qt's message.

use super::realtime_us;
use crate::capture::{CaptureRegion, DeviceEvent, DeviceEventKind, RawCursorSample};
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qjson::{Format, Object, Value};
use omareel_core::compat::qprocess::Process;
use omareel_core::compat::qsavefile::SaveFile;
use omareel_core::compat::{qfile, qnumber, qstring};
use std::io::Write;

/// `QByteArray::toLongLong(&ok)` in base 10: the bytes read as Latin-1, so anything but ASCII
/// fails, and Qt stops at a NUL.
fn byte_array_to_long_long(bytes: &[u8]) -> Option<i64> {
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
    qnumber::to_long_long(&text)
}

/// The file-static `firstFrameTimestamp`: the first line of a timestamp file whose first
/// TAB-separated field (after trimming the line) is a whole number, or 0 when no line has one
/// or the file can't be read. Both gpu-screen-recorder's one-number files and omareel's
/// two-column ones (`monotonic_microsec\trealtime_microsec`, header first) read this way.
pub fn first_frame_timestamp(path: &str) -> i64 {
    // QFile opened ReadOnly | Text: readLine only turns a final "\r\n" into "\n", which the
    // trim removes anyway.
    let Ok(bytes) = qfile::read_all(path) else { return 0 };
    for line in qfile::lines(&bytes) {
        let first = qfile::trimmed(line).split(|&b| b == b'\t').next().unwrap_or_default();
        if let Some(value) = byte_array_to_long_long(first) {
            return value;
        }
    }
    0
}

/// The bytes of an omareel timestamp file: the header, then the monotonic and realtime
/// microseconds.
pub fn timestamp_file_contents(monotonic_us: i64, realtime_us: i64) -> String {
    format!("monotonic_microsec\trealtime_microsec\n{monotonic_us}\t{realtime_us}\n")
}

/// The file-static `writeStartTimestamp`: writes the timestamp file for `monotonic_us`, with the
/// wall clock read now as its realtime column. False when the file could not be written.
pub fn write_start_timestamp(path: &str, monotonic_us: i64) -> bool {
    write_timestamp(path, monotonic_us, realtime_us())
}

/// [`write_start_timestamp`] with the realtime value given.
pub fn write_timestamp(path: &str, monotonic_us: i64, realtime_us: i64) -> bool {
    let mut file = SaveFile::new(path);
    if !file.open() {
        return false;
    }
    // QTextStream writes it all as the device closes, before commit renames it.
    file.write(timestamp_file_contents(monotonic_us, realtime_us).as_bytes());
    file.commit()
}

/// The file-static `positionObject`: a logical point in the capture's pixels,
/// `{"x": (x - region.x) * scale, "y": (y - region.y) * scale}`.
pub fn position_object(logical: PointF, region: &CaptureRegion) -> Object {
    let mut object = Object::new();
    object.insert("x", (logical.x - region.x) * region.scale);
    object.insert("y", (logical.y - region.y) * region.scale);
    object
}

/// The file-static `cursorAt`: where the pointer was at `time`, the last sample at or before it
/// (the first sample for an earlier time, the origin without samples). The samples are in time
/// order; of several at one time the last counts.
pub fn cursor_at(samples: &[RawCursorSample], time: i64) -> PointF {
    if samples.is_empty() {
        return PointF::default();
    }
    // std::upper_bound: the first sample after `time`.
    let after = samples.partition_point(|sample| sample.monotonic_us <= time);
    if after == 0 { samples[0].logical_position } else { samples[after - 1].logical_position }
}

/// The lines of `input.jsonl` that [`write_input_log`] writes, each a compact JSON object, in
/// time order (a stable sort, so cursor samples come before device events at the same time).
/// Moves are `{t, k: "m", x, y}`; buttons `{t, k: "d"|"u", b, x, y}` and scrolls `{t, k: "s", dx,
/// dy, x, y}` at the cursor's position then; keys `{t, k: "kd"|"ku", code}` with `name` and `mods`
/// on key down, and `label` too when the key has one. Positions are in the capture's pixels
/// ([`position_object`]).
pub fn input_log_lines(cursor: &[RawCursorSample], devices: &[DeviceEvent], region: &CaptureRegion) -> Vec<String> {
    let mut all: Vec<(i64, Object)> = Vec::with_capacity(cursor.len() + devices.len());
    let with_position = |object: &mut Object, logical: PointF| {
        let position = position_object(logical, region);
        object.insert("x", position.value("x").clone());
        object.insert("y", position.value("y").clone());
    };
    for sample in cursor {
        let mut object = Object::new();
        object.insert("t", sample.monotonic_us);
        object.insert("k", "m");
        with_position(&mut object, sample.logical_position);
        all.push((sample.monotonic_us, object));
    }
    for event in devices {
        let mut object = Object::new();
        object.insert("t", event.monotonic_us);
        match event.kind {
            DeviceEventKind::ButtonDown | DeviceEventKind::ButtonUp => {
                object.insert("k", if event.kind == DeviceEventKind::ButtonDown { "d" } else { "u" });
                object.insert("b", event.button.as_str());
                with_position(&mut object, cursor_at(cursor, event.monotonic_us));
            }
            DeviceEventKind::Scroll => {
                object.insert("k", "s");
                object.insert("dx", event.dx);
                object.insert("dy", event.dy);
                with_position(&mut object, cursor_at(cursor, event.monotonic_us));
            }
            DeviceEventKind::KeyDown | DeviceEventKind::KeyUp => {
                let down = event.kind == DeviceEventKind::KeyDown;
                object.insert("k", if down { "kd" } else { "ku" });
                object.insert("code", event.key_code);
                if down {
                    object.insert("name", event.key_name.as_str());
                    if !event.key_label.is_empty() {
                        object.insert("label", event.key_label.as_str());
                    }
                    let mods: Vec<Value> = event.modifiers.iter().map(|m| Value::from(m.as_str())).collect();
                    object.insert("mods", mods);
                }
            }
        }
        all.push((event.monotonic_us, object));
    }
    all.sort_by_key(|(time, _)| *time);
    all.into_iter().map(|(_, object)| Value::Object(object).to_json(Format::Compact)).collect()
}

/// The file-static `writeInputLog`: `input.jsonl` from the cursor samples and device events
/// ([`input_log_lines`], each followed by a newline). `Err` holds the message the C++ put in its
/// error (which it leaves alone on success).
pub fn write_input_log(
    path: &str,
    cursor: &[RawCursorSample],
    devices: &[DeviceEvent],
    region: &CaptureRegion,
) -> Result<(), String> {
    let lines = input_log_lines(cursor, devices, region);
    let mut file = SaveFile::new(path);
    if !file.open() {
        return Err(file.error_string());
    }
    for line in lines {
        file.write(format!("{line}\n").as_bytes());
    }
    if !file.commit() {
        return Err(file.error_string());
    }
    Ok(())
}

/// The file-static `writeJson`: the object as indented JSON (`capture.json`, `recording.json`,
/// the state file). `Err` holds the message the C++ put in its error.
pub fn write_json(path: &str, object: &Object) -> Result<(), String> {
    let mut file = SaveFile::new(path);
    if !file.open() {
        return Err(file.error_string());
    }
    file.write(Value::Object(object.clone()).to_json(Format::Indented).as_bytes());
    if !file.commit() {
        return Err(file.error_string());
    }
    Ok(())
}

/// How much of gpu-screen-recorder's output `drainRecorderOutput` keeps.
pub const RECORDER_OUTPUT_TAIL: usize = 4096;

/// The file-static `drainRecorderOutput`: what the recorder printed since the last call (its
/// merged channels), copied to the debug log when one is open (and flushed), and appended to
/// `tail`, of which the last [`RECORDER_OUTPUT_TAIL`] bytes are kept.
pub fn drain_recorder_output(process: &mut Process, tail: &mut Vec<u8>, debug_log: Option<&mut dyn Write>) {
    append_recorder_output(&process.read_all(), tail, debug_log);
}

/// [`drain_recorder_output`] for a chunk already read.
pub fn append_recorder_output(chunk: &[u8], tail: &mut Vec<u8>, debug_log: Option<&mut dyn Write>) {
    if chunk.is_empty() {
        return;
    }
    if let Some(log) = debug_log {
        // QFile::write and flush, whose failures the C++ ignores.
        let _ = log.write_all(chunk);
        let _ = log.flush();
    }
    tail.extend_from_slice(chunk);
    if tail.len() > RECORDER_OUTPUT_TAIL {
        tail.drain(..tail.len() - RECORDER_OUTPUT_TAIL);
    }
}

/// The file-static `lastNonEmptyLine`: the last line of the output that is not blank once
/// decoded as UTF-8 and trimmed, the reason a failed recorder gives; empty when there is none.
pub fn last_non_empty_line(output: &[u8]) -> String {
    for line in output.split(|&b| b == b'\n').rev() {
        let text = qstring::from_utf8(line);
        let line = qstring::trimmed(&text);
        if !line.is_empty() {
            return line.to_owned();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not in the C++: a key down's layout label, written only when it has one.
    #[test]
    fn key_downs_carry_their_label() {
        let key = |monotonic_us, kind, key_label: &str| DeviceEvent {
            monotonic_us,
            kind,
            key_code: 16,
            key_name: "KEY_Q".into(),
            key_label: key_label.into(),
            modifiers: vec!["ctrl".into()],
            ..DeviceEvent::default()
        };
        let devices = [
            key(1, DeviceEventKind::KeyDown, "A"),
            key(2, DeviceEventKind::KeyUp, "A"),
            key(3, DeviceEventKind::KeyDown, ""),
        ];
        assert_eq!(
            input_log_lines(&[], &devices, &CaptureRegion::default()),
            [
                r#"{"code":16,"k":"kd","label":"A","mods":["ctrl"],"name":"KEY_Q","t":1}"#,
                r#"{"code":16,"k":"ku","t":2}"#,
                r#"{"code":16,"k":"kd","mods":["ctrl"],"name":"KEY_Q","t":3}"#,
            ]
        );
    }
}
