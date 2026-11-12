//! `omareel apply <bundle> <ops.json|->`: edit operations run in order through the editor's
//! [`Document`], all or nothing. Each operation is a JSON object with an `op` name; the list
//! may be a bare array, an object with an `ops` array, or a single operation.
//!
//! Times are output seconds as `omareel describe` reports them, read against the timeline as it
//! is when that operation runs (a cut moves everything after it). An operation with
//! `"timeline": "source"` takes recording seconds instead, which no edit moves.
//!
//! | op | fields |
//! | --- | --- |
//! | `split` | `at` |
//! | `delete` | `from`, `to` |
//! | `trim` | `clip`, `in` and/or `out` (source seconds, or the inserted video's own) |
//! | `remove-clip` | `clip` |
//! | `merge` | `clip`, `with`: `next` or `previous` |
//! | `move-clip` | `clip` (an inserted video), `direction`: `earlier` or `later` |
//! | `reset-trims` | `clip` |
//! | `speed` | `clip` or `at`, `speed` |
//! | `layout` | `clip` or `at`, `layout` |
//! | `fit` | `clip` or `at`, `fit` |
//! | `insert` | `file`, `at` (default: the end) |
//! | `zoom` | `from`, `to`, optional `level` (1–4, default 2), `target` (`"auto"` or `{x, y}` 0–1) |
//! | `edit-zoom` | `id`, any of `from`, `to`, `level`, `target` |
//! | `remove-zoom` | `id` |
//! | `clear-zooms` | |
//! | `regenerate-zooms` | |
//! | `set` | `path` (dotted, as in describe's `settings`), `value` |

use super::{ms, source_span_outputs, source_time_outputs};
use crate::compat::qdir;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::editor::{Document, Effect, ImportedInsert, ZoomTarget};
use crate::project::{allowed_clip_speeds, Project};

/// One operation's failure, before the operation's number is added.
type OpResult = Result<Object, String>;

/// The operations in `json`: an array, `{"ops": [...]}`, or one operation.
pub fn parse_ops(json: &[u8]) -> Result<Vec<Object>, String> {
    let value = qjson::from_json(json).map_err(|error| format!("operations are not valid JSON: {}", error.error_string()))?;
    let list = match &value {
        Value::Array(items) => items.clone(),
        Value::Object(object) if object.contains("ops") => object.value("ops").to_array(),
        Value::Object(_) => vec![value.clone()],
        _ => return Err("operations must be a JSON array or object".into()),
    };
    list.into_iter()
        .enumerate()
        .map(|(index, item)| match item {
            Value::Object(object) if object.value("op").is_string() => Ok(object),
            _ => Err(format!("operation {} is not an object with an \"op\" name", index + 1)),
        })
        .collect()
}

fn number(op: &Object, key: &str) -> Result<f64, String> {
    match op.value(key) {
        value if value.is_double() => {
            let n = value.to_double(f64::NAN);
            if n.is_finite() { Ok(n) } else { Err(format!("\"{key}\" must be a finite number")) }
        }
        Value::Undefined => Err(format!("\"{key}\" is required")),
        _ => Err(format!("\"{key}\" must be a number")),
    }
}

fn optional_number(op: &Object, key: &str) -> Result<Option<f64>, String> {
    if op.value(key).is_undefined() { Ok(None) } else { number(op, key).map(Some) }
}

fn text(op: &Object, key: &str) -> Result<String, String> {
    op.value(key).as_str().map(str::to_owned).ok_or_else(|| format!("\"{key}\" must be a string"))
}

fn source_timeline(op: &Object) -> Result<bool, String> {
    match op.value("timeline").as_str() {
        None if op.value("timeline").is_undefined() => Ok(false),
        Some("output") => Ok(false),
        Some("source") => Ok(true),
        _ => Err("\"timeline\" must be \"output\" or \"source\"".into()),
    }
}

/// An output time from `key`, converting a source time (the first place it plays).
fn output_time(document: &Document, op: &Object, key: &str) -> Result<f64, String> {
    let t = number(op, key)?;
    if !source_timeline(op)? {
        if t < 0.0 || t > document.duration() + 1e-6 {
            return Err(format!("\"{key}\" {t} is outside the video (0 to {})", ms(document.duration())));
        }
        return Ok(t);
    }
    source_time_outputs(&document.project().clips, t)
        .first()
        .copied()
        .ok_or_else(|| format!("source time {t} is not in the edited video (it was cut)"))
}

/// The output spans `from`–`to` names: one, or for source times every place they play.
fn output_spans(document: &Document, op: &Object) -> Result<Vec<(f64, f64)>, String> {
    let from = number(op, "from")?;
    let to = number(op, "to")?;
    if to <= from {
        return Err("\"to\" must be after \"from\"".into());
    }
    if source_timeline(op)? {
        let spans = source_span_outputs(&document.project().clips, from, to);
        if spans.is_empty() {
            return Err(format!("source {from}–{to} is not in the edited video"));
        }
        return Ok(spans);
    }
    if from < 0.0 || to > document.duration() + 1e-6 {
        return Err(format!("{from}–{to} is outside the video (0 to {})", ms(document.duration())));
    }
    Ok(vec![(from, to)])
}

/// The clip an operation names with `clip` (an id) or `at` (the clip playing then).
fn clip_id(document: &Document, op: &Object) -> Result<String, String> {
    if let Some(id) = op.value("clip").as_str() {
        if document.clip_index(id) < 0 {
            return Err(format!("no clip with id {id:?}"));
        }
        return Ok(id.to_owned());
    }
    if !op.value("at").is_undefined() {
        let at = output_time(document, op, "at")?;
        let (index, _) = document.clip_for_output(at);
        if let Some(clip) = usize::try_from(index).ok().and_then(|i| document.project().clips.get(i)) {
            return Ok(clip.id.clone());
        }
    }
    Err("name the clip with \"clip\" (an id) or \"at\" (a time)".into())
}

fn zoom_target(value: &Value) -> Result<ZoomTarget, String> {
    if let Some(text) = value.as_str() {
        if ZoomTarget::is_auto(text) {
            return Ok(ZoomTarget::Auto);
        }
    }
    if let Some(point) = value.as_object() {
        let x = point.value("x");
        let y = point.value("y");
        if x.is_double() && y.is_double() {
            return Ok(ZoomTarget::Point(x.to_double(0.5), y.to_double(0.5)));
        }
    }
    Err("\"target\" must be \"auto\" or {\"x\": 0–1, \"y\": 0–1}".into())
}

/// The recording time a zoom ending at output `to` ends at: that of the moment just before
/// `to`, so a zoom ending where a clip ends stays in that clip.
fn zoom_source_end(document: &Document, to: f64) -> f64 {
    document.output_to_source((to - 1e-6).max(0.0)) + 1e-6
}

fn zoom_level(op: &Object) -> Result<Option<f64>, String> {
    match optional_number(op, "level")? {
        Some(level) if !(1.0..=4.0).contains(&level) => Err("\"level\" must be from 1 to 4".into()),
        level => Ok(level),
    }
}

fn ok() -> OpResult {
    Ok(Object::new())
}

fn with(key: &str, value: impl Into<Value>) -> OpResult {
    Ok(Object::from_iter([(key, value.into())]))
}

/// What a run of operations keeps beside the document.
#[derive(Default)]
struct Run {
    /// Check only: nothing is written, not even an inserted video's copy.
    dry_run: bool,
    warnings: Vec<String>,
    /// Videos copied into media/ by this run, removed again if it fails.
    copied: Vec<String>,
}

/// The edits of [`Document`] the operations use, refusing what the editor would refuse.
fn run_op(document: &mut Document, op: &Object, run: &mut Run) -> OpResult {
    let warnings = &mut run.warnings;
    let name = op.value("op").to_string_or("");
    match name.as_str() {
        "split" => {
            let at = output_time(document, op, "at")?;
            document.set_output_position(at);
            if !document.split_at_playhead() {
                return Err("can't split within 0.1 s of a clip's start or end".into());
            }
            with("clip", document.selected_clip_id())
        }
        "delete" => {
            let mut spans = output_spans(document, op)?;
            // From the last, so the earlier spans stay where they are.
            spans.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (from, to) in spans {
                if !document.delete_output_range(from, to) {
                    return Err("nothing to cut there, or the cut would remove all of the recording".into());
                }
            }
            with("output_duration", ms(document.duration()))
        }
        "trim" => {
            let id = clip_id(document, op)?;
            let clip = document.project().clips[document.clip_index(&id) as usize].clone();
            let new_in = optional_number(op, "in")?.unwrap_or(clip.in_);
            let new_out = optional_number(op, "out")?.unwrap_or(clip.out);
            if !document.trim_clip(&id, new_in, new_out) {
                return Err("a clip must stay at least 0.1 s long".into());
            }
            let clip = &document.project().clips[document.clip_index(&id) as usize];
            if (clip.in_ - new_in).abs() > 1e-6 || (clip.out - new_out).abs() > 1e-6 {
                warnings.push(format!("trim {id}: limited to {}–{} by its neighbours", ms(clip.in_), ms(clip.out)));
            }
            ok()
        }
        "remove-clip" => {
            let id = clip_id(document, op)?;
            if !document.remove_clip(&id) {
                return Err("the last clip of the recording can't be removed".into());
            }
            ok()
        }
        "merge" => {
            let id = clip_id(document, op)?;
            let direction = match op.value("with").as_str() {
                Some("next") | None => 1,
                Some("previous") => -1,
                _ => return Err("\"with\" must be \"next\" or \"previous\"".into()),
            };
            if !document.merge_clip(&id, direction) {
                return Err("only neighbouring recording clips can be merged".into());
            }
            with("clip", document.selected_clip_id())
        }
        "move-clip" => {
            let id = clip_id(document, op)?;
            let direction = match text(op, "direction")?.as_str() {
                "earlier" => -1,
                "later" => 1,
                _ => return Err("\"direction\" must be \"earlier\" or \"later\"".into()),
            };
            if !document.reorder_clip(&id, direction) {
                return Err("only inserted videos move, and not past the ends".into());
            }
            ok()
        }
        "reset-trims" => {
            let id = clip_id(document, op)?;
            if !document.reset_clip_trims(&id) {
                return Err("the clip has no room to grow".into());
            }
            ok()
        }
        "speed" => {
            let id = clip_id(document, op)?;
            let speed = number(op, "speed")?;
            if !document.set_clip_speed(&id, speed) {
                let allowed: Vec<String> = allowed_clip_speeds().iter().map(|s| s.to_string()).collect();
                return Err(format!("speed must be one of {}", allowed.join(", ")));
            }
            with("output_duration", ms(document.duration()))
        }
        "layout" => {
            let id = clip_id(document, op)?;
            if !document.set_clip_layout(&id, &text(op, "layout")?) {
                return Err("layout must be default, camera or screen, on a recording clip".into());
            }
            ok()
        }
        "fit" => {
            let id = clip_id(document, op)?;
            if !document.set_clip_fit(&id, &text(op, "fit")?) {
                return Err("fit must be fit, fill or framed, on an inserted video".into());
            }
            ok()
        }
        "insert" => {
            let file = qdir::absolute_path(&text(op, "file")?);
            let at = if op.value("at").is_undefined() { document.duration() } else { output_time(document, op, "at")? };
            document.set_output_position(at);
            if !document.can_insert_media(&file) {
                return Err("an import is already running".into());
            }
            let job = document.import_job(&file);
            let imported = if run.dry_run {
                // What the import would find, without copying the video into the bundle.
                match (job.probe_video)(&file) {
                    Ok(info) if info.is_valid() => ImportedInsert {
                        path: format!("media/{}", qdir::FileInfo::new(file.as_str()).file_name()),
                        info,
                        error: String::new(),
                    },
                    Ok(_) => return Err(format!("{file} has no video to insert")),
                    Err(error) => return Err(error),
                }
            } else {
                let media = qdir::file_path(document.bundle_path(), "media");
                let before: Vec<String> = std::fs::read_dir(&media)
                    .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                    .unwrap_or_default();
                let imported = job.run();
                if let Some(name) = imported.path.strip_prefix("media/") {
                    if !before.iter().any(|existing| existing == name) {
                        run.copied.push(qdir::file_path(&media, name));
                    }
                }
                imported
            };
            document.start_import(&file);
            document.finish_import(imported);
            if !document.import_error().is_empty() {
                return Err(document.import_error().to_owned());
            }
            with("clip", document.selected_clip_id())
        }
        "zoom" => {
            let spans = output_spans(document, op)?;
            if spans.len() != 1 {
                return Err("that source span plays in several places; zoom each with output times".into());
            }
            let (from, to) = spans[0];
            let level = zoom_level(op)?.unwrap_or(2.0);
            let target = if op.value("target").is_undefined() { ZoomTarget::Auto } else { zoom_target(op.value("target"))? };
            let (start, end) = (document.output_to_source(from), zoom_source_end(document, to));
            if end - start < 1.0 - 1e-6 {
                return Err("a zoom must cover at least a second of the recording".into());
            }
            let overlapping: Vec<&str> = document
                .project()
                .zooms
                .iter()
                .filter(|zoom| zoom.start < end && start < zoom.end)
                .map(|zoom| zoom.id.as_str())
                .collect();
            if !overlapping.is_empty() {
                return Err(format!("zooms can't overlap; edit or remove {} first", overlapping.join(", ")));
            }
            let id = document.add_zoom_at(from, (to - from).max(1.0));
            if id.is_empty() {
                return Err("no room for a zoom there: zooms can't overlap, last under a second, or sit on an inserted video".into());
            }
            document.resize_zoom(&id, start, end.max(start + 1.0));
            document.set_zoom_level(&id, level, false);
            document.set_zoom_target(&id, target);
            with("id", id)
        }
        "edit-zoom" => {
            let id = text(op, "id")?;
            let index = document.zoom_index(&id);
            if index < 0 {
                return Err(format!("no zoom with id {id:?}"));
            }
            if !op.value("from").is_undefined() || !op.value("to").is_undefined() {
                let zoom = document.project().zooms[index as usize].clone();
                let from = optional_number(op, "from")?;
                let to = optional_number(op, "to")?;
                let (mut start, mut end) = (zoom.start, zoom.end);
                if source_timeline(op)? {
                    start = from.unwrap_or(start);
                    end = to.unwrap_or(end);
                } else {
                    if let Some(from) = from {
                        start = document.output_to_source(from);
                    }
                    if let Some(to) = to {
                        end = zoom_source_end(document, to);
                    }
                }
                if !document.resize_zoom(&id, start, end) {
                    return Err("a zoom lasts at least a second and can't overlap its neighbours".into());
                }
            }
            if let Some(level) = zoom_level(op)? {
                document.set_zoom_level(&id, level, false);
            }
            if !op.value("target").is_undefined() {
                document.set_zoom_target(&id, zoom_target(op.value("target"))?);
            }
            ok()
        }
        "remove-zoom" => {
            let id = text(op, "id")?;
            if !document.remove_zoom(&id) {
                return Err(format!("no zoom with id {id:?}"));
            }
            ok()
        }
        "clear-zooms" => {
            let ids: Vec<String> = document.project().zooms.iter().map(|zoom| zoom.id.clone()).collect();
            for id in &ids {
                document.remove_zoom(id);
            }
            with("removed", ids.len() as i64)
        }
        "regenerate-zooms" => {
            document.regenerate_zooms();
            with("zooms", document.project().zooms.len() as i64)
        }
        "set" => {
            let path = text(op, "path")?;
            if path.split('.').next().is_some_and(|root| matches!(root, "clips" | "zooms")) {
                return Err("clips and zooms change through their own operations".into());
            }
            let value = op.value("value").clone();
            if value.is_undefined() {
                return Err("\"value\" is required".into());
            }
            let before = document.project().to_json();
            document.set_project_value(&path, value, false);
            if document.project().to_json() == before {
                warnings.push(format!("set {path}: no change (the same value, or a setting omareel doesn't have)"));
            }
            ok()
        }
        "" => Err("missing \"op\"".into()),
        other => Err(format!("unknown op {other:?}")),
    }
}

/// Runs the operations in order. The error names the operation that failed; the document may
/// then hold some of the edits, so the caller must not save it. Videos the run copied into
/// the bundle are removed again on failure; a `dry_run` copies none.
pub fn apply_ops(document: &mut Document, ops: &[Object], dry_run: bool) -> Result<(Vec<Value>, Vec<String>), String> {
    let mut run = Run { dry_run, ..Run::default() };
    let mut results = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        let name = op.value("op").to_string_or("");
        let outcome = run_op(document, op, &mut run);
        document.take_effects();
        match outcome {
            Ok(mut result) => {
                result.insert("op", name.as_str());
                results.push(result.into());
            }
            Err(error) => {
                for copy in &run.copied {
                    let _ = std::fs::remove_file(copy);
                }
                return Err(format!("operation {} ({name}): {error}", index + 1));
            }
        }
    }
    Ok((results, run.warnings))
}

/// `omareel apply`: the JSON report, and whether it succeeded. Nothing is saved on failure or
/// with `dry_run`; `describe_after` adds the timeline as `omareel describe` would print it.
pub fn run(bundle: &str, ops_json: &[u8], dry_run: bool, describe_after: bool) -> (String, bool) {
    run_on(|| super::open_document(bundle), ops_json, dry_run, describe_after)
}

/// [`run`] on the document `open` opens, once the operations parse.
pub fn run_on(
    open: impl FnOnce() -> Result<Document, String>,
    ops_json: &[u8],
    dry_run: bool,
    describe_after: bool,
) -> (String, bool) {
    let failure = |error: String| {
        (Object::from_iter([("ok", Value::from(false)), ("error", error.into())]).to_json(Format::Indented), false)
    };
    let ops = match parse_ops(ops_json) {
        Ok(ops) => ops,
        Err(error) => return failure(error),
    };
    let mut document = match open() {
        Ok(document) => document,
        Err(error) => return failure(error),
    };
    let (results, warnings) = match apply_ops(&mut document, &ops, dry_run) {
        Ok(done) => done,
        Err(error) => return failure(error),
    };
    if !dry_run {
        let saved = document.save_now();
        let effects = document.take_effects();
        if !saved {
            let error = effects
                .into_iter()
                .find_map(|effect| if let Effect::ErrorOccurred(message) = effect { Some(message) } else { None })
                .unwrap_or_else(|| "could not save project.json".into());
            return failure(error);
        }
    }
    let project: &Project = document.project();
    let mut report = Object::from_iter([
        ("ok", Value::from(true)),
        ("saved", (!dry_run).into()),
        ("results", results.into()),
        ("warnings", Value::array(warnings.into_iter().map(Value::from).collect())),
        ("output_duration", ms(document.duration()).into()),
        ("clips", (project.clips.len() as i64).into()),
        ("zooms", (project.zooms.len() as i64).into()),
    ]);
    if describe_after {
        report.insert("describe", super::describe::describe(&document));
    }
    (report.to_json(Format::Indented), true)
}
