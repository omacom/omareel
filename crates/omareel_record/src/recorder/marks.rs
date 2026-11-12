//! Markers, retakes and pauses while a recording runs: `omareel record --marker`, `--retake` and
//! `--pause`, and the recording bar's buttons.
//!
//! Markers and retakes need nothing from the recorder: the command (or the bar) appends its line
//! to the bundle's `markers.jsonl` (see [`omareel_core::recording_marks`]) and bumps a counter in
//! the state file so the bar can acknowledge it. A pause is asked for through the state file
//! (`pause_requested`, `pause_request_us`, `pause_request_via`), and the recorder daemon applies
//! it ([`PauseControl`]): the native capture stops queueing frames, so the video holds its last
//! frame and nothing on screen while paused is recorded, and the daemon writes the pause and
//! resume lines. The state then says `paused`, `paused_since_us` and `paused_total_us`, which the
//! bar's timer leaves out. Nothing here signals the recorder, so a mark can never interrupt a
//! system call the capture is in.
//!
//! At stop, [`scrub_input`] leaves out of `input.jsonl` what happened while paused, and the
//! shortcut press or bar click that made each mark, so neither shows as a keystroke, a click or
//! a zoom in the kept recording.

use super::monotonic_us;
use super::state::{self, modify_state_file, recording_state, state_file_path, update_recording_state};
use crate::capture::{DeviceEvent, DeviceEventKind, RawCursorSample};
use omareel_core::compat::qjson::Object;
use omareel_core::keystroke_track::makes_shortcut;
use omareel_core::recording_marks::{in_spans, retake_from, Mark, MarkKind, RecordingMarks, MARKS_FILE};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Appends a line to the bundle's `markers.jsonl` in one `write` on a file opened for appending,
/// so the commands and the recorder can add lines at the same time without tearing each other's.
pub fn append_mark(bundle: &str, mark: &Mark) -> Result<(), String> {
    use std::io::Write;
    let path = format!("{bundle}/{MARKS_FILE}");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|error| format!("Could not open {path}: {error}"))?;
    file.write_all(format!("{}\n", mark.to_line()).as_bytes()).map_err(|error| format!("Could not write {path}: {error}"))
}

/// The recording a mark goes to, from the state file: an error unless a recorder is running, its
/// capture has started and it is not stopping.
fn active_recording() -> Result<Object, String> {
    if !state::is_recording() {
        return Err("No recording is active".into());
    }
    let state = recording_state();
    if !state.value("capture_started").to_bool(false) {
        return Err("The recording has not started yet".into());
    }
    if state.value("stopping").to_bool(false) {
        return Err("The recording is stopping".into());
    }
    if state.value("bundle").to_string_or("").is_empty() {
        return Err("The recording has no bundle".into());
    }
    Ok(state)
}

/// While a pause is asked for, marks belong to the moment it began (the frame the video holds),
/// not to the paused time that will be cut.
fn mark_time(state: &Object, now_us: i64) -> i64 {
    let request_us = state.value("pause_request_us").to_variant_long_long();
    if state.value("pause_requested").to_bool(false) && request_us > 0 && request_us <= now_us {
        request_us
    } else {
        now_us
    }
}

/// Bumps `marks_sequence` and names the last mark, which the bar flashes.
fn acknowledge(kind: MarkKind) {
    let _ = modify_state_file(&state_file_path(), |state| {
        let sequence = state.value("marks_sequence").to_variant_long_long().wrapping_add(1);
        state.insert("marks_sequence", sequence);
        state.insert("last_mark", kind.name());
    });
}

/// `omareel record --marker [LABEL]`: a marker now. `via` is "bar" from the bar's button.
pub fn add_marker(label: &str, via: &str) -> Result<Mark, String> {
    let state = active_recording()?;
    let mut mark = Mark::new(MarkKind::Marker, mark_time(&state, monotonic_us()));
    mark.label = label.trim().to_owned();
    mark.via = via.to_owned();
    append_mark(&state.value("bundle").to_string_or(""), &mark)?;
    acknowledge(MarkKind::Marker);
    Ok(mark)
}

/// `omareel record --retake [SECONDS]`: the take since the last marker, retake or resume (or the
/// last `seconds`, five without a marker) is cut from the new project, and a retake marker left
/// where the cut ends.
pub fn add_retake(seconds: Option<f64>, via: &str) -> Result<Mark, String> {
    let state = active_recording()?;
    let bundle = state.value("bundle").to_string_or("");
    let now_us = mark_time(&state, monotonic_us());
    let marks = RecordingMarks::load_bundle(&bundle);
    let started_us = state.value("started_us").to_variant_long_long();
    let mut mark = Mark::new(MarkKind::Retake, now_us);
    mark.from_us = retake_from(now_us, &marks.marks, started_us, seconds);
    mark.via = via.to_owned();
    append_mark(&bundle, &mark)?;
    acknowledge(MarkKind::Retake);
    Ok(mark)
}

/// `omareel record --pause`: asks the recorder to pause, or to resume when a pause is already
/// asked for. True when it now asks for a pause.
pub fn toggle_pause(via: &str) -> Result<bool, String> {
    active_recording()?;
    let now_us = monotonic_us();
    let via = via.to_owned();
    modify_state_file(&state_file_path(), move |state| {
        let pause = !state.value("pause_requested").to_bool(false);
        state.insert("pause_requested", pause);
        state.insert("pause_request_us", now_us);
        state.insert("pause_request_via", via.as_str());
        pause
    })
}

/// How long a pause request's own time is trusted for; a later recorder pauses at the moment it
/// saw the request.
const REQUEST_TRUST_US: i64 = 2_000_000;

/// What the recorder does about the state's pause request when it is `paused` (or not) at
/// `now_us`: the line to write when the two differ. A pause takes the moment it was asked for, so
/// the frames captured before the recorder saw the request are cut too; a resume is the moment
/// the capture runs again.
pub fn pause_transition(paused: bool, state: &Object, now_us: i64) -> Option<Mark> {
    let requested = state.value("pause_requested").to_bool(false);
    if requested == paused {
        return None;
    }
    let mut mark = if requested {
        let request_us = state.value("pause_request_us").to_variant_long_long();
        let trusted = request_us > 0 && request_us <= now_us && now_us - request_us < REQUEST_TRUST_US;
        Mark::new(MarkKind::Pause, if trusted { request_us } else { now_us })
    } else {
        Mark::new(MarkKind::Resume, now_us)
    };
    mark.via = state.value("pause_request_via").to_string_or("");
    Some(mark)
}

/// The recorder daemon's side of pausing: how often it looks at the state file, and what it
/// keeps of the pauses so far.
pub struct PauseControl {
    bundle: String,
    paused: bool,
    since_us: i64,
    total_us: i64,
    last_check: Option<Instant>,
}

/// How often the recorder reads the state file for a pause request.
const PAUSE_CHECK_INTERVAL: Duration = Duration::from_millis(50);

impl PauseControl {
    pub fn new(bundle: &str) -> PauseControl {
        PauseControl { bundle: bundle.to_owned(), paused: false, since_us: 0, total_us: 0, last_check: None }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The paused time so far, without a pause still open.
    pub fn total_us(&self) -> i64 {
        self.total_us
    }

    /// At most every 50 ms: reads the state with `read` and, when it asks for the other state,
    /// switches, writes the mark, and publishes `paused`, `paused_since_us` and
    /// `paused_total_us`. The new state when it switched; the caller pauses or resumes the
    /// capture with it.
    pub fn poll(&mut self, read: impl FnOnce() -> Object) -> Option<bool> {
        if self.last_check.is_some_and(|last| last.elapsed() < PAUSE_CHECK_INTERVAL) {
            return None;
        }
        self.last_check = Some(Instant::now());
        let now_us = monotonic_us();
        let mark = pause_transition(self.paused, &read(), now_us)?;
        self.paused = mark.kind == MarkKind::Pause;
        if self.paused {
            self.since_us = mark.monotonic_us;
        } else {
            self.total_us += (mark.monotonic_us - self.since_us).max(0);
        }
        if let Err(error) = append_mark(&self.bundle, &mark) {
            super::write_stderr(&format!("{error}\n"));
        }
        let mut values = Object::new();
        values.insert("paused", self.paused);
        values.insert("paused_since_us", self.since_us);
        values.insert("paused_total_us", self.total_us);
        let _ = update_recording_state(&values);
        Some(self.paused)
    }
}

/// The recording's length so far without its pauses, from the state's `started_us`,
/// `paused_total_us`, and `paused_since_us` while `paused`: what the bar's timer shows.
pub fn recorded_us(state: &Object, now_us: i64) -> i64 {
    let started_us = state.value("started_us").to_variant_long_long();
    let mut paused_us = state.value("paused_total_us").to_variant_long_long();
    if state.value("paused").to_bool(false) {
        paused_us += (now_us - state.value("paused_since_us").to_variant_long_long()).max(0);
    }
    (now_us - started_us - paused_us).max(0)
}

/// How long before a mark the shortcut or click that made it may be.
const TRIGGER_WINDOW_US: i64 = 1_000_000;

fn is_modifier_key(name: &str) -> bool {
    matches!(
        name,
        "KEY_LEFTCTRL"
            | "KEY_RIGHTCTRL"
            | "KEY_LEFTSHIFT"
            | "KEY_RIGHTSHIFT"
            | "KEY_LEFTALT"
            | "KEY_RIGHTALT"
            | "KEY_LEFTMETA"
            | "KEY_RIGHTMETA"
    )
}

/// What a press and its release share: the button's name or the key's code.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Control {
    Button(String),
    Key(i32),
}

fn control(event: &DeviceEvent) -> Option<(Control, bool)> {
    match event.kind {
        DeviceEventKind::ButtonDown => Some((Control::Button(event.button.clone()), true)),
        DeviceEventKind::ButtonUp => Some((Control::Button(event.button.clone()), false)),
        DeviceEventKind::KeyDown => Some((Control::Key(event.key_code), true)),
        DeviceEventKind::KeyUp => Some((Control::Key(event.key_code), false)),
        DeviceEventKind::Scroll => None,
    }
}

/// The release of the press at `index`: the next release of the same button or key.
fn release_after(events: &[DeviceEvent], index: usize) -> Option<usize> {
    let (pressed, _) = control(&events[index])?;
    events[index + 1..]
        .iter()
        .position(|event| control(event).is_some_and(|(other, down)| !down && other == pressed))
        .map(|offset| index + 1 + offset)
}

/// Leaves out of a recording's input what its marks say should not show (nothing when there are
/// no marks, so `input.jsonl` stays as it was):
/// - while paused: cursor samples, scrolls and presses, and the releases of those presses (also
///   after the resume); releases of what was held down before the pause stay;
/// - for each mark the bar made, the last click in the second before it (the click on the
///   button);
/// - for each other mark, the last shortcut press in the second before it (the keybind that ran
///   the command), the modifiers held for it, and their releases.
pub fn scrub_input(
    cursor: &[RawCursorSample],
    devices: &[DeviceEvent],
    marks: &RecordingMarks,
) -> (Vec<RawCursorSample>, Vec<DeviceEvent>) {
    if marks.is_empty() {
        return (cursor.to_vec(), devices.to_vec());
    }
    let pauses = marks.pauses();
    let cursor = cursor.iter().filter(|sample| !in_spans(sample.monotonic_us, &pauses)).copied().collect();
    let mut events = devices.to_vec();
    events.sort_by_key(|event| event.monotonic_us);
    let mut dropped = vec![false; events.len()];
    let drop_with_release = |index: usize, dropped: &mut Vec<bool>| {
        dropped[index] = true;
        if let Some(release) = release_after(&events, index) {
            dropped[release] = true;
        }
    };
    for mark in &marks.marks {
        let in_window = |event: &DeviceEvent| {
            event.monotonic_us <= mark.monotonic_us && event.monotonic_us > mark.monotonic_us - TRIGGER_WINDOW_US
        };
        if mark.from_bar() {
            let click = (0..events.len())
                .rev()
                .find(|&i| !dropped[i] && events[i].kind == DeviceEventKind::ButtonDown && in_window(&events[i]));
            if let Some(index) = click {
                drop_with_release(index, &mut dropped);
            }
            continue;
        }
        let trigger = (0..events.len()).rev().find(|&i| {
            let event = &events[i];
            !dropped[i]
                && event.kind == DeviceEventKind::KeyDown
                && in_window(event)
                && !is_modifier_key(&event.key_name)
                && makes_shortcut(&event.modifiers)
        });
        let Some(trigger) = trigger else { continue };
        let held_modifiers: Vec<usize> = (0..trigger)
            .filter(|&i| {
                events[i].kind == DeviceEventKind::KeyDown
                    && is_modifier_key(&events[i].key_name)
                    && release_after(&events, i).is_none_or(|release| release > trigger)
            })
            .collect();
        drop_with_release(trigger, &mut dropped);
        for index in held_modifiers {
            drop_with_release(index, &mut dropped);
        }
    }
    let mut pressed_while_paused: HashSet<Control> = HashSet::new();
    for (index, event) in events.iter().enumerate() {
        if dropped[index] {
            continue;
        }
        let paused = in_spans(event.monotonic_us, &pauses);
        match control(event) {
            None => dropped[index] = paused,
            Some((pressed, true)) if paused => {
                dropped[index] = true;
                pressed_while_paused.insert(pressed);
            }
            Some((released, false)) => dropped[index] = pressed_while_paused.remove(&released),
            Some(_) => {}
        }
    }
    let events = events.into_iter().zip(dropped).filter(|(_, dropped)| !dropped).map(|(event, _)| event).collect();
    (cursor, events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use omareel_core::compat::geometry::PointF;
    use omareel_core::compat::qjson::Value;

    fn state_with(values: &[(&str, Value)]) -> Object {
        values.iter().map(|(key, value)| (*key, value.clone())).collect()
    }

    fn key(t: i64, kind: DeviceEventKind, code: i32, name: &str, mods: &[&str]) -> DeviceEvent {
        DeviceEvent {
            monotonic_us: t,
            kind,
            key_code: code,
            key_name: name.into(),
            modifiers: mods.iter().map(|m| (*m).to_owned()).collect(),
            ..DeviceEvent::default()
        }
    }

    fn button(t: i64, kind: DeviceEventKind) -> DeviceEvent {
        DeviceEvent { monotonic_us: t, kind, button: "left".into(), ..DeviceEvent::default() }
    }

    fn sample(t: i64) -> RawCursorSample {
        RawCursorSample { monotonic_us: t, logical_position: PointF::new(1.0, 2.0) }
    }

    fn times(events: &[DeviceEvent]) -> Vec<i64> {
        events.iter().map(|event| event.monotonic_us).collect()
    }

    #[test]
    fn nothing_is_scrubbed_without_marks() {
        let devices = [button(5, DeviceEventKind::ButtonDown), key(1, DeviceEventKind::KeyUp, 30, "KEY_A", &[])];
        let (cursor, events) = scrub_input(&[sample(1)], &devices, &RecordingMarks::default());
        assert_eq!(cursor, [sample(1)]);
        assert_eq!(events, devices, "not even reordered");
    }

    #[test]
    fn pauses_take_their_input_with_them() {
        use DeviceEventKind::*;
        let marks = RecordingMarks {
            marks: vec![Mark { via: "bar".into(), ..Mark::new(MarkKind::Pause, 10_000_000) }, Mark::new(MarkKind::Resume, 20_000_000)],
        };
        let devices = [
            button(2_000_000, ButtonDown),
            key(9_000_000, KeyDown, 42, "KEY_LEFTSHIFT", &["shift"]),
            button(9_500_000, ButtonDown),  // the bar's pause button
            button(9_600_000, ButtonUp),
            button(11_000_000, ButtonUp),  // released while paused, pressed before: stays
            key(12_000_000, KeyDown, 30, "KEY_A", &["ctrl"]),
            DeviceEvent { monotonic_us: 13_000_000, kind: Scroll, dy: 1, ..DeviceEvent::default() },
            key(15_000_000, KeyUp, 42, "KEY_LEFTSHIFT", &[]),
            key(21_000_000, KeyUp, 30, "KEY_A", &[]),  // pressed while paused
            button(22_000_000, ButtonDown),
        ];
        let (cursor, events) =
            scrub_input(&[sample(9_999_999), sample(10_000_000), sample(19_999_999), sample(20_000_000)], &devices, &marks);
        assert_eq!(cursor, [sample(9_999_999), sample(20_000_000)]);
        assert_eq!(times(&events), [2_000_000, 9_000_000, 11_000_000, 15_000_000, 22_000_000]);
    }

    #[test]
    fn the_keybind_that_made_a_mark_is_left_out() {
        use DeviceEventKind::*;
        let marks = RecordingMarks { marks: vec![Mark::new(MarkKind::Marker, 10_000_000)] };
        let devices = [
            key(1_000_000, KeyDown, 30, "KEY_A", &["ctrl"]),  // an earlier shortcut stays
            key(1_100_000, KeyUp, 30, "KEY_A", &[]),
            key(9_500_000, KeyDown, 125, "KEY_LEFTMETA", &["meta"]),
            key(9_600_000, KeyDown, 42, "KEY_LEFTSHIFT", &["meta", "shift"]),
            key(9_950_000, KeyDown, 50, "KEY_M", &["meta", "shift"]),
            key(10_100_000, KeyUp, 50, "KEY_M", &[]),
            key(10_150_000, KeyUp, 42, "KEY_LEFTSHIFT", &[]),
            key(10_200_000, KeyUp, 125, "KEY_LEFTMETA", &[]),
            key(12_000_000, KeyDown, 50, "KEY_M", &["meta"]),
        ];
        let (_, events) = scrub_input(&[], &devices, &marks);
        assert_eq!(times(&events), [1_000_000, 1_100_000, 12_000_000]);
    }

    #[test]
    fn the_bar_click_that_made_a_mark_is_left_out() {
        use DeviceEventKind::*;
        let mut marker = Mark::new(MarkKind::Marker, 10_000_000);
        marker.via = "bar".into();
        let marks = RecordingMarks { marks: vec![marker] };
        let devices = [
            button(8_000_000, ButtonDown),
            button(8_100_000, ButtonUp),
            button(9_800_000, ButtonDown),
            button(9_900_000, ButtonUp),
            key(9_950_000, KeyDown, 50, "KEY_M", &["meta"]),
        ];
        let (_, events) = scrub_input(&[], &devices, &marks);
        assert_eq!(times(&events), [8_000_000, 8_100_000, 9_950_000]);
    }

    #[test]
    fn pause_requests_become_pause_and_resume_marks() {
        let now = 50_000_000;
        let asked = state_with(&[
            ("pause_requested", true.into()),
            ("pause_request_us", (now - 30_000).into()),
            ("pause_request_via", "bar".into()),
        ]);
        let pause = pause_transition(false, &asked, now).unwrap();
        assert_eq!((pause.kind, pause.monotonic_us, pause.via.as_str()), (MarkKind::Pause, now - 30_000, "bar"));
        assert_eq!(pause_transition(true, &asked, now), None);
        let stale = state_with(&[("pause_requested", true.into()), ("pause_request_us", (now - 5_000_000).into())]);
        assert_eq!(pause_transition(false, &stale, now).unwrap().monotonic_us, now, "an old request pauses now");
        let resume = pause_transition(true, &state_with(&[("pause_requested", false.into())]), now).unwrap();
        assert_eq!((resume.kind, resume.monotonic_us), (MarkKind::Resume, now));
        assert_eq!(pause_transition(false, &Object::new(), now), None);
    }

    #[test]
    fn the_timer_leaves_pauses_out() {
        let running = state_with(&[("started_us", 1_000_000i64.into()), ("paused_total_us", 2_000_000i64.into())]);
        assert_eq!(recorded_us(&running, 11_000_000), 8_000_000);
        let paused = state_with(&[
            ("started_us", 1_000_000i64.into()),
            ("paused_total_us", 2_000_000i64.into()),
            ("paused", true.into()),
            ("paused_since_us", 9_000_000i64.into()),
        ]);
        assert_eq!(recorded_us(&paused, 11_000_000), 6_000_000);
        assert_eq!(recorded_us(&paused, 30_000_000), 6_000_000, "the timer stands still while paused");
    }

    #[test]
    fn marks_append_whole_lines() {
        let dir = std::env::temp_dir().join(format!("omareel-marks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bundle = dir.to_string_lossy().into_owned();
        let mut marker = Mark::new(MarkKind::Marker, 5);
        marker.label = "one".into();
        append_mark(&bundle, &marker).unwrap();
        append_mark(&bundle, &Mark::new(MarkKind::Pause, 7)).unwrap();
        let read = RecordingMarks::load_bundle(&bundle);
        assert_eq!(read.marks, [marker, Mark::new(MarkKind::Pause, 7)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
