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
fn cuts_shrink_and_remove_overlays() {
    let world = World::new("overlay-cut");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let none = Object::new();
    let box_id = qt.run(|d| d.add_overlay("box", 0.2, 1.6, None, &none));
    let gone_box = qt.run(|d| d.add_overlay("blur", 0.5, 1.0, None, &none));
    let late_title = qt.run(|d| d.add_overlay("text", 1.4, 1.9, None, &none));
    let inside_title = qt.run(|d| d.add_overlay("text", 0.5, 1.0, None, &none));
    let across_title = qt.run(|d| d.add_overlay("card", 0.0, 0.6, None, &none));
    assert!(qt.run(|d| d.delete_output_range(0.4, 1.3)));
    // Screen items keep source times: what still shows of them.
    let b = overlay(&qt, &box_id);
    assert!((b.start - 0.2).abs() < 0.03 && (b.end - 1.6).abs() < 0.03, "{b:?}");
    assert!(qt.document.overlay(&gone_box).is_none());
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
    assert_eq!(qt.document.overlays().len(), 5);
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
