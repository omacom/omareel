//! `omareel describe <bundle>`: the edited timeline as JSON, in the terms `omareel apply` takes.

use super::{ms, source_span_outputs, source_time_outputs};
use crate::compat::geometry::PointF;
use crate::compat::qjson::{Format, Object, Value};
use crate::compat::qround::q_round;
use crate::editor::{aspect_ratio, Document};
use crate::input_log::{InputEvent, InputKind};
use crate::keystroke_track::KeystrokeTrack;
use crate::project::{allowed_clip_fits, allowed_clip_layouts, allowed_clip_speeds, Keystrokes};

/// Input pauses at least this long are listed as idle stretches.
pub const IDLE_SECONDS: f64 = 2.0;
/// At most this many clicks are listed; the count says how many there were.
const CLICK_LIMIT: usize = 400;

fn span(start: f64, end: f64) -> Value {
    Object::from_iter([("start", Value::from(ms(start))), ("end", Value::from(ms(end)))]).into()
}

fn spans(list: &[(f64, f64)]) -> Value {
    Value::array(list.iter().map(|&(start, end)| span(start, end)).collect())
}

fn times(list: &[f64]) -> Value {
    Value::array(list.iter().map(|&t| Value::from(ms(t))).collect())
}

fn point(position: PointF) -> Value {
    Object::from_iter([("x", Value::from(ms(position.x))), ("y", Value::from(ms(position.y)))]).into()
}

/// The stretches of the recording with no input at all (no pointer movement, clicks, scrolls or
/// shortcut keys) lasting at least [`IDLE_SECONDS`].
pub fn idle_spans(events: &[InputEvent], duration: f64) -> Vec<(f64, f64)> {
    let mut result = Vec::new();
    let mut last = 0.0;
    for event in events {
        if event.time - last >= IDLE_SECONDS {
            result.push((last, event.time));
        }
        last = last.max(event.time);
    }
    if duration - last >= IDLE_SECONDS {
        result.push((last, duration));
    }
    result
}

/// The summary of an open document.
pub fn describe(document: &Document) -> Object {
    let recording = document.recording();
    let project = document.project();
    let clips = &project.clips;
    let width = recording.source_width;
    let height = recording.source_height;
    let normalized = |position: PointF| {
        PointF::new(position.x / f64::from(width.max(1)), position.y / f64::from(height.max(1)))
    };

    let mut source = Object::from_iter([
        ("duration", Value::from(ms(recording.source_duration))),
        ("width", width.into()),
        ("height", height.into()),
        ("fps", ms(recording.fps).into()),
        ("desktop_audio", recording.has_desktop_audio.into()),
        ("microphone_audio", recording.has_microphone_audio.into()),
    ]);
    if let Some(camera) = &recording.camera {
        source.insert(
            "camera",
            Object::from_iter([
                ("width", Value::from(camera.width)),
                ("height", camera.height.into()),
                ("duration", ms(camera.duration).into()),
            ]),
        );
    }

    let aspect = aspect_ratio(project, width, height);
    let (export_width, export_height) = crate::export::plan::padded_even_size(
        q_round(f64::from(project.export_settings.height) * aspect),
        project.export_settings.height,
    );
    let output = Object::from_iter([
        ("duration", Value::from(ms(document.duration()))),
        ("aspect", project.aspect.as_str().into()),
        ("export_width", export_width.into()),
        ("export_height", export_height.into()),
        ("export_fps", project.export_settings.fps.into()),
    ]);

    let mut cursor = 0.0;
    let clip_list: Vec<Value> = clips
        .iter()
        .map(|clip| {
            let length = (clip.out - clip.in_) / clip.speed;
            let mut object = Object::from_iter([
                ("id", Value::from(clip.id.as_str())),
                ("kind", if clip.is_media() { "video" } else { "recording" }.into()),
                ("output", span(cursor, cursor + length)),
                ("speed", clip.speed.into()),
            ]);
            if clip.is_media() {
                object.insert("media", clip.media.as_str());
                object.insert("media_range", span(clip.in_, clip.out));
                object.insert("fit", clip.fit.as_str());
            } else {
                object.insert("source", span(clip.in_, clip.out));
                object.insert("layout", clip.layout.as_str());
            }
            cursor += length;
            object.into()
        })
        .collect();

    let zooms: Vec<Value> = project
        .zooms
        .iter()
        .map(|zoom| {
            Object::from_iter([
                ("id", Value::from(zoom.id.as_str())),
                ("source", span(zoom.start, zoom.end)),
                ("output", spans(&source_span_outputs(clips, zoom.start, zoom.end))),
                ("level", zoom.level.into()),
                ("target", if zoom.automatic_target { Value::from("auto") } else { point(zoom.target) }),
            ])
            .into()
        })
        .collect();

    let events = document.input().events();
    let clicks: Vec<&InputEvent> = events.iter().filter(|event| event.kind == InputKind::ButtonDown).collect();
    let click_list: Vec<Value> = clicks
        .iter()
        .take(CLICK_LIMIT)
        .map(|event| {
            Object::from_iter([
                ("source", Value::from(ms(event.time))),
                ("output", times(&source_time_outputs(clips, event.time))),
                ("button", event.button.as_str().into()),
                ("position", point(normalized(event.position))),
            ])
            .into()
        })
        .collect();
    let shortcut_settings = Keystrokes { enabled: true, ..project.keystrokes.clone() };
    let shortcuts: Vec<Value> = KeystrokeTrack::build(events, &shortcut_settings)
        .presses()
        .map(|(time, text)| {
            Object::from_iter([
                ("source", Value::from(ms(time))),
                ("output", times(&source_time_outputs(clips, time))),
                ("keys", text.into()),
            ])
            .into()
        })
        .collect();
    let idle: Vec<Value> = idle_spans(events, recording.source_duration)
        .into_iter()
        .map(|(start, end)| {
            Object::from_iter([
                ("source", span(start, end)),
                ("output", spans(&source_span_outputs(clips, start, end))),
                ("seconds", ms(end - start).into()),
            ])
            .into()
        })
        .collect();
    let activity = Object::from_iter([
        ("click_count", Value::from(clicks.len() as i64)),
        ("clicks", click_list.into()),
        ("shortcuts", shortcuts.into()),
        ("idle", idle.into()),
    ]);

    // Everything else in project.json, for `set` operations (overlays and markers have their
    // own lists and operations).
    let mut settings = project.to_json();
    for key in ["clips", "zooms", "version", "defaultsVersion", "name", "overlays", "markers"] {
        settings.remove(key);
    }

    let strings = |list: &[&str]| Value::array(list.iter().map(|&s| Value::from(s)).collect());
    let presets: Vec<Value> = crate::export::presets::PRESETS
        .iter()
        .map(|preset| {
            Object::from_iter([
                ("id", Value::from(preset.id)),
                ("description", preset.description.into()),
                ("format", preset.format.into()),
                ("aspect", preset.aspect.into()),
                ("reframe", if preset.reframe.is_empty() { "off" } else { preset.reframe }.into()),
            ])
            .into()
        })
        .collect();
    let allowed = Object::from_iter([
        ("speeds", Value::array(allowed_clip_speeds().into_iter().map(Value::from).collect())),
        ("layouts", Value::array(allowed_clip_layouts().into_iter().map(Value::from).collect())),
        ("fits", Value::array(allowed_clip_fits().into_iter().map(Value::from).collect())),
        ("transitions", strings(&crate::transitions::STYLES)),
        ("overlay_kinds", strings(&crate::project::OVERLAY_KINDS)),
        ("blur_styles", strings(&crate::project::BLUR_STYLES)),
        ("loudness", strings(&crate::project::AudioCleanup::LOUDNESS_TARGETS)),
        ("reframe", strings(&["follow", "off"])),
        ("aspects", strings(&crate::project::allowed_aspects())),
        ("presets", presets.into()),
    ]);

    let mut summary = Object::from_iter([
        ("bundle", Value::from(document.bundle_path())),
        ("name", project.name.as_str().into()),
        ("source", source.into()),
        ("output", output.into()),
        ("clips", clip_list.into()),
        ("zooms", zooms.into()),
        ("activity", activity.into()),
        ("settings", settings.into()),
        ("allowed", allowed.into()),
    ]);
    if let Some(transcript) = document.transcript() {
        let segments: Vec<Value> = transcript
            .segments
            .iter()
            .map(|segment| {
                Object::from_iter([
                    ("source", span(segment.start, segment.end)),
                    ("output", spans(&source_span_outputs(clips, segment.start, segment.end))),
                    ("text", segment.text.as_str().into()),
                ])
                .into()
            })
            .collect();
        summary.insert(
            "transcript",
            Object::from_iter([
                ("backend", Value::from(transcript.backend.as_str())),
                ("word_timing", transcript.word_timing.name().into()),
                ("segments", segments.into()),
            ]),
        );
    }
    // chapters.json: unreadable is the same as missing here; `set-chapters` reports it.
    if let Ok(Some(chapters)) = crate::chapters::Chapters::load(document.bundle_path()) {
        summary.insert("chapters", chapters_summary(&chapters, clips));
    }
    if !project.overlays.is_empty() {
        let overlays: Vec<Value> =
            project.overlays.iter().map(|overlay| super::polish_ops::overlay_summary(document, overlay).into()).collect();
        summary.insert("overlays", overlays);
    }
    if !project.markers.is_empty() {
        let markers: Vec<Value> = project
            .markers
            .iter()
            .map(|marker| {
                Object::from_iter([
                    ("id", Value::from(marker.id.as_str())),
                    ("label", marker.label.as_str().into()),
                    ("kind", marker.kind.as_str().into()),
                    ("source", ms(marker.time).into()),
                    ("output", times(&source_time_outputs(clips, marker.time))),
                ])
                .into()
            })
            .collect();
        summary.insert("markers", markers);
    }
    let cuts = super::recording_cuts(document.bundle_path(), recording.source_duration);
    if !cuts.is_empty() {
        let cuts: Vec<Value> = cuts
            .into_iter()
            .map(|(kind, start, end)| {
                Object::from_iter([
                    ("kind", Value::from(kind)),
                    ("source", span(start, end)),
                    ("output", spans(&source_span_outputs(clips, start, end))),
                ])
                .into()
            })
            .collect();
        summary.insert("recording_cuts", cuts);
    }
    let transition_cuts: Vec<Value> = document
        .transition_cuts()
        .into_iter()
        .map(|cut| {
            Object::from_iter([
                ("clip", Value::from(clips[cut.clip].id.as_str())),
                ("output", ms(cut.time).into()),
                ("style", cut.style.name().into()),
            ])
            .into()
        })
        .collect();
    summary.insert(
        "transitions",
        Object::from_iter([("style", Value::from(project.transition.as_str())), ("cuts", transition_cuts.into())]),
    );
    summary.insert(
        "reframe",
        Object::from_iter([
            ("mode", Value::from(if project.reframe.is_empty() { "off" } else { project.reframe.as_str() })),
            ("active", crate::reframe::active(project, width, height).into()),
        ]),
    );
    let cleanup = &project.audio.cleanup;
    summary.insert(
        "audio_cleanup",
        Object::from_iter([
            ("denoise", Value::from(cleanup.denoise)),
            ("strength", cleanup.strength.into()),
            ("loudness", cleanup.loudness.as_str().into()),
            ("denoise_available", document.denoise_available().into()),
        ]),
    );
    if let Some(music) = document.music() {
        let mut object = Object::from_iter([("file", Value::from(music.file.as_str()))]);
        if let Some(duration) = document.music_duration() {
            object.insert("duration", ms(duration));
        }
        summary.insert("music", object);
    }
    summary
}

/// chapters.json with each chapter's output time (`null` once its stretch was cut).
pub fn chapters_summary(chapters: &crate::chapters::Chapters, clips: &[crate::project::Clip]) -> Object {
    let placed = chapters.on_timeline(clips);
    let list: Vec<Value> = chapters
        .chapters
        .iter()
        .map(|chapter| {
            let output = placed
                .iter()
                .find(|p| p.source == chapter.source && p.title == chapter.title)
                .map_or(Value::Null, |p| Value::from(ms(p.output)));
            Object::from_iter([
                ("source", Value::from(ms(chapter.source))),
                ("output", output),
                ("title", chapter.title.as_str().into()),
            ])
            .into()
        })
        .collect();
    Object::from_iter([
        ("title", Value::from(chapters.title.as_str())),
        ("description", chapters.description.as_str().into()),
        ("chapters", list.into()),
    ])
}

/// `omareel describe <bundle>`: the summary, or the error to report. `bundle` is absolute.
pub fn run(bundle: &str) -> Result<String, String> {
    let document = super::open_document(bundle)?;
    Ok(describe(&document).to_json(Format::Indented))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(time: f64) -> InputEvent {
        InputEvent { time, ..InputEvent::default() }
    }

    #[test]
    fn idle_spans_cover_the_gaps_and_the_ends() {
        let events = [event(2.5), event(3.0), event(3.5), event(8.0), event(8.1)];
        assert_eq!(idle_spans(&events, 12.0), vec![(0.0, 2.5), (3.5, 8.0), (8.1, 12.0)]);
        assert_eq!(idle_spans(&[], 1.5), vec![]);
        assert_eq!(idle_spans(&[], 5.0), vec![(0.0, 5.0)]);
    }
}
