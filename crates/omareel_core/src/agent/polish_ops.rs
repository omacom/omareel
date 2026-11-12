//! The `omareel apply` operations for annotations, markers, transitions, quiet parts, reframing
//! and audio cleanup ([`super::apply`] has the table and the conventions they share).
//!
//! Overlays keep their two anchors: a `screen` item belongs to the recording (its `rect` is 0–1
//! of the recording's full frame, before the crop, and it is stored at SOURCE times), a `frame`
//! item belongs to the canvas (0–1 of the output frame, stored at OUTPUT times). The operations
//! take `from`/`to` on the op's timeline as every other operation does and convert them to the
//! item's own time base; a `screen` item over output times gets the recording span those output
//! times show.

use super::apply::{clip_id, number, ok, optional_number, output_time, source_timeline, text, with, OpResult};
use super::{ms, source_span_outputs, source_time_outputs};
use crate::compat::cpp;
use crate::compat::qcolor::QColor;
use crate::compat::qjson::{Object, Value};
use crate::editor::Document;
use crate::project::{AudioCleanup, Overlay, BLUR_STYLES, MIN_OVERLAY_LENGTH, OVERLAY_ANCHORS, OVERLAY_KINDS, TEXT_BACKGROUNDS};
use crate::transcript::speedup::SpeedUpSettings;
use crate::transitions;

/// The fields of an overlay's kind an operation may set, as the op names them (`tail` and
/// `head` are an arrow's ends, project.json's `from` and `to`).
fn kind_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "text" => &["text", "size", "color", "background", "fadeIn", "fadeOut", "rect"],
        "box" => &["color", "width", "radius", "spotlight", "fadeIn", "fadeOut", "rect"],
        "arrow" => &["tail", "head", "color", "width", "fadeIn", "fadeOut"],
        "badge" => &["number", "color", "fadeIn", "fadeOut", "rect"],
        "blur" => &["style", "amount", "rect"],
        "card" => &["title", "subtitle", "fadeIn", "fadeOut"],
        _ => &[],
    }
}

/// Fields every overlay operation may have besides its kind's.
const COMMON_FIELDS: [&str; 6] = ["op", "id", "kind", "anchor", "from", "to"];

fn check_fields(op: &Object, kind: &str) -> Result<(), String> {
    let own = kind_fields(kind);
    for key in op.keys() {
        if key == "timeline" || COMMON_FIELDS.contains(&key) || own.contains(&key) {
            continue;
        }
        return Err(format!("a {kind} overlay has no \"{key}\" (it takes {})", own.join(", ")));
    }
    Ok(())
}

fn unit(value: f64) -> bool {
    (-1e-9..=1.0 + 1e-9).contains(&value)
}

/// `{x, y}` from 0 to 1.
fn point(op: &Object, key: &str) -> Result<Value, String> {
    let invalid = || format!("\"{key}\" must be {{\"x\": 0–1, \"y\": 0–1}}");
    let object = op.value(key).as_object().ok_or_else(invalid)?;
    let x = number(object, "x").map_err(|_| invalid())?;
    let y = number(object, "y").map_err(|_| invalid())?;
    if !unit(x) || !unit(y) {
        return Err(invalid());
    }
    Ok(Object::from_iter([("x", Value::from(x)), ("y", y.into())]).into())
}

/// `rect`: `{x, y, w, h}`, 0–1 and inside the frame; `partial` lets an edit give only some.
fn rect(op: &Object, partial: bool) -> Result<Value, String> {
    let invalid = || "\"rect\" must be {\"x\", \"y\", \"w\", \"h\"}, each 0–1, inside the frame".to_string();
    let object = op.value("rect").as_object().ok_or_else(invalid)?;
    let mut result = Object::new();
    for key in ["x", "y", "w", "h"] {
        match optional_number(object, key).map_err(|_| invalid())? {
            Some(value) if unit(value) && (value > 0.0 || matches!(key, "x" | "y")) => result.insert(key, value),
            Some(_) => return Err(invalid()),
            None if partial => {}
            None => return Err(invalid()),
        }
    }
    let fits = |at: &str, size: &str| {
        let (at, size) = (result.value(at), result.value(size));
        !(at.is_double() && size.is_double()) || at.to_double(0.0) + size.to_double(0.0) <= 1.0 + 1e-6
    };
    if !fits("x", "w") || !fits("y", "h") {
        return Err(invalid());
    }
    Ok(result.into())
}

fn ranged(op: &Object, key: &str, low: f64, high: f64) -> Result<Value, String> {
    let value = number(op, key)?;
    if !(low..=high).contains(&value) {
        return Err(format!("\"{key}\" must be from {low} to {high}"));
    }
    Ok(value.into())
}

/// The op's look fields as project.json writes them, checked.
fn look_patch(op: &Object, kind: &str, partial: bool) -> Result<Object, String> {
    let mut patch = Object::new();
    for &key in kind_fields(kind) {
        if op.value(key).is_undefined() {
            continue;
        }
        let value = match key {
            "text" | "title" | "subtitle" => Value::from(text(op, key)?),
            "color" => {
                let color = text(op, key)?;
                if !color.is_empty() && !QColor::parse(&color).is_valid() {
                    return Err("\"color\" must be a colour such as \"#ff5500\" (or \"\" for the theme's)".into());
                }
                color.into()
            }
            "background" => {
                let background = text(op, key)?;
                if !TEXT_BACKGROUNDS.contains(&background.as_str()) {
                    return Err(format!("\"background\" must be one of {}", TEXT_BACKGROUNDS.join(", ")));
                }
                background.into()
            }
            "style" => {
                let style = text(op, key)?;
                if !BLUR_STYLES.contains(&style.as_str()) {
                    return Err(format!("\"style\" must be one of {}", BLUR_STYLES.join(", ")));
                }
                style.into()
            }
            "size" => ranged(op, key, 8.0, 400.0)?,
            "width" => ranged(op, key, 1.0, 40.0)?,
            "radius" => ranged(op, key, 0.0, 400.0)?,
            "spotlight" | "amount" => ranged(op, key, 0.0, 1.0)?,
            "fadeIn" | "fadeOut" => ranged(op, key, 0.0, 5.0)?,
            "number" => {
                let value = number(op, key)?;
                if value.fract() != 0.0 || !(0.0..=999.0).contains(&value) {
                    return Err("\"number\" must be a whole number from 0 to 999".into());
                }
                Value::from(value as i32)
            }
            "rect" => rect(op, partial)?,
            "tail" => {
                patch.insert("from", point(op, key)?);
                continue;
            }
            "head" => {
                patch.insert("to", point(op, key)?);
                continue;
            }
            _ => continue,
        };
        patch.insert(key, value);
    }
    Ok(patch)
}

/// The anchor an overlay of `kind` gets: the op's, checked against what the kind allows, or
/// `current`.
fn anchor(op: &Object, kind: &str, current: &str) -> Result<String, String> {
    let Some(anchor) = op.value("anchor").as_str() else {
        if op.value("anchor").is_undefined() {
            return Ok(current.to_owned());
        }
        return Err("\"anchor\" must be \"screen\" or \"frame\"".into());
    };
    if !OVERLAY_ANCHORS.contains(&anchor) {
        return Err("\"anchor\" must be \"screen\" or \"frame\"".into());
    }
    match (kind, anchor) {
        ("blur", "frame") => Err("blur is always on the screen (anchor \"screen\")".into()),
        ("card", "screen") => Err("a card is always on the frame (anchor \"frame\")".into()),
        _ => Ok(anchor.to_owned()),
    }
}

/// The recording span output `from`..`to` shows: from the earliest to the latest recording
/// time any recording clip plays in it (cut footage between is inside it). `None` when only
/// inserted videos play there.
fn shown_source_extent(document: &Document, from: f64, to: f64) -> Option<(f64, f64)> {
    let mut extent: Option<(f64, f64)> = None;
    let mut cursor = 0.0;
    for clip in &document.project().clips {
        let length = (clip.out - clip.in_) / clip.speed;
        let (a, b) = (cpp::max(from, cursor), cpp::min(to, cursor + length));
        if !clip.is_media() && b > a {
            let (start, end) = (clip.in_ + (a - cursor) * clip.speed, clip.in_ + (b - cursor) * clip.speed);
            extent = Some(extent.map_or((start, end), |(s, e)| (s.min(start), e.max(end))));
        }
        cursor += length;
    }
    extent
}

/// `from`/`to` on the op's timeline as times in the item's own base: source seconds for a
/// `screen` item, output seconds for a `frame` one.
fn item_times(document: &Document, op: &Object, from: f64, to: f64, screen: bool) -> Result<(f64, f64), String> {
    if to - from < MIN_OVERLAY_LENGTH - 1e-9 {
        return Err(format!("\"to\" must be at least {MIN_OVERLAY_LENGTH} s after \"from\""));
    }
    if source_timeline(op)? {
        if from < 0.0 || to > document.source_duration() + 1e-6 {
            return Err(format!("source {from}–{to} is outside the recording (0 to {})", ms(document.source_duration())));
        }
        if screen {
            return Ok((from, to));
        }
        let spans = source_span_outputs(&document.project().clips, from, to);
        let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
            return Err(format!("source {from}–{to} is not in the edited video (it was cut)"));
        };
        return Ok((first.0, last.1));
    }
    if from < 0.0 || to > document.duration() + 1e-6 {
        return Err(format!("{from}–{to} is outside the video (0 to {})", ms(document.duration())));
    }
    if !screen {
        return Ok((from, to));
    }
    shown_source_extent(document, from, to)
        .filter(|(start, end)| end - start > 1e-6)
        .ok_or_else(|| format!("only inserted videos play at {from}–{to}; a screen overlay needs the recording (use \"anchor\": \"frame\")"))
}

/// Where an overlay plays and is, for results and `describe`.
pub fn overlay_summary(document: &Document, overlay: &Overlay) -> Object {
    let clips = &document.project().clips;
    let mut object = Object::from_iter([
        ("id", Value::from(overlay.id.as_str())),
        ("kind", overlay.kind.as_str().into()),
        ("anchor", overlay.anchor.as_str().into()),
    ]);
    let span = |start: f64, end: f64| Value::from(Object::from_iter([("start", Value::from(ms(start))), ("end", ms(end).into())]));
    if overlay.is_screen_anchored() {
        object.insert("source", span(overlay.start, overlay.end));
        let outputs = source_span_outputs(clips, overlay.start, overlay.end);
        object.insert("output", Value::array(outputs.iter().map(|&(a, b)| span(a, b)).collect()));
    } else {
        object.insert("output", Value::array(vec![span(overlay.start, overlay.end)]));
    }
    // The rest as project.json has it, the arrow's ends named as the operations name them.
    for (key, value) in overlay.to_json().iter() {
        let key = match key {
            "id" | "kind" | "anchor" | "start" | "end" => continue,
            "from" if overlay.kind == "arrow" => "tail",
            "to" if overlay.kind == "arrow" => "head",
            other => other,
        };
        object.insert(key, value.clone());
    }
    object
}

fn add_overlay(document: &mut Document, op: &Object, kind: &str) -> OpResult {
    if !OVERLAY_KINDS.contains(&kind) {
        return Err(format!("\"kind\" must be one of {}", OVERLAY_KINDS.join(", ")));
    }
    check_fields(op, kind)?;
    if kind == "blur" && op.value("rect").is_undefined() {
        return Err("a blur needs a \"rect\": the region to hide, 0–1 of the recording's full frame".into());
    }
    let defaults = Overlay::new_of_kind(kind).expect("a known kind");
    let anchor = anchor(op, kind, &defaults.anchor)?;
    let screen = anchor == "screen";
    let (start, end) = item_times(document, op, number(op, "from")?, number(op, "to")?, screen)?;
    let mut props = look_patch(op, kind, false)?;
    props.insert("anchor", anchor.as_str());
    let id = document.add_overlay(kind, start, end, None, &props);
    if id.is_empty() {
        return Err(format!("could not add the {kind} overlay"));
    }
    let overlay = document.overlay(&id).cloned().expect("just added");
    let mut result = overlay_summary(document, &overlay);
    if screen && source_span_outputs(&document.project().clips, overlay.start, overlay.end).is_empty() {
        result.insert("warning", "it covers no footage the video still shows");
    }
    Ok(result)
}

fn edit_overlay(document: &mut Document, op: &Object) -> OpResult {
    let id = text(op, "id")?;
    let current = document.overlay(&id).cloned().ok_or_else(|| format!("no overlay with id {id:?}"))?;
    if let Some(kind) = op.value("kind").as_str() {
        if kind != current.kind {
            return Err(format!("an overlay's kind can't change ({id} is a {}); remove it and add a {kind}", current.kind));
        }
    }
    check_fields(op, &current.kind)?;
    let new_anchor = anchor(op, &current.kind, &current.anchor)?;
    let mut patch = look_patch(op, &current.kind, true)?;
    if new_anchor != current.anchor {
        patch.insert("anchor", new_anchor.as_str());
    }
    let (from, to) = (optional_number(op, "from")?, optional_number(op, "to")?);
    if from.is_some() || to.is_some() {
        // The times not given stay where they are, read on the op's timeline.
        let (now_from, now_to) = if source_timeline(op)? {
            if current.is_screen_anchored() {
                (current.start, current.end)
            } else {
                (document.output_to_source(current.start), document.output_to_source((current.end - 1e-6).max(0.0)))
            }
        } else {
            let range = document.overlay_output_range(&current);
            if range.0 < 0.0 && (from.is_none() || to.is_none()) {
                return Err(format!("{id} no longer shows (its footage was cut): give both \"from\" and \"to\""));
            }
            range
        };
        let (start, end) = item_times(document, op, from.unwrap_or(now_from), to.unwrap_or(now_to), new_anchor == "screen")?;
        patch.insert("start", start);
        patch.insert("end", end);
    }
    if patch.is_empty() {
        return Err("nothing to change: give any of from, to, anchor, or the kind's fields".into());
    }
    let changed = document.update_overlay(&id, &patch, false);
    let overlay = document.overlay(&id).cloned().expect("still there");
    let mut result = overlay_summary(document, &overlay);
    if !changed {
        result.insert("warning", "no change");
    }
    Ok(result)
}

/// The operations of this module; `None` for any other name.
pub(super) fn run(document: &mut Document, op: &Object, warnings: &mut Vec<String>) -> Option<OpResult> {
    let name = op.value("op").to_string_or("");
    let result = match name.as_str() {
        "add-overlay" => add_overlay(document, op, &text(op, "kind").unwrap_or_default()),
        "blur" => {
            for key in op.keys() {
                if !matches!(key, "op" | "from" | "to" | "rect" | "style" | "amount" | "timeline") {
                    return Some(Err(format!("blur takes from, to, rect, style and amount, not \"{key}\"")));
                }
            }
            add_overlay(document, op, "blur")
        }
        "edit-overlay" => edit_overlay(document, op),
        "remove-overlay" => text(op, "id").and_then(|id| {
            if document.remove_overlay(&id) { ok() } else { Err(format!("no overlay with id {id:?}")) }
        }),
        "add-marker" => add_marker(document, op),
        "rename-marker" => (|| {
            let id = text(op, "id")?;
            if document.marker_index(&id) < 0 {
                return Err(format!("no marker with id {id:?}"));
            }
            let label = text(op, "label")?;
            if !document.rename_marker(&id, &label) {
                warnings.push(format!("rename-marker {id}: it already has that label"));
            }
            ok()
        })(),
        "remove-marker" => text(op, "id").and_then(|id| {
            if document.remove_marker(&id) { ok() } else { Err(format!("no marker with id {id:?}")) }
        }),
        "transition" => (|| {
            let style = text(op, "style")?;
            if !transitions::STYLES.contains(&style.as_str()) {
                return Err(format!("\"style\" must be one of {}", transitions::STYLES.join(", ")));
            }
            if !document.set_transition(&style) {
                warnings.push(format!("transition: the project already uses {style}"));
            }
            with("cuts", document.transition_cuts().len() as i64)
        })(),
        "cut-transition" => (|| {
            let id = clip_id(document, op)?;
            let style = match op.value("style") {
                Value::Null => None,
                Value::String(style) if transitions::STYLES.contains(&style.as_str()) => Some(style.clone()),
                _ => return Err(format!("\"style\" must be one of {}, or null to follow the project", transitions::STYLES.join(", "))),
            };
            if !document.clip_starts_at_cut(&id) {
                return Err(format!("clip {id} doesn't start at a cut (the first clip, a split, or next to an inserted video)"));
            }
            if !document.set_cut_transition(&id, style.as_deref()) {
                warnings.push(format!("cut-transition {id}: no change"));
            }
            ok()
        })(),
        "speed-up-quiet" => speed_up_quiet(document, op),
        "reframe" => (|| {
            let mode = text(op, "mode")?;
            if !matches!(mode.as_str(), "follow" | "off") {
                return Err("\"mode\" must be \"follow\" or \"off\"".into());
            }
            if !document.set_reframe(&mode) {
                warnings.push(format!("reframe: already {mode}"));
            }
            let active = crate::reframe::active(document.project(), document.source_width(), document.source_height());
            if mode == "follow" && !active {
                warnings.push(format!(
                    "reframe: the canvas has the recording's shape (aspect {}), so there is nothing to reframe; \
                     set \"aspect\" to 9:16 or 1:1, or export with --preset shorts",
                    document.project().aspect
                ));
            }
            with("active", active)
        })(),
        "audio-cleanup" => audio_cleanup(document, op, warnings),
        _ => return None,
    };
    Some(result)
}

fn add_marker(document: &mut Document, op: &Object) -> OpResult {
    let label = if op.value("label").is_undefined() { String::new() } else { text(op, "label")? };
    let id = if source_timeline(op)? {
        let at = number(op, "at")?;
        if at < 0.0 || at > document.source_duration() + 1e-6 {
            return Err(format!("source time {at} is outside the recording (0 to {})", ms(document.source_duration())));
        }
        document.add_marker(at, &label)
    } else {
        let at = output_time(document, op, "at")?;
        let id = document.add_marker_at_output(at, &label);
        if id.is_empty() {
            return Err(format!("an inserted video plays at {at}; markers mark moments of the recording"));
        }
        id
    };
    let marker = &document.markers()[document.marker_index(&id) as usize];
    Ok(Object::from_iter([
        ("id", Value::from(id.as_str())),
        ("source", ms(marker.time).into()),
        ("output", Value::array(source_time_outputs(&document.project().clips, marker.time).into_iter().map(|t| Value::from(ms(t))).collect())),
    ]))
}

fn speed_up_quiet(document: &mut Document, op: &Object) -> OpResult {
    if !document.recording().has_audio {
        return Err("the recording has no sound: quiet parts are found from where it speaks".into());
    }
    let defaults = SpeedUpSettings::default();
    let target = optional_number(op, "target")?.unwrap_or(defaults.target);
    let min = optional_number(op, "min")?.unwrap_or(defaults.min_length);
    if !(0.25..=60.0).contains(&target) {
        return Err("\"target\" (seconds each quiet part should play in) must be from 0.25 to 60".into());
    }
    if !(0.5..=600.0).contains(&min) {
        return Err("\"min\" (the shortest quiet part to speed up, seconds) must be from 0.5 to 600".into());
    }
    super::apply::measure_speech_and_screen(document, true)?;
    let settings = SpeedUpSettings { min_length: min, target, ..defaults };
    let parts = document.quiet_parts(&settings);
    let before = document.duration();
    let changed = if parts.is_empty() { 0 } else { document.speed_up_source_ranges(&parts) };
    Ok(Object::from_iter([
        ("parts", Value::from(changed as i64)),
        ("saved_seconds", ms(before - document.duration()).into()),
        ("output_duration", ms(document.duration()).into()),
    ]))
}

fn audio_cleanup(document: &mut Document, op: &Object, warnings: &mut Vec<String>) -> OpResult {
    let mut cleanup = document.project().audio.cleanup.clone();
    let mut any = false;
    match op.value("denoise") {
        Value::Undefined => {}
        Value::Bool(denoise) => {
            cleanup.denoise = *denoise;
            any = true;
        }
        _ => return Err("\"denoise\" must be true or false".into()),
    }
    if let Some(strength) = optional_number(op, "strength")? {
        if !(0.1..=1.0).contains(&strength) {
            return Err("\"strength\" must be from 0.1 to 1".into());
        }
        cleanup.strength = strength;
        any = true;
    }
    match op.value("loudness") {
        Value::Undefined => {}
        value => {
            let loudness = match value {
                Value::Bool(false) => "off".to_owned(),
                value if value.is_double() => format!("{}", value.to_double(0.0)),
                value => value.to_string_or(""),
            };
            if !AudioCleanup::LOUDNESS_TARGETS.contains(&loudness.as_str()) {
                return Err("\"loudness\" must be \"off\", -16 (podcasts, YouTube) or -14 (social video)".into());
            }
            cleanup.loudness = loudness;
            any = true;
        }
    }
    if !any {
        return Err("give any of \"denoise\", \"strength\", \"loudness\"".into());
    }
    if cleanup.denoise && !document.denoise_available() {
        warnings.push("audio-cleanup: the recording has no microphone sound, so denoise does nothing".into());
    }
    document.set_audio_cleanup(cleanup);
    let cleanup = &document.project().audio.cleanup;
    Ok(Object::from_iter([
        ("denoise", Value::from(cleanup.denoise)),
        ("strength", cleanup.strength.into()),
        ("loudness", cleanup.loudness.as_str().into()),
    ]))
}
