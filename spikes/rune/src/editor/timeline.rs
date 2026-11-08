//! BottomBar.qml and Timeline.qml: the transport (play, time, step, playback range), the tools
//! (add zoom, split, insert clip, timeline scale) and the tracks (ClipTrack with trims, speed and
//! layout; ZoomTrack with add, move, resize, multi-select; the keys row; thumbnails), the ruler
//! and the playhead.

use super::{edited, label, mono};
use crate::preview::{Preview, play_glyph, tenths};
use omareel_core::editor::Effect;
use rune::{Color, Cursor, Cx, El, Key, KeyEvent, Phase, Spring, Tx, col, el, row, text};
use std::time::Duration;

/// The timeline's own UI state (drags).
#[derive(Default)]
pub struct State {
    /// A zoom being dragged: its id, its source start when the drag began, and the timeline's
    /// points per second (handlers have no layout; the block's own width gives it).
    pub zoom_drag: Option<(String, f64, f32)>,
}

const TRACK_H: f32 = 34.0;
const HEADER_W: f32 = 84.0;

pub fn on_effect(_s: &mut Preview, _effect: &Effect) {}

pub fn poll(_s: &mut Preview) -> bool {
    false
}

pub fn busy(s: &Preview) -> bool {
    s.edit.timeline.zoom_drag.is_some()
}

/// S splits at the playhead, Delete removes the selected zoom.
pub fn on_key(s: &mut Preview, key: &KeyEvent, tx: &mut Tx) -> bool {
    match &key.key {
        Key::Text(t) if t == "s" => {
            if s.document.split_at_playhead() {
                edited(s, "Split clip");
            }
            true
        }
        Key::Delete | Key::Backspace => {
            let id = s.document.selected_zoom_id().to_owned();
            if !id.is_empty() && s.document.remove_zoom(&id) {
                edited(s, "Delete zoom");
                tx.wake_after(Duration::ZERO);
            }
            true
        }
        _ => false,
    }
}


pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let duration = s.document.duration().max(0.001) as f32;
    let tracks_w = cx.layout_size("tracks").map_or(1000.0, |[w, _]| w).max(1.0);
    let pps = tracks_w / duration;
    let position = s.document.output_position() as f32;

    // Header: play, time, hints.
    let play = el()
        .key("tl-play")
        .role("button")
        .label(if s.playing { "Pause" } else { "Play" })
        .size(32.0, 32.0)
        .radius(16.0)
        .center()
        .fill(ui.accent)
        .hover(ui.accent.mix(Color::WHITE, 0.1))
        .press_scale(0.94)
        .cursor(Cursor::Pointer)
        .on_press_tx(|s: &mut Preview, tx: &mut Tx| {
            s.toggle();
            tx.wake_after(Duration::ZERO);
        })
        .child(play_glyph(s.playing, ui.accent_text));
    let header = row()
        .key("tl-header")
        .w_full()
        .h(44.0)
        .px(16.0)
        .gap(12.0)
        .items_center()
        .child(play)
        .child(text(format!("{} / {}", tenths(f64::from(position)), tenths(f64::from(duration)))).font(mono(13.0, 500.0)).color(ui.text))
        .child(el().grow(1.0))
        .child(text("Space play · S split · ⌫ delete zoom · ⌘Z undo").font(label(12.0, 400.0)).color(ui.text_faint));

    // The ruler and the tracks share the x axis: seconds × pps from the tracks' left.
    let mut ruler = el().key("ruler").w_full().h(22.0).cursor(Cursor::Text).on_drag(|s: &mut Preview, drag, tx: &mut Tx| {
        let t = f64::from((drag.x / drag.w.max(1.0)).clamp(0.0, 1.0)) * s.document.duration();
        s.scrubbing = !matches!(drag.phase, Phase::Up);
        s.seek(t);
        tx.wake_after(Duration::ZERO);
    });
    let step = if pps > 80.0 { 1.0 } else if pps > 30.0 { 2.0 } else { 5.0 };
    let mut tick = 0.0;
    while tick <= duration + 0.001 {
        let x = tick * pps;
        ruler = ruler
            .child(el().absolute(x, 14.0).size(1.0, 8.0).fill(ui.text_faint.alpha(0.5)))
            .child(text(format!("{}:{:02}", (tick as i32) / 60, (tick as i32) % 60)).font(mono(10.0, 500.0)).color(ui.text_faint).absolute(x + 4.0, 1.0));
        tick += step;
    }

    let clips = clip_track(s, ui, pps);
    let zooms = zoom_track(s, ui, pps);
    let keys = key_track(s, ui, pps);
    // The playhead glides on seeks (the layout spring) and lands at once while playing.
    let playhead = el()
        .key("playhead")
        .absolute(position * pps - 1.0, 0.0)
        .size(2.0, 22.0 + 3.0 * (TRACK_H + 6.0))
        .fill(ui.accent)
        .motion(Spring::new(0.25, 0.0))
        .inert()
        .child(el().absolute(-5.0, 0.0).size(12.0, 12.0).radius(6.0).fill(ui.accent));
    let tracks = col().key("tracks").grow(1.0).min_w(0.0).gap(6.0).child(ruler).child(clips).child(zooms).child(keys).child(playhead);
    let headers = col()
        .w(HEADER_W)
        .gap(6.0)
        .pt(22.0 + 6.0)
        .child(track_label(ui, "Clips"))
        .child(track_label(ui, "Zoom"))
        .child(track_label(ui, "Keys"));
    col()
        .key("timeline")
        .w_full()
        .h(232.0)
        .fill(ui.surface)
        .child(header)
        .child(row().w_full().grow(1.0).px(16.0).pb(16.0).child(headers).child(tracks))
}

fn track_label(ui: &crate::theme::Ui, name: &str) -> El {
    row().h(TRACK_H).items_center().child(text(name.to_owned()).font(label(12.0, 600.0)).color(ui.text_muted))
}

fn clip_track(s: &Preview, ui: &crate::theme::Ui, pps: f32) -> El {
    let mut track = el().key("clip-track").w_full().h(TRACK_H);
    let mut start = 0.0f32;
    let selected = s.document.selected_clip_id().to_owned();
    for (i, clip) in s.document.project().clips.iter().enumerate() {
        let length = ((clip.out - clip.in_) / clip.speed.max(0.01)) as f32;
        let camera = clip.layout == "camera";
        let fill = if camera { ui.zoom_accent.mix(ui.card, 0.55) } else { ui.accent.mix(ui.card, 0.7) };
        let is_selected = clip.id == selected;
        let id = clip.id.clone();
        let mut block = row()
            .key(format!("clip-{}", clip.id))
            .absolute(start * pps + 1.0, 0.0)
            .size((length * pps - 2.0).max(4.0), TRACK_H)
            .radius(8.0)
            .px(10.0)
            .gap(6.0)
            .items_center()
            .fill(fill)
            .hover(fill.mix(Color::WHITE, 0.08))
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut Preview| {
                s.document.set_selected_clip_id(&id);
                edited(s, "");
            })
            .child(text(format!("Clip {}", i + 1)).font(label(12.0, 600.0)).color(ui.text_strong).truncate())
            .child(text(format!("{length:.1} s")).font(mono(11.0, 500.0)).color(ui.text_muted));
        if camera {
            block = block.child(text("Camera").font(label(11.0, 600.0)).color(ui.text_strong).px(6.0).radius(4.0).fill(ui.zoom_accent.alpha(0.35)));
        }
        if is_selected {
            block = block.child(selection_ring(ui, (length * pps - 2.0).max(4.0)));
        }
        track = track.child(block);
        start += length;
    }
    track
}

/// The ring around the selected item: one element, matched across tracks, so it flies to the
/// new selection.
fn selection_ring(ui: &crate::theme::Ui, w: f32) -> El {
    el().key("selection").matched("selection").absolute(-3.0, -3.0).size(w + 6.0, TRACK_H + 6.0).radius(11.0).border(2.0, ui.text_strong).inert()
}

fn zoom_track(s: &Preview, ui: &crate::theme::Ui, pps: f32) -> El {
    let mut track = el().key("zoom-track").w_full().h(TRACK_H).on_press(|s: &mut Preview| {
        s.document.set_selected_zoom_id("");
        edited(s, "");
    });
    let selected = s.document.selected_zoom_id().to_owned();
    for zoom in &s.document.project().zooms {
        let (start, end) = s.document.source_range_to_output(zoom.start, zoom.end);
        if start < 0.0 || end <= start {
            continue;
        }
        let id = zoom.id.clone();
        let is_selected = zoom.id == selected;
        let dragging = s.edit.timeline.zoom_drag.as_ref().is_some_and(|(d, ..)| *d == zoom.id);
        let width = ((end - start) as f32 * pps - 2.0).max(18.0);
        let seconds = (end - start) as f32;
        let fill = ui.zoom_accent.mix(ui.card, if is_selected { 0.15 } else { 0.4 });
        let mut block = row()
            .key(format!("zoom-{}", zoom.id))
            .absolute(start as f32 * pps + 1.0, 0.0)
            .size(width, TRACK_H)
            .radius(8.0)
            .px(10.0)
            .items_center()
            .fill(fill)
            .hover(fill.mix(Color::WHITE, 0.08))
            .cursor(if dragging { Cursor::Grabbing } else { Cursor::Grab })
            .scale(if dragging { 1.04 } else { 1.0 })
            .shadow(if dragging { 8.0 } else { 0.0 }, if dragging { 18.0 } else { 0.0 }, Color::BLACK.alpha(if dragging { 0.35 } else { 0.0 }))
            .on_drag(move |s: &mut Preview, drag, tx: &mut Tx| {
                match drag.phase {
                    Phase::Down => {
                        s.document.set_selected_zoom_id(&id);
                        let start = s.document.project().zooms.iter().find(|z| z.id == id).map_or(0.0, |z| z.start);
                        let pps = (drag.w + 2.0) / seconds.max(0.01);
                        s.edit.timeline.zoom_drag = Some((id.clone(), start, pps));
                        s.document.begin_coalesced_edit("zoom-drag");
                    }
                    Phase::Move => {
                        if let Some((_, from, pps)) = &s.edit.timeline.zoom_drag {
                            let to = (from + f64::from(drag.dx / pps.max(1.0))).max(0.0);
                            s.document.move_zoom(&id, to);
                        }
                    }
                    Phase::Up => {
                        s.edit.timeline.zoom_drag = None;
                        s.document.end_coalesced_edit();
                        // The lift and shadow settle on springs.
                        tx.animated();
                    }
                }
                edited(s, if matches!(drag.phase, Phase::Up) && drag.dx.abs() > 2.0 { "Move zoom" } else { "" });
                tx.wake_after(Duration::ZERO);
            })
            .child(text(format!("{:.1}×", zoom.level)).font(mono(12.0, 600.0)).color(ui.text_strong));
        if is_selected {
            block = block.child(selection_ring(ui, width));
        }
        track = track.child(block);
    }
    track
}

fn key_track(s: &Preview, ui: &crate::theme::Ui, pps: f32) -> El {
    let mut track = el().key("key-track").w_full().h(TRACK_H);
    for (i, event) in s.document.input().events().iter().enumerate() {
        if event.kind != omareel_core::input_log::InputKind::KeyDown || event.modifiers.is_empty() {
            continue;
        }
        let t = s.document.source_to_output(event.time, -1);
        if t < 0.0 {
            continue;
        }
        let mods: Vec<&str> = event.modifiers.iter().map(|m| omareel_core::keystroke_track::modifier_label(m)).collect();
        let key = event.key_label.clone();
        let label = format!("{}+{}", mods.join("+"), if key.is_empty() { event.key_name.trim_start_matches("KEY_").to_owned() } else { key });
        track = track.child(
            row()
                .key(format!("key-{i}"))
                .absolute(t as f32 * pps, 6.0)
                .h(TRACK_H - 12.0)
                .px(7.0)
                .items_center()
                .radius(6.0)
                .fill(ui.normal_fill)
                .border(1.0, ui.hairline)
                .child(text(label).font(mono(11.0, 600.0)).color(ui.text)),
        );
    }
    track
}
