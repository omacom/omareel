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
//! A command checks that the recording runs and appends its mark while it holds the state file's
//! lock, and the recorder says `stopping` under that lock before it reads the marks back, so a
//! mark is either seen at stop or refused ("The recording is stopping"); a command never writes
//! the state file once the recorder has removed it.
//!
//! At stop, [`scrub_input`] leaves out of `input.jsonl` what happened while paused, and the
//! keybind press or bar click that made each mark, so neither shows as a keystroke, a click or
//! a zoom in the kept recording.

use super::monotonic_us;
use super::state::{self, modify_state_file, state_file_path, update_state_file};
use crate::capture::{DeviceEvent, DeviceEventKind, RawCursorSample};
use super::files::cursor_at;
use omareel_core::compat::geometry::RectF;
use omareel_core::compat::qjson::Object;
use omareel_core::recording_marks::{
    in_spans, recorded_between, retake_from, Mark, MarkKind, RecordingMarks, MARKS_FILE,
};
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread::JoinHandle;
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

/// Why `state` (read under the lock) takes no mark: no recorder running (`alive` says whether its
/// `pid` is one), its capture not started, or stopping.
fn inactive_reason(state: &Object, alive: impl FnOnce(i64) -> bool) -> Option<&'static str> {
    if !alive(state.value("pid").to_variant_long_long()) {
        return Some("No recording is active");
    }
    if !state.value("capture_started").to_bool(false) {
        return Some("The recording has not started yet");
    }
    if state.value("stopping").to_bool(false) {
        return Some("The recording is stopping");
    }
    if state.value("bundle").to_string_or("").is_empty() {
        return Some("The recording has no bundle");
    }
    None
}

/// Runs `change` on the running recording's state under the state file's lock, and writes what it
/// changed; an error, and nothing written, unless a recording runs that takes marks.
fn with_active_recording<R>(change: impl FnOnce(&mut Object) -> Result<R, String>) -> Result<R, String> {
    if !state::is_recording() {
        return Err("No recording is active".into());
    }
    modify_state_file(&state_file_path(), |state| {
        if let Some(reason) = inactive_reason(state, state::recorder_daemon_alive) {
            return Err(reason.to_owned());
        }
        change(state)
    })
}

/// Where a mark made at `now_us` goes on the timeline: while a pause is asked for, the moment it
/// began (the frame the video holds), not the paused time that will be cut. The mark keeps
/// `now_us` as when it was made ([`Mark::made_us`]), which is where its keybind is looked for.
fn mark_time(state: &Object, now_us: i64) -> i64 {
    let request_us = state.value("pause_request_us").to_variant_long_long();
    if state.value("pause_requested").to_bool(false) && request_us > 0 && request_us <= now_us {
        request_us
    } else {
        now_us
    }
}

/// Bumps `marks_sequence` and names the last mark, which the bar flashes.
fn acknowledge(state: &mut Object, kind: MarkKind) {
    let sequence = state.value("marks_sequence").to_variant_long_long().wrapping_add(1);
    state.insert("marks_sequence", sequence);
    state.insert("last_mark", kind.name());
}

/// `omareel record --marker [LABEL]`: a marker now. `via` is "bar" from the bar's button.
pub fn add_marker(label: &str, via: &str) -> Result<Mark, String> {
    with_active_recording(|state| {
        let now_us = monotonic_us();
        let mut mark = Mark::new(MarkKind::Marker, mark_time(state, now_us));
        mark.set_made_us(now_us);
        mark.label = label.trim().to_owned();
        mark.via = via.to_owned();
        append_mark(&state.value("bundle").to_string_or(""), &mark)?;
        acknowledge(state, MarkKind::Marker);
        Ok(mark)
    })
}

/// `omareel record --retake [SECONDS]`: the take since the last marker, retake or resume (or the
/// last `seconds` of recorded time, five without a marker) is cut from the new project, and a
/// retake marker left where the cut ends. With it, how many recorded seconds the cut holds.
pub fn add_retake(seconds: Option<f64>, via: &str) -> Result<(Mark, f64), String> {
    with_active_recording(|state| {
        let bundle = state.value("bundle").to_string_or("");
        let now_us = monotonic_us();
        let marks = RecordingMarks::load_bundle(&bundle);
        let started_us = state.value("started_us").to_variant_long_long();
        let mut mark = Mark::new(MarkKind::Retake, mark_time(state, now_us));
        mark.set_made_us(now_us);
        mark.from_us = retake_from(mark.monotonic_us, &marks.marks, started_us, seconds);
        mark.via = via.to_owned();
        append_mark(&bundle, &mark)?;
        acknowledge(state, MarkKind::Retake);
        let cut_us = recorded_between(mark.from_us, mark.monotonic_us, &marks.pauses());
        Ok((mark, cut_us as f64 / 1_000_000.0))
    })
}

/// `omareel record --pause`: asks the recorder to pause, or to resume when a pause is already
/// asked for. True when it now asks for a pause.
pub fn toggle_pause(via: &str) -> Result<bool, String> {
    let via = via.to_owned();
    with_active_recording(move |state| {
        let pause = !state.value("pause_requested").to_bool(false);
        state.insert("pause_requested", pause);
        state.insert("pause_request_us", monotonic_us());
        state.insert("pause_request_via", via.as_str());
        Ok(pause)
    })
}

/// How long a pause request's own time is trusted for; a later recorder pauses at the moment it
/// saw the request.
const REQUEST_TRUST_US: i64 = 2_000_000;

/// What the recorder does about the state's pause request when it is `paused` (or not) at
/// `now_us`: the line to write when the two differ. A pause takes the moment it was asked for, so
/// the frames captured before the recorder saw the request are cut too; a resume is the moment
/// the capture runs again. Both keep the request's moment as when they were made.
pub fn pause_transition(paused: bool, state: &Object, now_us: i64) -> Option<Mark> {
    let requested = state.value("pause_requested").to_bool(false);
    if requested == paused {
        return None;
    }
    let request_us = state.value("pause_request_us").to_variant_long_long();
    let valid = request_us > 0 && request_us <= now_us;
    let mut mark = if requested {
        let trusted = valid && now_us - request_us < REQUEST_TRUST_US;
        Mark::new(MarkKind::Pause, if trusted { request_us } else { now_us })
    } else {
        Mark::new(MarkKind::Resume, now_us)
    };
    if valid {
        mark.set_made_us(request_us);
    }
    mark.via = state.value("pause_request_via").to_string_or("");
    Some(mark)
}

/// The recorder daemon's side of pausing: how often it looks at the state file, what it keeps of
/// the pauses so far, and the thread that publishes them to the state file, so the capture loop
/// never waits for the state file's lock.
pub struct PauseControl {
    bundle: String,
    state_path: String,
    paused: bool,
    since_us: i64,
    total_us: i64,
    /// When the capture itself was paused (the moments it switched, not the requests'), for
    /// `measuredFps`; an open one ends at `i64::MAX`.
    capture_pauses: Vec<(i64, i64)>,
    last_check: Option<Instant>,
    publisher: Option<(mpsc::Sender<Object>, JoinHandle<()>)>,
}

/// How often the recorder reads the state file for a pause request.
const PAUSE_CHECK_INTERVAL: Duration = Duration::from_millis(50);

impl PauseControl {
    pub fn new(bundle: &str) -> PauseControl {
        PauseControl {
            bundle: bundle.to_owned(),
            state_path: state_file_path(),
            paused: false,
            since_us: 0,
            total_us: 0,
            capture_pauses: Vec::new(),
            last_check: None,
            publisher: None,
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The paused time so far, without a pause still open.
    pub fn total_us(&self) -> i64 {
        self.total_us
    }

    /// How long the capture was paused within `from_us..to_us`.
    pub fn capture_paused_within(&self, from_us: i64, to_us: i64) -> i64 {
        self.capture_pauses.iter().map(|&(start, end)| (end.min(to_us) - start.max(from_us)).max(0)).sum()
    }

    /// At most every 50 ms: reads the state with `read` and, when it asks for the other state,
    /// switches, writes the mark, and hands `paused`, `paused_since_us` and `paused_total_us` to
    /// the publishing thread. The new state when it switched; the caller pauses or resumes the
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
            self.capture_pauses.push((now_us, i64::MAX));
        } else {
            self.total_us += (mark.monotonic_us - self.since_us).max(0);
            if let Some(open) = self.capture_pauses.last_mut() {
                open.1 = now_us;
            }
        }
        if let Err(error) = append_mark(&self.bundle, &mark) {
            super::write_stderr(&format!("{error}\n"));
        }
        let mut values = Object::new();
        values.insert("paused", self.paused);
        values.insert("paused_since_us", self.since_us);
        values.insert("paused_total_us", self.total_us);
        self.publish(values);
        Some(self.paused)
    }

    /// Hands `values` to the thread that merges them into the state file (started the first
    /// time); it waits for the lock, so the capture loop does not.
    fn publish(&mut self, values: Object) {
        let state_path = self.state_path.clone();
        let (sender, _) = self.publisher.get_or_insert_with(|| {
            let (sender, receiver) = mpsc::channel::<Object>();
            let thread = std::thread::Builder::new()
                .name("omareel-pause-state".into())
                .spawn(move || {
                    while let Ok(mut values) = receiver.recv() {
                        // Only the latest values matter.
                        while let Ok(newer) = receiver.try_recv() {
                            values = newer;
                        }
                        let _ = update_state_file(&state_path, &values);
                    }
                })
                .expect("spawn the pause state thread");
            (sender, thread)
        });
        let _ = sender.send(values);
    }

    /// Waits for the last state write, so none lands after the recorder said `stopping` or
    /// removed the state file. Called once the capture loop is over.
    pub fn finish(&mut self) {
        if let Some((sender, thread)) = self.publisher.take() {
            drop(sender);
            let _ = thread.join();
        }
    }
}

impl Drop for PauseControl {
    fn drop(&mut self) {
        self.finish();
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

/// How long before a mark was made the keybind that made it may be pressed: the command it runs
/// takes a moment to start.
const KEYBIND_WINDOW_US: i64 = 1_000_000;

/// How long after its click's release the bar's mark may be made (the bar takes the state file's
/// lock first, up to a second when it is busy).
const BAR_CLICK_WINDOW_US: i64 = 3_000_000;

/// How far outside its computed box a click still counts as on the bar (logical pixels).
const BAR_SLACK: f64 = 4.0;

/// Where the recording bar is, in global logical coordinates, for [`scrub_input`]: the bar is a
/// layer surface anchored to the top of `monitor` (logical box) and centred, 12 px below the top
/// of the area other bars leave (`reserved_top`), `width` × `height` as it published them
/// (`bar_width`, `bar_height` in the state). Its top is taken from the monitor's top so either
/// placement counts. `None` without a size.
pub fn bar_box(monitor: RectF, reserved_top: f64, width: f64, height: f64) -> Option<RectF> {
    if width <= 0.0 || height <= 0.0 || monitor.width <= 0.0 {
        return None;
    }
    let x = monitor.x + ((monitor.width - width) / 2.0).max(0.0);
    let top = monitor.y + 12.0 + reserved_top.max(0.0);
    Some(RectF::new(x - BAR_SLACK, monitor.y, width + 2.0 * BAR_SLACK, top + height + BAR_SLACK - monitor.y))
}

fn contains(rect: &RectF, x: f64, y: f64) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

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
/// - for each mark the bar made, the click on its button: the last press whose position (from
///   the cursor samples) is on the bar (`bar`, see [`bar_box`]), released before the mark was
///   made and at most three seconds before; nothing when the bar's place is unknown;
/// - for each other mark, its keybind: the last key press with Super held in the second before
///   the mark was made, the modifiers held for it, and their releases. Hyprland's global binds
///   use Super and apps' shortcuts almost never do; a mark made any other way (a bind without
///   Super, a macro key, the command typed in a terminal) leaves the keys alone.
pub fn scrub_input(
    cursor: &[RawCursorSample],
    devices: &[DeviceEvent],
    marks: &RecordingMarks,
    bar: Option<RectF>,
) -> (Vec<RawCursorSample>, Vec<DeviceEvent>) {
    if marks.is_empty() {
        return (cursor.to_vec(), devices.to_vec());
    }
    let pauses = marks.pauses();
    let kept_cursor = cursor.iter().filter(|sample| !in_spans(sample.monotonic_us, &pauses)).copied().collect();
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
        let made_us = mark.made_us();
        if mark.from_bar() {
            let Some(bar) = bar else { continue };
            let click = (0..events.len()).rev().find(|&i| {
                let event = &events[i];
                if dropped[i] || event.kind != DeviceEventKind::ButtonDown || event.monotonic_us > made_us {
                    return false;
                }
                let released_us = release_after(&events, i).map_or(event.monotonic_us, |r| events[r].monotonic_us);
                if released_us > made_us || made_us - released_us > BAR_CLICK_WINDOW_US {
                    return false;
                }
                let position = cursor_at(cursor, event.monotonic_us);
                contains(&bar, position.x, position.y)
            });
            if let Some(index) = click {
                drop_with_release(index, &mut dropped);
            }
            continue;
        }
        let trigger = (0..events.len()).rev().find(|&i| {
            let event = &events[i];
            !dropped[i]
                && event.kind == DeviceEventKind::KeyDown
                && event.monotonic_us <= made_us
                && event.monotonic_us > made_us - KEYBIND_WINDOW_US
                && !is_modifier_key(&event.key_name)
                && event.modifiers.iter().any(|modifier| modifier == "meta")
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
    (kept_cursor, events)
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

    fn at(t: i64, x: f64, y: f64) -> RawCursorSample {
        RawCursorSample { monotonic_us: t, logical_position: PointF::new(x, y) }
    }

    /// A bar over 0..10 × 0..10.
    const BAR: RectF = RectF::new(0.0, 0.0, 10.0, 10.0);

    fn times(events: &[DeviceEvent]) -> Vec<i64> {
        events.iter().map(|event| event.monotonic_us).collect()
    }

    #[test]
    fn nothing_is_scrubbed_without_marks() {
        let devices = [button(5, DeviceEventKind::ButtonDown), key(1, DeviceEventKind::KeyUp, 30, "KEY_A", &[])];
        let (cursor, events) = scrub_input(&[sample(1)], &devices, &RecordingMarks::default(), Some(BAR));
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
            scrub_input(&[sample(9_999_999), sample(10_000_000), sample(19_999_999), sample(20_000_000)], &devices, &marks, Some(BAR));
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
        let (_, events) = scrub_input(&[], &devices, &marks, None);
        assert_eq!(times(&events), [1_000_000, 1_100_000, 12_000_000]);
    }

    #[test]
    fn the_bar_click_that_made_a_mark_is_left_out() {
        use DeviceEventKind::*;
        let mut marker = Mark::new(MarkKind::Marker, 10_000_000);
        marker.via = "bar".into();
        let marks = RecordingMarks { marks: vec![marker] };
        let cursor = [at(7_900_000, 500.0, 500.0), at(9_700_000, 5.0, 5.0)];
        let devices = [
            button(8_000_000, ButtonDown),
            button(8_100_000, ButtonUp),
            button(9_800_000, ButtonDown),
            button(9_900_000, ButtonUp),
            key(9_950_000, KeyDown, 50, "KEY_M", &["meta"]),
        ];
        let (_, events) = scrub_input(&cursor, &devices, &marks, Some(BAR));
        assert_eq!(times(&events), [8_000_000, 8_100_000, 9_950_000]);
        // Clicked elsewhere just before (the bar's own click unseen): nothing goes.
        let off_bar = [at(7_900_000, 500.0, 500.0)];
        assert_eq!(scrub_input(&off_bar, &devices, &marks, Some(BAR)).1, devices);
        // Where the bar is unknown, no click goes.
        assert_eq!(scrub_input(&cursor, &devices, &marks, None).1, devices);
    }

    #[test]
    fn a_bar_button_held_long_goes_by_its_own_times() {
        use DeviceEventKind::*;
        let mut retake = Mark::new(MarkKind::Retake, 7_050_000);
        retake.via = "bar".into();
        let marks = RecordingMarks { marks: vec![retake] };
        let cursor = [at(1_000_000, 500.0, 500.0), at(4_900_000, 5.0, 5.0)];
        let devices = [
            key(4_400_000, KeyDown, 29, "KEY_LEFTCTRL", &["ctrl"]),
            key(4_500_000, KeyDown, 31, "KEY_S", &["ctrl"]),  // a real Ctrl+S stays
            key(4_550_000, KeyUp, 31, "KEY_S", &[]),
            key(4_600_000, KeyUp, 29, "KEY_LEFTCTRL", &[]),
            button(5_000_000, ButtonDown),  // held on the retake button for two seconds
            button(7_000_000, ButtonUp),
        ];
        let (_, events) = scrub_input(&cursor, &devices, &marks, Some(BAR));
        assert_eq!(times(&events), [4_400_000, 4_500_000, 4_550_000, 4_600_000]);
    }

    #[test]
    fn a_mark_made_while_paused_keeps_the_input_before_the_pause() {
        use DeviceEventKind::*;
        // Ctrl+S at 9.2 s, Super+P pauses at 9.8 s, Super+M marks at 15 s while paused: the marker
        // sits at the pause's start but was made at 15.2 s.
        let mut pause = Mark::new(MarkKind::Pause, 9_850_000);
        pause.set_made_us(9_850_000);
        let mut marker = Mark::new(MarkKind::Marker, 9_850_000);
        marker.set_made_us(15_200_000);
        let marks = RecordingMarks { marks: vec![pause, marker, Mark::new(MarkKind::Resume, 20_000_000)] };
        let devices = [
            key(9_100_000, KeyDown, 29, "KEY_LEFTCTRL", &["ctrl"]),
            key(9_200_000, KeyDown, 31, "KEY_S", &["ctrl"]),
            key(9_250_000, KeyUp, 31, "KEY_S", &[]),
            key(9_300_000, KeyUp, 29, "KEY_LEFTCTRL", &[]),
            key(9_700_000, KeyDown, 125, "KEY_LEFTMETA", &["meta"]),
            key(9_800_000, KeyDown, 25, "KEY_P", &["meta"]),
            key(9_900_000, KeyUp, 25, "KEY_P", &[]),
            key(9_950_000, KeyUp, 125, "KEY_LEFTMETA", &[]),
            key(15_000_000, KeyDown, 125, "KEY_LEFTMETA", &["meta"]),
            key(15_100_000, KeyDown, 50, "KEY_M", &["meta"]),
            button(21_000_000, ButtonDown),
        ];
        let (_, events) = scrub_input(&[], &devices, &marks, None);
        assert_eq!(times(&events), [9_100_000, 9_200_000, 9_250_000, 9_300_000, 21_000_000]);
        // A real click just before the pause stays too.
        let mut bar_pause = Mark::new(MarkKind::Pause, 9_850_000);
        bar_pause.via = "bar".into();
        let mut bar_marker = Mark::new(MarkKind::Marker, 9_850_000);
        bar_marker.via = "bar".into();
        bar_marker.set_made_us(15_200_000);
        let marks = RecordingMarks { marks: vec![bar_pause, bar_marker, Mark::new(MarkKind::Resume, 20_000_000)] };
        let cursor = [at(9_000_000, 500.0, 500.0), at(9_700_000, 5.0, 5.0), at(15_000_000, 6.0, 5.0)];
        let devices = [
            button(9_500_000, ButtonDown),  // a real click
            button(9_550_000, ButtonUp),
            button(9_750_000, ButtonDown),  // the pause button
            button(9_840_000, ButtonUp),
            button(15_100_000, ButtonDown),  // the marker button, while paused
            button(15_150_000, ButtonUp),
        ];
        let (_, events) = scrub_input(&cursor, &devices, &marks, Some(BAR));
        assert_eq!(times(&events), [9_500_000, 9_550_000]);
    }

    #[test]
    fn only_a_super_keybind_is_taken_for_a_trigger() {
        use DeviceEventKind::*;
        let marks = RecordingMarks { marks: vec![Mark::new(MarkKind::Marker, 10_050_000)] };
        // A bind on F9 (no modifier) after a real Ctrl+C: both stay.
        let f9 = [
            key(9_300_000, KeyDown, 29, "KEY_LEFTCTRL", &["ctrl"]),
            key(9_400_000, KeyDown, 46, "KEY_C", &["ctrl"]),
            key(9_450_000, KeyUp, 46, "KEY_C", &[]),
            key(9_500_000, KeyUp, 29, "KEY_LEFTCTRL", &[]),
            key(10_000_000, KeyDown, 67, "KEY_F9", &[]),
            key(10_040_000, KeyUp, 67, "KEY_F9", &[]),
        ];
        assert_eq!(scrub_input(&[], &f9, &marks, None).1, f9);
        // The command typed in a terminal: Ctrl+U to clear the line, then Enter.
        let typed = [
            key(9_200_000, KeyDown, 29, "KEY_LEFTCTRL", &["ctrl"]),
            key(9_300_000, KeyDown, 22, "KEY_U", &["ctrl"]),
            key(9_350_000, KeyUp, 22, "KEY_U", &[]),
            key(9_400_000, KeyUp, 29, "KEY_LEFTCTRL", &[]),
            key(9_900_000, KeyDown, 28, "KEY_ENTER", &[]),
        ];
        assert_eq!(scrub_input(&[], &typed, &marks, None).1, typed);
        // A Super press more than a second before the mark is not its keybind.
        let late = [key(9_000_000, KeyDown, 50, "KEY_M", &["meta"])];
        assert_eq!(scrub_input(&[], &late, &marks, None).1, late);
    }

    #[test]
    fn the_bar_sits_centred_below_the_reserved_top() {
        let monitor = RectF::new(1920.0, 0.0, 2560.0, 1440.0);
        let bar = bar_box(monitor, 26.0, 400.0, 40.0).unwrap();
        assert_eq!((bar.x, bar.y, bar.width, bar.height), (1920.0 + 1080.0 - 4.0, 0.0, 408.0, 26.0 + 12.0 + 40.0 + 4.0));
        assert!(contains(&bar, 3100.0, 60.0) && !contains(&bar, 3100.0, 90.0) && !contains(&bar, 2990.0, 60.0));
        assert_eq!(bar_box(monitor, 0.0, 0.0, 40.0), None);
    }

    fn scratch_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("omareel-marks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn a_pause_never_waits_for_the_state_lock() {
        use omareel_core::compat::qlockfile::LockFile;
        let dir = scratch_dir("lock");
        let state_path = format!("{dir}/recording.json");
        std::fs::write(&state_path, "{\"pid\": 1}").unwrap();
        let mut control = PauseControl::new(&dir);
        control.state_path = state_path.clone();
        let mut held = LockFile::new(format!("{state_path}.lock"));
        assert!(held.try_lock(0));
        let asked = state_with(&[("pause_requested", true.into()), ("pause_request_us", monotonic_us().into())]);
        let started = Instant::now();
        assert_eq!(control.poll(|| asked.clone()), Some(true));
        assert!(started.elapsed() < Duration::from_millis(100), "poll waited {:?}", started.elapsed());
        assert!(control.paused());
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
        control.finish();
        let state = state::read_state_file(&state_path);
        assert!(state.value("paused").to_bool(false), "published once the lock was free");
        assert_eq!(state.value("pid").to_variant_long_long(), 1);
        assert_eq!(RecordingMarks::load_bundle(&dir).marks.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refused_change_writes_nothing() {
        let dir = scratch_dir("refused");
        let state_path = format!("{dir}/recording.json");
        let refused: Result<(), String> = modify_state_file(&state_path, |state| {
            state.insert("marks_sequence", 1);
            match inactive_reason(state, state::recorder_daemon_alive) {
                Some(reason) => Err(reason.to_owned()),
                None => Ok(()),
            }
        });
        assert_eq!(refused, Err("No recording is active".to_owned()));
        assert!(!std::path::Path::new(&state_path).exists(), "a removed state file stays removed");
        let mut state = state_with(&[("pid", 7.into()), ("capture_started", true.into()), ("bundle", "/b".into())]);
        assert_eq!(inactive_reason(&state, |pid| pid == 7), None);
        assert_eq!(inactive_reason(&state, |_| false), Some("No recording is active"));
        state.insert("stopping", true);
        assert_eq!(inactive_reason(&state, |pid| pid == 7), Some("The recording is stopping"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn measured_time_leaves_the_capture_pauses_out() {
        let mut control = PauseControl::new("/nonexistent");
        control.capture_pauses = vec![(2_000_000, 5_000_000), (8_000_000, i64::MAX)];
        assert_eq!(control.capture_paused_within(1_000_000, 10_000_000), 3_000_000 + 2_000_000);
        assert_eq!(control.capture_paused_within(3_000_000, 4_000_000), 1_000_000);
        assert_eq!(control.capture_paused_within(6_000_000, 7_000_000), 0);
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
        assert_eq!(pause.at_us, 0, "made when it was asked for: no separate at");
        assert_eq!(pause_transition(true, &asked, now), None);
        let stale = state_with(&[("pause_requested", true.into()), ("pause_request_us", (now - 5_000_000).into())]);
        let late = pause_transition(false, &stale, now).unwrap();
        assert_eq!((late.monotonic_us, late.made_us()), (now, now - 5_000_000), "an old request pauses now");
        let resume = pause_transition(true, &state_with(&[("pause_requested", false.into())]), now).unwrap();
        assert_eq!((resume.kind, resume.monotonic_us), (MarkKind::Resume, now));
        let asked_resume = state_with(&[("pause_requested", false.into()), ("pause_request_us", (now - 40_000).into())]);
        assert_eq!(pause_transition(true, &asked_resume, now).unwrap().made_us(), now - 40_000);
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
