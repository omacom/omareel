//! Overlay edits on the document: adding, patching, removing, undo, and what cuts do to them.

use super::*;
use crate::compat::geometry::RectF;
use crate::project::Overlay;

fn overlay(qt: &Qt, id: &str) -> Overlay {
    qt.document.overlay(id).cloned().unwrap_or_else(|| panic!("no overlay {id}"))
}

fn patch(json: &str) -> Object {
    crate::compat::qjson::object_from_json(json.as_bytes()).unwrap()
}

#[test]
fn add_picks_the_time_base_from_the_anchor() {
    let world = World::new("overlay-add");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.seek(1.0);
    assert!(qt.run(Document::split_at_playhead));
    let second = qt.clip_id(1);
    assert!(qt.run(|d| d.trim_clip(&second, 1.5, 2.0)));
    // Output 1.2 is source 1.7 now.
    let title = qt.run(|d| d.add_overlay_at("text", 1.2, 3.0));
    let t = overlay(&qt, &title);
    assert_eq!((t.anchor.as_str(), t.start), ("frame", 1.2));
    assert!((t.end - 1.5).abs() < 1e-9, "clamped to the output's end: {}", t.end);
    assert_eq!(qt.document.selected_overlay_id(), title);
    let box_id = qt.run(|d| d.add_overlay_at("box", 1.2, 3.0));
    let b = overlay(&qt, &box_id);
    assert_eq!(b.anchor, "screen");
    assert!((b.start - 1.7).abs() < 1e-6 && (b.end - 2.0).abs() < 1e-6, "{b:?}");
    // Badges count up; unknown kinds are refused.
    let first = qt.run(|d| d.add_overlay("badge", 0.0, 1.0, None, &Object::new()));
    let second_badge = qt.run(|d| d.add_overlay("badge", 0.0, 1.0, None, &Object::new()));
    assert_eq!((overlay(&qt, &first).number, overlay(&qt, &second_badge).number), (1, 2));
    assert!(qt.run(|d| d.add_overlay("sticker", 0.0, 1.0, None, &Object::new())).is_empty());
    // Everything given is kept; blur stays on the screen whatever it is told.
    let blur = qt.run(|d| {
        d.add_overlay("blur", 0.2, 0.8, Some(RectF::new(0.1, 0.1, 0.2, 0.2)), &patch(r#"{"anchor":"frame","style":"blur","amount":0.9}"#))
    });
    let blur = overlay(&qt, &blur);
    assert_eq!((blur.anchor.as_str(), blur.style.as_str(), blur.amount), ("screen", "blur", 0.9));
    assert_eq!(blur.rect, RectF::new(0.1, 0.1, 0.2, 0.2));
    // Selecting a clip leaves the overlay.
    let clip = qt.clip_id(0);
    qt.run(|d| d.set_selected_clip_id(&clip));
    assert!(qt.document.selected_overlay_id().is_empty());
}

#[test]
fn edits_are_one_undo_step_each_and_drags_coalesce() {
    let world = World::new("overlay-undo");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let steps = qt.undo_steps();
    let id = qt.run(|d| d.add_overlay("text", 0.0, 1.0, None, &patch(r#"{"text":"Hello"}"#)));
    assert_eq!(qt.undo_steps(), steps + 1);
    assert_eq!(overlay(&qt, &id).text, "Hello");
    // A drag: many patches, one step.
    qt.run(|d| d.begin_coalesced_edit("overlay-drag"));
    for i in 1..=3 {
        let x = 0.1 * f64::from(i);
        assert!(qt.run(|d| d.update_overlay(&id, &patch(&format!(r#"{{"rect":{{"x":{x}}}}}"#)), true)));
    }
    qt.run(Document::end_coalesced_edit);
    assert_eq!(qt.undo_steps(), steps + 2);
    let moved = overlay(&qt, &id);
    assert!((moved.rect.x - 0.3).abs() < 1e-9);
    assert_eq!(moved.rect.width, Overlay::new_of_kind("text").unwrap().rect.width);
    // Nothing changing is no step.
    assert!(!qt.run(|d| d.update_overlay(&id, &patch(r#"{"text":"Hello"}"#), false)));
    assert_eq!(qt.undo_steps(), steps + 2);
    qt.run(Document::undo);
    assert_eq!(overlay(&qt, &id).rect.x, Overlay::new_of_kind("text").unwrap().rect.x);
    assert!(qt.run(|d| d.remove_overlay(&id)));
    assert!(qt.document.overlays().is_empty());
    assert!(qt.document.selected_overlay_id().is_empty());
    qt.run(Document::undo);
    assert_eq!(qt.document.overlays().len(), 1);
    qt.run(Document::undo);
    assert!(qt.document.overlays().is_empty());
    qt.run(Document::redo);
    assert_eq!(overlay(&qt, &id).text, "Hello");
}

#[test]
fn cuts_shrink_and_remove_overlays_but_never_a_blur() {
    let world = World::new("overlay-cut");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let none = Object::new();
    let box_id = qt.run(|d| d.add_overlay("box", 0.2, 1.6, None, &none));
    let gone_arrow = qt.run(|d| d.add_overlay("arrow", 0.5, 1.0, None, &none));
    let blur = qt.run(|d| d.add_overlay("blur", 0.5, 1.5, None, &none));
    let cut_blur = qt.run(|d| d.add_overlay("blur", 0.6, 1.0, None, &none));
    let late_title = qt.run(|d| d.add_overlay("text", 1.4, 1.9, None, &none));
    let inside_title = qt.run(|d| d.add_overlay("text", 0.5, 1.0, None, &none));
    let across_title = qt.run(|d| d.add_overlay("card", 0.0, 0.6, None, &none));
    assert!(qt.run(|d| d.delete_output_range(0.4, 1.3)));
    // Screen items keep source times: what still shows of them.
    let b = overlay(&qt, &box_id);
    assert!((b.start - 0.2).abs() < 0.03 && (b.end - 1.6).abs() < 0.03, "{b:?}");
    assert!(qt.document.overlay(&gone_arrow).is_none());
    // A blur keeps its whole span, even one whose footage is all cut.
    let b2 = overlay(&qt, &blur);
    assert_eq!((b2.start, b2.end), (0.5, 1.5));
    let b3 = overlay(&qt, &cut_blur);
    assert_eq!((b3.start, b3.end), (0.6, 1.0));
    assert_eq!(qt.document.overlay_output_range(&b3), (-1.0, -1.0));
    // Frame items keep output times: later ones move back, ones across shrink, ones inside go.
    let late = overlay(&qt, &late_title);
    assert!((late.start - 0.5).abs() < 0.03 && (late.end - 1.0).abs() < 0.03, "{late:?}");
    assert!(qt.document.overlay(&inside_title).is_none());
    let across = overlay(&qt, &across_title);
    assert!((across.start - 0.0).abs() < 1e-9 && (across.end - 0.4).abs() < 0.03, "{across:?}");
    // Where the box plays now: output 0.2–0.4 and the part after the cut.
    let (start, end) = qt.document.overlay_output_range(&b);
    assert!((start - 0.2).abs() < 0.03 && (end - 0.7).abs() < 0.03, "{start} {end}");
    // The cut is one step, overlays included.
    qt.run(Document::undo);
    assert_eq!(qt.document.overlays().len(), 7);
    assert_eq!(overlay(&qt, &late_title).start, 1.4);

    // Removing a clip cuts its output span the same way.
    qt.seek(1.0);
    assert!(qt.run(Document::split_at_playhead));
    let first = qt.clip_id(0);
    assert!(qt.run(|d| d.remove_clip(&first)));
    let late = overlay(&qt, &late_title);
    assert!((late.start - 0.4).abs() < 0.03 && (late.end - 0.9).abs() < 0.03, "{late:?}");
}

#[test]
fn changing_the_anchor_converts_time_and_place() {
    let world = World::new("overlay-anchor");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    // Cut the first half second, so output and source differ by 0.5.
    assert!(qt.run(|d| d.delete_output_range(0.0, 0.5)));
    let id = qt.run(|d| d.add_overlay("box", 1.0, 1.4, Some(RectF::new(0.0, 0.0, 1.0, 1.0)), &Object::new()));
    assert!(qt.run(|d| d.update_overlay(&id, &patch(r#"{"anchor":"frame"}"#), false)));
    let o = overlay(&qt, &id);
    assert!((o.start - 0.5).abs() < 0.03 && (o.end - 0.9).abs() < 0.03, "{o:?}");
    // The whole recording becomes the frame's place on the canvas.
    let frame = qt.document.screen_frame_on_canvas();
    assert!((o.rect.x - frame.x).abs() < 1e-6 && (o.rect.width - frame.width).abs() < 1e-6, "{o:?} {frame:?}");
    assert!(frame.x > 0.0 && frame.width < 1.0);
    assert!(qt.run(|d| d.update_overlay(&id, &patch(r#"{"anchor":"screen"}"#), false)));
    let back = overlay(&qt, &id);
    assert!((back.start - 1.0).abs() < 0.03 && (back.rect.x).abs() < 1e-6 && (back.rect.width - 1.0).abs() < 1e-6, "{back:?}");
}

#[test]
fn preview_samples_what_the_exporter_samples() {
    let world = World::new("overlay-sample");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let id = qt.run(|d| d.add_overlay("box", 0.5, 1.5, None, &patch(r#"{"fadeIn":0,"fadeOut":0}"#)));
    qt.seek(1.0);
    let shown = qt.document.preview_overlays();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].value("id").to_string_or(""), id);
    assert_eq!(shown[0].value("opacity").to_double(0.0), 1.0);
    qt.seek(1.6);
    assert!(qt.document.preview_overlays().is_empty());
}

#[test]
fn footage_restored_after_a_cut_comes_back_blurred() {
    let world = World::new("overlay-blur-restore");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let none = Object::new();
    let blur = qt.run(|d| d.add_overlay("blur", 0.5, 1.5, None, &none));
    let all_cut = qt.run(|d| d.add_overlay("blur", 1.2, 1.8, None, &none));
    assert!(qt.run(|d| d.delete_output_range(1.0, 5.0)));
    assert_eq!((overlay(&qt, &blur).start, overlay(&qt, &blur).end), (0.5, 1.5));
    assert_eq!((overlay(&qt, &all_cut).start, overlay(&qt, &all_cut).end), (1.2, 1.8));
    // Restoring the footage by any edit brings the region back over all of it.
    let c = qt.clip_id(0);
    assert!(qt.run(|d| d.reset_clip_trims(&c)));
    let (start, end) = qt.document.overlay_output_range(&overlay(&qt, &blur));
    assert!((start - 0.5).abs() < 1e-9 && (end - 1.5).abs() < 1e-9, "{start} {end}");
    let (start, end) = qt.document.overlay_output_range(&overlay(&qt, &all_cut));
    assert!((start - 1.2).abs() < 1e-9 && (end - 1.8).abs() < 1e-9, "{start} {end}");
    // And by a trim.
    qt.run(Document::undo);
    let c = qt.clip_id(0);
    assert!(qt.run(|d| d.trim_clip(&c, 0.0, 1.7)));
    assert_eq!((overlay(&qt, &blur).start, overlay(&qt, &blur).end), (0.5, 1.5));
}

#[test]
fn frame_items_keep_to_the_video_on_every_edit() {
    let world = World::new("overlay-end-card");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let none = Object::new();
    let d0 = qt.document.duration();
    let card = qt.run(|d| d.add_overlay("card", d0 - 0.5, d0, None, &none));
    let title = qt.run(|d| d.add_overlay("text", 0.2, 0.8, None, &none));
    let late = qt.run(|d| d.add_overlay("text", d0 - 1.0, d0 - 0.7, None, &none));
    let at_end = |qt: &Qt, what: &str| {
        let duration = qt.document.duration();
        let o = overlay(qt, &card);
        assert!((o.end - duration).abs() < 1e-9 && (o.start - (duration - 0.5)).abs() < 1e-9, "{what}: {o:?}, duration {duration}");
        // An item in the middle keeps its output times.
        let t = overlay(qt, &title);
        assert_eq!((t.start, t.end), (0.2, 0.8), "{what}");
    };

    // A trim shortens the video: the end card stays at the end; an item past the end moves in.
    let c = qt.clip_id(0);
    assert!(qt.run(|d| d.trim_clip(&c, 0.0, 1.0)));
    at_end(&qt, "trim");
    let l = overlay(&qt, &late);
    assert!(l.end <= qt.document.duration() + 1e-9 && (l.end - l.start - 0.3).abs() < 1e-9, "{l:?}");
    // Trimming back out takes the card along again; the late item stays where it went.
    assert!(qt.run(|d| d.trim_clip(&c, 0.0, d0)));
    at_end(&qt, "trim back");
    qt.run(Document::undo);
    assert_eq!(overlay(&qt, &card).end, d0);

    // A speed change.
    let c = qt.clip_id(0);
    assert!(qt.run(|d| d.set_clip_speed(&c, 2.0)));
    at_end(&qt, "speed");
    qt.run(Document::undo);
    assert_eq!(overlay(&qt, &card).end, d0);

    // An inserted video at the end.
    qt.seek(d0);
    qt.document.finish_import(ImportedInsert {
        path: "media/clip.mp4".into(),
        info: crate::media_probe::VideoInfo { width: 640, height: 360, duration: 2.0, fps: 30.0, audio: false },
        error: String::new(),
    });
    assert!((qt.document.duration() - (d0 + 2.0)).abs() < 1e-9);
    at_end(&qt, "insert");

    // A split and a merge change nothing.
    qt.seek(0.5);
    assert!(qt.run(Document::split_at_playhead));
    at_end(&qt, "split");
    let c = qt.clip_id(0);
    assert!(qt.run(|d| d.merge_clip(&c, 1)));
    at_end(&qt, "merge");
}

#[test]
fn the_preview_shows_a_blur_early_and_late_and_across_cuts() {
    let world = World::new("overlay-blur-preview");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let none = Object::new();
    let blur = qt.run(|d| d.add_overlay("blur", 1.0, 1.5, None, &none));
    qt.run(|d| d.set_selected_overlay_id(""));
    let shown = |qt: &Qt| qt.document.preview_overlays().iter().any(|o| o.value("id").as_str() == Some(blur.as_str()));
    for (at, expected) in [(0.9, false), (0.97, true), (1.2, true), (1.53, true), (1.6, false)] {
        qt.seek(at);
        assert_eq!(shown(&qt), expected, "at {at}: position {}, source {}", qt.document.output_position(), qt.document.source_position());
    }
    // The cell size comes along for the preview's grid.
    qt.seek(1.2);
    let item = qt.document.preview_overlays().into_iter().find(|o| o.value("id").as_str() == Some(blur.as_str())).unwrap();
    assert!(item.value("block").to_double(0.0) >= 0.04);
    // Cut 1.3–1.9: just after the cut (recording 1.9, past the blur), the video may still be on
    // the frame before it, so the blur stays a moment.
    assert!(qt.run(|d| d.delete_output_range(1.3, 1.9)));
    qt.seek(1.32);
    assert!(shown(&qt));
    qt.seek(1.4);
    assert!(!shown(&qt));
}

// Trimming pauses and fillers cut source ranges: frame items after each cut move back with it,
// screen items shrink as zooms do, and a blur keeps its whole span.
#[test]
fn source_range_cuts_move_frame_items_and_keep_blurs() {
    let world = World::new("overlay-source-cuts");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let late = qt.run(|d| d.add_overlay("text", 1.5, 1.8, None, &Object::new()));
    let inside = qt.run(|d| d.add_overlay("text", 0.25, 0.35, None, &Object::new()));
    let label = qt.run(|d| d.add_overlay("box", 0.2, 0.6, None, &Object::new()));
    let blur = qt.run(|d| d.add_overlay("blur", 0.2, 0.6, Some(RectF::new(0.1, 0.1, 0.2, 0.2)), &Object::new()));
    // Two cuts of the 2 s recording: 0.2–0.4 and 1.0–1.2 (0.2 s each).
    assert_eq!(qt.run(|d| d.remove_source_ranges(&[(1.0, 1.2), (0.2, 0.4)])), 2);
    let l = overlay(&qt, &late);
    assert!((l.start - 1.1).abs() < 1e-9 && (l.end - 1.4).abs() < 1e-9, "{l:?}");
    // An item inside a cut goes, as with a deleted range.
    assert!(qt.document.overlay(&inside).is_none());
    let b = overlay(&qt, &label);
    assert!((b.start - 0.4).abs() < 1e-9 && (b.end - 0.6).abs() < 1e-9, "{b:?}");
    let r = overlay(&qt, &blur);
    assert_eq!((r.start, r.end), (0.2, 0.6));
    // One undo step brings everything back.
    qt.run(Document::undo);
    assert!(qt.document.overlay(&inside).is_some());
    assert_eq!(overlay(&qt, &late).start, 1.5);
}
