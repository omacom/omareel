//! Timeline.qml, ClipTrack.qml, ZoomTrack.qml and OmTrimHandle.qml: the track labels, the
//! scrolling tracks (ruler, clip track, range, zoom track, camera row), the playhead and the
//! scroll bar.
//!
//! Geometry is Timeline.qml's: labels 72 pt wide, a 32 pt ruler, 48 pt rows 8 pt apart with 40
//! pt blocks inside, and 40 pt of room after the end. Handlers get no layout, so every drag
//! keeps the content position it started from (the element's own point on Down) and adds the
//! pointer's travel (`Drag::dx`).

use super::popup::badge_speed;
use super::thumbs::Tile;
use super::{
    Edge, Gesture, LABEL_W, Popup, begin_range, begin_range_resize, cancel_range, clear_range, content_w, content_x, control_radius, finish_range, pps,
    set_scale, small_radius, update_range,
};
use crate::editor::{edited, label, mono};
use crate::icons;
use crate::kit::icon;
use crate::preview::Preview;
use crate::theme::Ui;
use rune::{Color, Cursor, Cx, Drag, El, Phase, PointerButton, Shape, SurfaceFit, Tx, col, el, row, text};
use std::time::Duration;

const RULER_H: f32 = 32.0;
const ROW_STEP: f32 = 56.0;
const CLIP_Y: f32 = 36.0;
const ZOOM_Y: f32 = 92.0;
const CAMERA_Y: f32 = 148.0;
const BLOCK_H: f32 = 40.0;
const HANDLE_W: f32 = 14.0;

fn alpha(c: Color, a: f32) -> Color {
    c.alpha(c.a * a)
}

/// `Qt.lighter(c, f)`, near enough: towards white.
fn lighter(c: Color, f: f32) -> Color {
    c.mix(Color::WHITE, (f - 1.0).clamp(0.0, 1.0))
}

fn wake(tx: &mut Tx) {
    tx.wake_after(Duration::ZERO);
}

/// The time at a content position.
fn time_at(s: &Preview, x: f32) -> f64 {
    (f64::from(x / pps(s))).clamp(0.0, s.document.duration())
}

fn shift(tx: &Tx) -> bool {
    tx.mods().shift
}

fn ctrl(tx: &Tx) -> bool {
    tx.mods().ctrl || tx.mods().cmd
}

/// `outputStart(index)`.
fn output_start(s: &Preview, index: usize) -> f64 {
    s.document.project().clips.iter().take(index).map(|c| (c.out - c.in_) / c.speed).sum()
}

/// The clip's trim bounds (`modelData.lower`, `modelData.upper` from `projectMap`).
fn clip_bounds(s: &Preview, index: usize) -> (f64, f64) {
    let map = s.document.project_map();
    let clips = map.value("clips").to_array();
    clips.get(index).map_or((0.0, s.document.source_duration()), |c| {
        let c = c.to_object();
        (c.value("lower").to_double(0.0), c.value("upper").to_double(s.document.source_duration()))
    })
}

fn hovered(s: &Preview, key: &str) -> bool {
    s.edit.timeline.hover.as_deref() == Some(key)
}

/// Tracks the element under the pointer for looks that depend on it.
fn hover_key(e: El, key: String) -> El {
    e.on_hover_change(move |s: &mut Preview, on: bool, tx: &mut Tx| {
        if on {
            s.edit.timeline.hover = Some(key.clone());
        } else if s.edit.timeline.hover.as_deref() == Some(key.as_str()) {
            s.edit.timeline.hover = None;
        }
        tx.animated();
    })
}

// ---------------------------------------------------------------------------------------------
// The timeline

pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let has_camera = s.document.has_camera();
    let height = if has_camera { 204.0 } else { 148.0 };
    if let Some([w, _]) = cx.layout_size("tl-flick") {
        s.edit.timeline.flick_w.set(w.max(1.0));
    }
    let mut labels = col().key("tl-labels").w(LABEL_W).h_full();
    let mut names = vec![("Clip", &icons::FILM), ("Zoom", &icons::ZOOM_IN)];
    if has_camera {
        names.push(("Camera", &icons::CAMERA));
    }
    for (i, (name, i_icon)) in names.into_iter().enumerate() {
        labels = labels.child(
            row()
                .absolute(10.0, RULER_H + i as f32 * ROW_STEP + 16.0)
                .h(16.0)
                .gap(5.0)
                .items_center()
                .child(icon(i_icon, 12.0, ui.text_faint))
                .child(text(name).font(label(11.0, 500.0)).color(ui.text_muted)),
        );
    }

    let px = pps(s);
    let cw = content_w(s);
    let scroll = content_x(s);
    let content = el()
        .key("tl-content")
        .absolute(-scroll, 0.0)
        .size(cw, height)
        .instant_changes()
        .child(ruler(s, ui, px, cw, scroll))
        .child(track_bg(ui, RULER_H, cw.max(s.document.duration() as f32 * px)))
        .child(clip_track(s, ui, px, scroll))
        .child(range(s, ui, px))
        .child(track_bg(ui, RULER_H + ROW_STEP, cw.max(s.document.duration() as f32 * px)))
        .child(zoom_track(s, ui, px, cw));
    let content = if has_camera { content.child(track_bg(ui, RULER_H + 2.0 * ROW_STEP, cw)).child(camera_row(s, ui, px)) } else { content };
    let content = content.child(playhead(s, ui, px, has_camera));

    let flick = el()
        .key("tl-flick")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .clip_soft(0.0)
        .pointer_buttons([PointerButton::Middle])
        .on_wheel(|s: &mut Preview, e, tx: &mut Tx| {
            let amount = if e.dy != 0.0 { e.dy } else { e.dx };
            if amount != 0.0 {
                let factor = 1.25f64.powf(f64::from(amount) / 120.0);
                let scale = s.edit.timeline.scale * factor;
                set_scale(s, scale, e.x);
                wake(tx);
            }
            true
        })
        .on_drag(|s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            // The middle button pans (Qt pans with the right button, which Rune only reports
            // as a click).
            match drag.phase {
                Phase::Down => s.edit.timeline.gesture = Some(Gesture::Pan { from: content_x(s) }),
                Phase::Move => {
                    if let Some(Gesture::Pan { from }) = s.edit.timeline.gesture {
                        s.edit.timeline.content_x = (from - drag.dx).clamp(0.0, (content_w(s) - s.edit.timeline.flick_w.get()).max(0.0));
                    }
                }
                Phase::Up => s.edit.timeline.gesture = None,
            }
            wake(tx);
        })
        .child(content)
        .child(scroll_bar(s, ui, cw, scroll, height));
    row()
        .key("timeline")
        .w_full()
        .h(height)
        .fill(ui.surface)
        .child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(ui.hairline))
        .child(labels)
        .child(flick)
}

/// A row's faint background.
fn track_bg(ui: &Ui, y: f32, w: f32) -> El {
    el().absolute(0.0, y).size(w, 48.0).radius(control_radius(ui)).fill(alpha(ui.foreground, 0.03)).inert()
}

/// The horizontal scroll bar, when the tracks overflow.
fn scroll_bar(s: &Preview, ui: &Ui, cw: f32, scroll: f32, height: f32) -> El {
    let view_w = s.edit.timeline.flick_w.get();
    if cw <= view_w + 0.5 {
        return el().key("tl-scrollbar").size(0.0, 0.0);
    }
    let track_w = (view_w - 8.0).max(1.0);
    let thumb_w = (track_w * view_w / cw).max(32.0).min(track_w);
    let thumb_x = (scroll / (cw - view_w)) * (track_w - thumb_w);
    let active = hovered(s, "tl-scrollbar") || matches!(s.edit.timeline.gesture, Some(Gesture::Pan { .. }));
    let thumb = el()
        .key("tl-scroll-thumb")
        .absolute(4.0 + thumb_x, if active { 3.0 } else { 4.0 })
        .size(thumb_w, if active { 6.0 } else { 4.0 })
        .radius(3.0)
        .fill(alpha(ui.foreground, if active { 0.35 } else { 0.18 }));
    let bar = el()
        .key("tl-scrollbar")
        .absolute(0.0, height - 12.0)
        .w_full()
        .h(12.0)
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            let view_w = s.edit.timeline.flick_w.get();
            let cw = content_w(s);
            let travel = ((view_w - 8.0) - ((view_w - 8.0) * view_w / cw).max(32.0)).max(1.0);
            match drag.phase {
                Phase::Down => s.edit.timeline.gesture = Some(Gesture::Pan { from: content_x(s) }),
                Phase::Move => {
                    if let Some(Gesture::Pan { from }) = s.edit.timeline.gesture {
                        s.edit.timeline.content_x = (from + drag.dx / travel * (cw - view_w)).clamp(0.0, cw - view_w);
                    }
                }
                Phase::Up => s.edit.timeline.gesture = None,
            }
            wake(tx);
        })
        .child(thumb);
    hover_key(bar, "tl-scrollbar".into())
}

// ---------------------------------------------------------------------------------------------
// Ruler

/// `tickStepSeconds`: about 90 pt between ticks, on 1, 2, 5 steps.
fn tick_step(px: f32) -> f64 {
    let target = 90.0 / f64::from(px.max(0.001));
    let power = 10f64.powf(target.log10().floor());
    for multiple in [1.0, 2.0, 5.0, 10.0] {
        if multiple * power >= target {
            return multiple * power;
        }
    }
    10.0 * power
}

/// `formatRulerTime`.
fn ruler_time(seconds: f64, step: f64) -> String {
    let ms = (seconds * 1000.0).round().max(0.0) as i64;
    let time = format!("{:02}:{:02}", ms / 60000, (ms / 1000) % 60);
    if step >= 1.0 {
        return time;
    }
    let precision = if step >= 0.1 { 1 } else if step >= 0.01 { 2 } else { 3 };
    format!("{time}.{}", &format!("{:03}", ms % 1000)[..precision])
}

fn ruler(s: &Preview, ui: &Ui, px: f32, cw: f32, scroll: f32) -> El {
    let step = tick_step(px);
    let step_px = (step as f32 * px).max(1.0);
    let first = ((scroll / step_px).floor() as i64 - 1).max(0);
    let count = (s.edit.timeline.flick_w.get() / step_px).ceil() as i64 + 4;
    let mut r = el()
        .key("ruler")
        .absolute(0.0, 0.0)
        .size(cw, RULER_H)
        .cursor(Cursor::Pointer)
        .on_drag(|s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            let t = time_at(s, drag.x);
            match drag.phase {
                Phase::Down => {
                    if shift(tx) {
                        begin_range(s, t);
                    } else {
                        s.begin_scrub();
                        s.scrub_to(t);
                        s.edit.timeline.gesture = Some(Gesture::Scrub);
                    }
                }
                Phase::Move => match s.edit.timeline.gesture {
                    Some(Gesture::Range) => update_range(s, t),
                    _ => s.scrub_to(t),
                },
                Phase::Up => {
                    if drag.cancelled() {
                        if s.edit.timeline.gesture == Some(Gesture::Range) {
                            cancel_range(s);
                        } else {
                            s.end_scrub();
                        }
                    } else if s.edit.timeline.gesture == Some(Gesture::Range) {
                        finish_range(s, t);
                    } else {
                        s.scrub_to(t);
                        s.end_scrub();
                    }
                    s.edit.timeline.gesture = None;
                    edited(s, "");
                }
            }
            wake(tx);
        });
    for i in first..first + count {
        let t = i as f64 * step;
        if t > s.document.duration() + 1e-9 {
            break;
        }
        let x = t as f32 * px;
        r = r
            .child(el().absolute(x, RULER_H - 10.0).size(1.0, 6.0).fill(ui.stroke_strong).inert())
            .child(text(ruler_time(t, step)).font(mono(10.0, 400.0)).color(ui.text_faint).absolute(x + 4.0, 6.0).inert());
    }
    r
}

// ---------------------------------------------------------------------------------------------
// The playback range over the clip track

fn range(s: &Preview, ui: &Ui, px: f32) -> El {
    let t = &s.edit.timeline;
    let visible = t.range_anchor >= 0.0 && t.range_head >= 0.0 && (t.resizing_range || ((t.range_head - t.range_anchor).abs() as f32) * px >= 2.0);
    if !visible {
        return el().key("clip-range").size(0.0, 0.0);
    }
    let x = t.range_start() as f32 * px;
    let w = ((t.range_end() - t.range_start()) as f32 * px).max(0.0);
    let handles = !t.selecting_range || t.resizing_range;
    let mut r = el()
        .key("clip-range")
        .absolute(x, RULER_H)
        .size(w, 48.0)
        .fill(alpha(ui.accent, 0.30))
        .border(2.0, ui.accent);
    if handles {
        for edge in [Edge::Left, Edge::Right] {
            let name = if edge == Edge::Left { "range-trim-left" } else { "range-trim-right" };
            let hit = trim_handle(s, ui, name, edge, w, 48.0, ui.accent, true, matches!(t.gesture, Some(Gesture::Range)) && t.resizing_range && t.resize_edge == edge);
            r = r.child(hit.on_drag(move |s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
                let px = pps(s);
                // The pointer in content points: where the handle was at the press, plus travel.
                if matches!(drag.phase, Phase::Down) {
                    let t = &s.edit.timeline;
                    let origin = if edge == Edge::Left { t.range_start() as f32 * px - 6.0 } else { t.range_end() as f32 * px - HANDLE_W };
                    drag.data = origin + drag.start_x;
                }
                let pointer = f64::from((drag.data + drag.dx) / px);
                match drag.phase {
                    Phase::Down => begin_range_resize(s, edge, pointer),
                    Phase::Move => {
                        let offset = s.edit.timeline.resize_offset;
                        update_range(s, (pointer + offset).clamp(0.0, s.document.duration()));
                    }
                    Phase::Up => {
                        if drag.cancelled() {
                            cancel_range(s);
                        } else {
                            let offset = s.edit.timeline.resize_offset;
                            finish_range(s, (pointer + offset).clamp(0.0, s.document.duration()));
                        }
                        edited(s, "");
                    }
                }
                wake(tx);
            }));
        }
    }
    r
}

/// OmTrimHandle: a 14 pt cap on the block's edge, rounded on the outside, with a grip that
/// grows while hovered or dragged; the hit area is 20 pt wide and reaches 6 pt past the block.
#[allow(clippy::too_many_arguments)]
fn trim_handle(s: &Preview, ui: &Ui, key: &str, edge: Edge, block_w: f32, h: f32, accent: Color, shown: bool, engaged: bool) -> El {
    let hover = hovered(s, key);
    let shown = shown || hover || engaged;
    let cap = if engaged { lighter(accent, 1.20) } else if hover { lighter(accent, 1.12) } else { accent };
    let r = control_radius(ui);
    let corners = if edge == Edge::Left { [r, 0.0, 0.0, r] } else { [0.0, r, r, 0.0] };
    let grip_w = if hover || engaged { 3.0 } else { 2.0 };
    let grip_h = h * if hover || engaged { 0.6 } else { 0.4 };
    let visual = el()
        .key(format!("{key}-cap"))
        .absolute(if edge == Edge::Left { 6.0 } else { 0.0 }, 6.0)
        .size(HANDLE_W, h)
        .corners(corners)
        .fill(cap)
        .opacity(if shown { 1.0 } else { 0.0 })
        .inert()
        .child(el().absolute((HANDLE_W - grip_w) / 2.0, (h - grip_h) / 2.0).size(grip_w, grip_h).radius(grip_w / 2.0).fill(ui.surface));
    let x = if edge == Edge::Left { -6.0 } else { block_w - HANDLE_W };
    hover_key(
        el().key(key.to_owned()).absolute(x, -6.0).size(20.0, h + 12.0).cursor(Cursor::ResizeColumn).child(visual),
        key.to_owned(),
    )
}

// ---------------------------------------------------------------------------------------------
// Clips

fn clip_track(s: &Preview, ui: &Ui, px: f32, scroll: f32) -> El {
    let mut track = el()
        .key("clip-track")
        .absolute(0.0, CLIP_Y)
        .size(content_w(s), BLOCK_H)
        .on_drag(|s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            let t = time_at(s, drag.start_x + drag.dx);
            track_scrub(s, drag, tx, t, true);
            wake(tx);
        });
    let selected = s.document.selected_clip_id().to_owned();
    let clips = s.document.project().clips.clone();
    let mut start = 0.0f64;
    for (i, clip) in clips.iter().enumerate() {
        let length = (clip.out - clip.in_) / clip.speed;
        let rest_x = start as f32 * px;
        let rest_w = length as f32 * px;
        start += length;
        let gesture = match &s.edit.timeline.gesture {
            Some(g @ (Gesture::ClipMove { id, .. } | Gesture::ClipTrim { id, .. })) if *id == clip.id => Some(g.clone()),
            _ => None,
        };
        let (x, w) = match &gesture {
            Some(Gesture::ClipMove { x, w, .. } | Gesture::ClipTrim { x, w, .. }) => (*x, *w),
            _ => (rest_x, rest_w),
        };
        let (shown_in, shown_out) = match &gesture {
            Some(Gesture::ClipTrim { edge: Edge::Left, preview, .. }) => (*preview, clip.out),
            Some(Gesture::ClipTrim { edge: Edge::Right, preview, .. }) => (clip.in_, *preview),
            _ => (clip.in_, clip.out),
        };
        let is_selected = clip.id == selected;
        let block_key = format!("clip-{}", clip.id);
        let mut block = el().key(block_key.clone()).absolute(x, 0.0).size(w.max(1.0), BLOCK_H).fill(ui.normal_fill).border(1.0, ui.normal_border);

        // The filmstrip: 16:9 tiles, only those in view.
        let tile_w = BLOCK_H * 16.0 / 9.0;
        let view_w = s.edit.timeline.flick_w.get();
        let first = (((scroll - x) / tile_w).floor() as i64 - 1).max(0);
        let last = ((w / tile_w).ceil() as i64).min(((scroll + view_w - x) / tile_w).ceil() as i64 + 1);
        let mut strip = el().key(format!("filmstrip-{}", clip.id)).absolute(0.0, 0.0).size(w.max(1.0), BLOCK_H).clip_soft(0.0).inert();
        for index in first..last.max(first) {
            let tx0 = index as f32 * tile_w;
            let tw = (tile_w + 1.0).min(w - tx0);
            if tw <= 0.0 {
                continue;
            }
            let source = shown_in + f64::from((tx0 + tw / 2.0) / px) * clip.speed;
            let tile = Tile {
                clip: clip.id.clone(),
                time: (source * 1000.0).round() as i64,
                from: (shown_in * 1000.0).round() as i64,
                to: (shown_out * 1000.0).round() as i64,
            };
            if let Some(path) = s.edit.timeline.thumbs.tile(tile) {
                let image = s.edit.timeline.thumbs.image(&s.cache, &path);
                strip = strip.child(el().key(format!("tile-{}-{index}", clip.id)).absolute(tx0, 0.0).size(tw, BLOCK_H).pixels_fit(image.pixels(), SurfaceFit::Fill));
            }
        }
        block = block.child(strip);

        // The tint and the border over it.
        let missing = clip.is_media() && !s.document.inserts().get(&clip.media).is_some_and(|i| i.duration > 0.0);
        let hover = hovered(s, &block_key);
        let tint = if missing {
            alpha(ui.record, 0.35)
        } else if is_selected {
            alpha(ui.accent, 0.14)
        } else {
            Color::TRANSPARENT
        };
        let border = if is_selected { ui.accent } else if hover { alpha(ui.accent, 0.7) } else { ui.normal_border };
        block = block.child(el().absolute(0.0, 0.0).size(w.max(1.0), BLOCK_H).fill(tint).border(if is_selected { 1.5 } else { 1.0 }, border).inert());

        // The body: scrub (a click selects), Shift-drag a range, Ctrl-drag slides the clip.
        let margin = (w / 3.0).min(14.0);
        let id = clip.id.clone();
        let body = el()
            .key(format!("clip-body-{}", clip.id))
            .absolute(margin, 0.0)
            .size((w - 2.0 * margin).max(0.0), BLOCK_H)
            .cursor(if matches!(gesture, Some(Gesture::ClipMove { .. })) { Cursor::Grabbing } else { Cursor::Pointer })
            .on_drag(move |s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
                clip_body_drag(s, drag, tx, &id, i, margin, false);
                wake(tx);
            });
        block = block.child(body);

        // The badge: name and speed; a click opens the speed menu.
        if w >= 68.0 {
            block = block.child(badge(s, ui, clip, i, w));
        }
        if is_selected {
            for edge in [Edge::Left, Edge::Right] {
                let key = format!("clip-trim-{}-{}", clip.id, if edge == Edge::Left { "left" } else { "right" });
                let engaged = matches!(&gesture, Some(Gesture::ClipTrim { edge: e, .. }) if *e == edge);
                let id = clip.id.clone();
                let handle = trim_handle(s, ui, &key, edge, w, BLOCK_H, ui.accent, true, engaged).on_drag(move |s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
                    clip_trim_drag(s, drag, &id, i, edge);
                    wake(tx);
                });
                block = block.child(handle);
            }
        }
        track = track.child(hover_key(block, block_key));
    }
    track
}

/// Presses on the clip track (`emptyClipArea`) and on a clip's body or badge: scrub, or with
/// Shift a range. `deselect`: the empty track clears the clip selection on press.
fn track_scrub(s: &mut Preview, drag: &Drag, tx: &Tx, t: f64, deselect: bool) {
    match drag.phase {
        Phase::Down => {
            if shift(tx) {
                begin_range(s, t);
            } else {
                if deselect {
                    s.document.set_selected_clip_id("");
                }
                s.begin_scrub();
                s.scrub_to(t);
                s.edit.timeline.gesture = Some(Gesture::Scrub);
            }
            edited(s, "");
        }
        Phase::Move => match s.edit.timeline.gesture {
            Some(Gesture::Range) => update_range(s, t),
            _ => s.scrub_to(t),
        },
        Phase::Up => {
            if drag.cancelled() {
                if s.edit.timeline.gesture == Some(Gesture::Range) {
                    cancel_range(s);
                } else {
                    s.end_scrub();
                }
            } else if s.edit.timeline.gesture == Some(Gesture::Range) {
                finish_range(s, t);
            } else {
                s.scrub_to(t);
                s.end_scrub();
            }
            s.edit.timeline.gesture = None;
            edited(s, "");
        }
    }
}

/// `clipBlock.select()`.
fn select_clip(s: &mut Preview, id: &str) {
    clear_range(s);
    s.document.set_selected_clip_id(id);
    edited(s, "");
}

/// The clip body and badge: `local_x` is where the element sits in the block.
fn clip_body_drag(s: &mut Preview, drag: &Drag, tx: &mut Tx, id: &str, index: usize, local_x: f32, badge: bool) {
    let px = pps(s);
    let block_x = output_start(s, index) as f32 * px;
    let content_x = block_x + local_x + drag.start_x + drag.dx;
    let t = time_at(s, content_x);
    let dragged = drag.dx.abs() >= 3.0;
    match drag.phase {
        Phase::Down if !shift(tx) && ctrl(tx) => {
            // beginBodyDrag
            clear_range(s);
            let was_selected = s.document.selected_clip_id() == id;
            s.document.set_selected_clip_id(id);
            let Some(clip) = s.document.project().clips.get(index).cloned() else { return };
            s.document.begin_coalesced_edit(&format!("move-clip-{id}"));
            let w = ((clip.out - clip.in_) / clip.speed) as f32 * px;
            s.edit.timeline.gesture = Some(Gesture::ClipMove {
                id: id.to_owned(),
                press_block_x: block_x,
                original_in: clip.in_,
                speed: clip.speed,
                x: block_x,
                w,
                dragged: false,
                was_selected,
            });
            edited(s, "");
        }
        Phase::Move if matches!(s.edit.timeline.gesture, Some(Gesture::ClipMove { .. })) => {
            if let Some(Gesture::ClipMove { press_block_x, x, dragged: d, .. }) = &mut s.edit.timeline.gesture {
                *x = (*press_block_x + drag.dx).max(0.0);
                *d |= dragged;
            }
        }
        Phase::Up if matches!(s.edit.timeline.gesture, Some(Gesture::ClipMove { .. })) => {
            if let Some(Gesture::ClipMove { id, press_block_x, original_in, speed, x, was_selected, dragged: d, .. }) = s.edit.timeline.gesture.take() {
                let d = d || dragged;
                if !drag.cancelled() && d {
                    let next_in = original_in + f64::from((x - press_block_x) / px) * speed;
                    if s.document.move_clip(&id, next_in) {
                        edited(s, "Move clip");
                    }
                } else if !drag.cancelled() && was_selected {
                    s.document.set_selected_clip_id("");
                }
                s.document.end_coalesced_edit();
                edited(s, "");
            }
        }
        Phase::Up => {
            let range = s.edit.timeline.gesture == Some(Gesture::Range);
            track_scrub(s, drag, tx, t, false);
            if !drag.cancelled() && !range && !dragged && !tx.mods().shift {
                select_clip(s, id);
                if badge && !ctrl(tx) {
                    s.edit.timeline.popup = Some(Popup::Speed(id.to_owned()));
                }
            }
        }
        _ => track_scrub(s, drag, tx, t, false),
    }
}

fn clip_trim_drag(s: &mut Preview, drag: &Drag, id: &str, index: usize, edge: Edge) {
    let px = pps(s);
    let Some(clip) = s.document.project().clips.get(index).cloned() else { return };
    match drag.phase {
        Phase::Down => {
            s.document.set_selected_clip_id(id);
            clear_range(s);
            s.pause();
            let block_x = output_start(s, index) as f32 * px;
            let w = ((clip.out - clip.in_) / clip.speed) as f32 * px;
            let original = if edge == Edge::Left { clip.in_ } else { clip.out };
            s.document.begin_coalesced_edit(&format!("trim-{id}"));
            s.edit.timeline.gesture = Some(Gesture::ClipTrim { id: id.to_owned(), edge, press_block_x: block_x, press_w: w, original, preview: original, x: block_x, w });
            edited(s, "");
        }
        Phase::Move => {
            let (lower, upper) = clip_bounds(s, index);
            if let Some(Gesture::ClipTrim { press_block_x, press_w, original, preview, x, w, .. }) = &mut s.edit.timeline.gesture {
                let delta = f64::from(drag.dx / px) * clip.speed;
                if edge == Edge::Left {
                    *preview = (*original + delta).min(clip.out - 0.1).max(lower);
                    let actual = ((*preview - *original) / clip.speed) as f32 * px;
                    *x = *press_block_x + actual;
                    *w = *press_w - actual;
                } else {
                    *preview = (*original + delta).min(upper).max(clip.in_ + 0.1);
                    *x = *press_block_x;
                    *w = *press_w + ((*preview - *original) / clip.speed) as f32 * px;
                }
            }
        }
        Phase::Up => {
            if let Some(Gesture::ClipTrim { preview, .. }) = s.edit.timeline.gesture.take() {
                if !drag.cancelled() {
                    let (a, b) = if edge == Edge::Left { (preview, clip.out) } else { (clip.in_, preview) };
                    if s.document.trim_clip(id, a, b) {
                        edited(s, "Trim clip");
                    }
                }
                s.document.end_coalesced_edit();
                edited(s, "");
            }
        }
    }
}

/// The clip's badge (`clipBadge`): its name and speed over the filmstrip.
fn badge(s: &Preview, ui: &Ui, clip: &omareel_core::project::Clip, index: usize, w: f32) -> El {
    let key = format!("clip-badge-{}", clip.id);
    let hover = hovered(s, &key);
    let trimming = matches!(&s.edit.timeline.gesture, Some(Gesture::ClipTrim { id, .. }) if *id == clip.id);
    let missing = clip.is_media() && !s.document.inserts().get(&clip.media).is_some_and(|i| i.duration > 0.0);
    let name = if clip.is_media() {
        let file = std::path::Path::new(&clip.media).file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
        if missing { format!("Missing · {file}") } else { file }
    } else {
        match clip.layout.as_str() {
            "camera" => "Camera".into(),
            "screen" => "Screen".into(),
            _ => "Clip".into(),
        }
    };
    let speed = badge_speed(clip.speed);
    let speed_w = speed.chars().count() as f32 * 6.6;
    let name_w = (w - 32.0 - speed_w - if hover { 34.0 } else { 6.0 }).max(0.0);
    let mut content = row()
        .gap(6.0)
        .items_center()
        .child(text(name).font(label(12.0, 500.0)).color(if missing { ui.record } else { ui.text_strong }).truncate().max_w(name_w))
        .child(text(speed).font(mono(11.0, 400.0)).color(if clip.speed != 1.0 { ui.accent } else { ui.text_muted }));
    if hover {
        content = content.child(icon(&icons::CHEVRON_DOWN, 11.0, ui.text_muted));
    }
    let id = clip.id.clone();
    let b = row()
        .key(key.clone())
        .role("button")
        .label("Clip speed")
        .h(24.0)
        .max_w(w - 16.0)
        .px(8.0)
        .center()
        .radius(small_radius(ui))
        .fill(alpha(ui.canvas, if hover { 0.9 } else { 0.7 }))
        .opacity(if trimming { 0.6 } else { 1.0 })
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            // The badge is centred; its left edge in the block is near enough the body's for
            // the time under the pointer (a badge press only scrubs when dragged).
            let px = pps(s);
            let block_w = s.document.project().clips.get(index).map_or(0.0, |c| ((c.out - c.in_) / c.speed) as f32 * px);
            let local = (block_w - drag.w) / 2.0;
            clip_body_drag(s, drag, tx, &id, index, local, true);
            wake(tx);
        })
        .child(content);
    row().absolute(0.0, 0.0).size(w, BLOCK_H).center().child(hover_key(b, key))
}

// ---------------------------------------------------------------------------------------------
// Zooms

fn zoom_track(s: &Preview, ui: &Ui, px: f32, cw: f32) -> El {
    let mut track = el()
        .key("zoom-track")
        .absolute(0.0, ZOOM_Y)
        .size(cw, BLOCK_H)
        .on_drag(|s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            let width = drag.w;
            let x = (drag.start_x + drag.dx).clamp(0.0, width);
            match drag.phase {
                Phase::Down => {
                    let mut added = false;
                    if drag.clicks == 2 {
                        // onDoubleClicked: a two-second zoom there.
                        let t = f64::from(drag.start_x / pps(s));
                        added = !s.document.add_zoom_at(t, 2.0).is_empty();
                        if added {
                            edited(s, "Add zoom");
                        }
                    }
                    s.edit.timeline.gesture = Some(Gesture::Marquee { origin: drag.start_x, current: drag.start_x, added });
                }
                Phase::Move => {
                    if let Some(Gesture::Marquee { current, .. }) = &mut s.edit.timeline.gesture {
                        *current = x;
                    }
                }
                Phase::Up => {
                    if let Some(Gesture::Marquee { origin, added, .. }) = s.edit.timeline.gesture.take() {
                        if !drag.cancelled() {
                            if (x - origin).abs() >= 2.0 {
                                let px = pps(s);
                                s.document.select_zooms_in_output_range(f64::from(origin.min(x) / px), f64::from(origin.max(x) / px));
                            } else if !added {
                                // The release ending a double-click keeps the new zoom selected.
                                s.document.set_selected_zoom_ids(&[]);
                            }
                        }
                        edited(s, "");
                    }
                }
            }
            wake(tx);
        });
    if let Some(Gesture::Marquee { origin, current, .. }) = &s.edit.timeline.gesture
        && (current - origin).abs() >= 2.0
    {
        track = track.child(
            el().key("zoom-marquee")
                .absolute(origin.min(*current), 1.0)
                .size((current - origin).abs(), BLOCK_H - 2.0)
                .radius(control_radius(ui))
                .fill(alpha(ui.zoom_accent, 0.14))
                .border(1.0, alpha(ui.zoom_accent, 0.8))
                .inert(),
        );
    }
    let selected: Vec<String> = s.document.selected_zoom_ids().to_vec();
    let zooms = s.document.project().zooms.clone();
    for zoom in &zooms {
        let (start, end) = s.document.source_range_to_output(zoom.start, zoom.end);
        if start < 0.0 || end <= start {
            continue;
        }
        let gesture = match &s.edit.timeline.gesture {
            Some(g @ (Gesture::ZoomMove { id, .. } | Gesture::ZoomResize { id, .. })) if *id == zoom.id => Some(g.clone()),
            _ => None,
        };
        let rest_x = start.max(0.0) as f32 * px;
        let rest_w = ((end - start) as f32 * px).max(18.0);
        let (x, w) = match &gesture {
            Some(Gesture::ZoomMove { x, w, .. } | Gesture::ZoomResize { x, w, .. }) => (*x, *w),
            _ => (rest_x, rest_w),
        };
        let is_selected = selected.contains(&zoom.id);
        let key = format!("zoom-{}", zoom.id);
        let hover = hovered(s, &key) || s.edit.timeline.hover.as_deref().is_some_and(|h| h.starts_with(&format!("zoom-trim-{}-", zoom.id)));
        let resizing = matches!(gesture, Some(Gesture::ZoomResize { .. }));
        let mut block = el()
            .key(key.clone())
            .absolute(x, 0.0)
            .size(w, BLOCK_H)
            .radius(control_radius(ui))
            .fill(alpha(ui.zoom_accent, if is_selected { 0.30 } else if hover { 0.24 } else { 0.18 }))
            .border(if is_selected { 1.5 } else { 1.0 }, if is_selected { ui.zoom_accent } else { alpha(ui.zoom_accent, if hover { 0.7 } else { 0.42 }) });
        if w >= 72.0 {
            let manual = !zoom.automatic_target;
            block = block.child(
                row()
                    .absolute(18.0, 0.0)
                    .size(w - 36.0, BLOCK_H)
                    .gap(5.0)
                    .items_center()
                    .justify_center()
                    .opacity(if resizing { 0.6 } else { 1.0 })
                    .inert()
                    .child(icon(&icons::ZOOM_IN, 13.0, ui.zoom_accent))
                    .child(text(format!("{:.1}×", zoom.level)).font(mono(11.0, 500.0)).color(ui.text_strong))
                    .child(text(if manual { "Manual" } else { "Auto" }).font(label(12.0, 400.0)).color(ui.text_muted).truncate().grow(1.0).min_w(0.0)),
            );
        }
        let id = zoom.id.clone();
        block = block.child(
            el().key(format!("zoom-body-{}", zoom.id))
                .absolute(HANDLE_W, 0.0)
                .size((w - 2.0 * HANDLE_W).max(0.0), BLOCK_H)
                .cursor(if matches!(gesture, Some(Gesture::ZoomMove { .. })) { Cursor::Grabbing } else { Cursor::Grab })
                .on_drag(move |s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
                    zoom_move_drag(s, drag, &id);
                    wake(tx);
                }),
        );
        for edge in [Edge::Left, Edge::Right] {
            let hkey = format!("zoom-trim-{}-{}", zoom.id, if edge == Edge::Left { "left" } else { "right" });
            let engaged = matches!(&gesture, Some(Gesture::ZoomResize { edge: e, .. }) if *e == edge);
            let id = zoom.id.clone();
            block = block.child(trim_handle(s, ui, &hkey, edge, w, BLOCK_H, ui.zoom_accent, is_selected || hover, engaged).on_drag(
                move |s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
                    zoom_resize_drag(s, drag, &id, edge);
                    wake(tx);
                },
            ));
        }
        track = track.child(hover_key(block, key));
    }
    track
}

/// The zoom's block as drawn at rest: content x and width.
fn zoom_rect(s: &Preview, id: &str) -> Option<(f32, f32, f64, f64)> {
    let zoom = s.document.project().zooms.iter().find(|z| z.id == id)?;
    let (start, end) = s.document.source_range_to_output(zoom.start, zoom.end);
    let px = pps(s);
    Some((start.max(0.0) as f32 * px, ((end - start) as f32 * px).max(18.0), zoom.start, zoom.end))
}

fn zoom_move_drag(s: &mut Preview, drag: &Drag, id: &str) {
    let px = pps(s);
    match drag.phase {
        Phase::Down => {
            if !s.document.selected_zoom_ids().iter().any(|z| z == id) {
                s.document.set_selected_zoom_id(id);
            }
            let Some((x, w, ..)) = zoom_rect(s, id) else { return };
            s.document.begin_coalesced_edit("move-selection");
            s.edit.timeline.gesture = Some(Gesture::ZoomMove { id: id.to_owned(), press_block_x: x, x, w });
            edited(s, "");
        }
        Phase::Move => {
            let limit = s.document.duration() as f32 * px;
            if let Some(Gesture::ZoomMove { press_block_x, x, w, .. }) = &mut s.edit.timeline.gesture {
                *x = (*press_block_x + drag.dx).min(limit - *w).max(0.0);
            }
        }
        Phase::Up => {
            if let Some(Gesture::ZoomMove { x, press_block_x, .. }) = s.edit.timeline.gesture.take() {
                if !drag.cancelled() {
                    let start = s.document.output_to_source(f64::from(x / px));
                    if s.document.move_selected_zooms(id, start) && (x - press_block_x).abs() > 0.5 {
                        edited(s, "Move zoom");
                    }
                }
                s.document.end_coalesced_edit();
                edited(s, "");
            }
        }
    }
}

fn zoom_resize_drag(s: &mut Preview, drag: &Drag, id: &str, edge: Edge) {
    let px = pps(s);
    match drag.phase {
        Phase::Down => {
            s.document.set_selected_zoom_id(id);
            let Some((x, w, ..)) = zoom_rect(s, id) else { return };
            s.document.begin_coalesced_edit(&format!("resize-{id}"));
            s.edit.timeline.gesture = Some(Gesture::ZoomResize { id: id.to_owned(), edge, press_block_x: x, press_right: x + w, press_w: w, x, w });
            edited(s, "");
        }
        Phase::Move => {
            let limit = s.document.duration() as f32 * px;
            if let Some(Gesture::ZoomResize { press_block_x, press_right, press_w, x, w, .. }) = &mut s.edit.timeline.gesture {
                if edge == Edge::Left {
                    let next = (*press_block_x + drag.dx).min(*press_right - px).max(0.0);
                    *x = next;
                    *w = *press_right - next;
                } else {
                    *w = (*press_w + drag.dx).min(limit - *x).max(px);
                }
            }
        }
        Phase::Up => {
            if let Some(Gesture::ZoomResize { x, w, .. }) = s.edit.timeline.gesture.take() {
                if !drag.cancelled()
                    && let Some((_, _, start, end)) = zoom_rect(s, id)
                {
                    let (a, b) = if edge == Edge::Left {
                        (s.document.output_to_source(f64::from(x / px)), end)
                    } else {
                        (start, s.document.output_to_source(f64::from((x + w) / px)))
                    };
                    if s.document.resize_zoom(id, a, b) {
                        edited(s, "Resize zoom");
                    }
                }
                s.document.end_coalesced_edit();
                edited(s, "");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Camera row

fn camera_row(s: &Preview, ui: &Ui, px: f32) -> El {
    let hover = hovered(s, "camera-block");
    let w = (s.document.duration() as f32 * px).max(20.0);
    let mut strip = row().absolute(3.0, 3.0).size(w - 6.0, BLOCK_H - 6.0).gap(2.0).clip_soft(0.0).inert();
    let thumbs = &s.edit.timeline.thumbs.camera;
    if thumbs.is_empty() {
        for i in 0..8 {
            strip = strip.child(el().w((w - 6.0 - 14.0) / 8.0).h_full().fill(if i % 2 == 1 { ui.fill } else { ui.fill_hover }));
        }
    } else {
        let tile_w = (w - 6.0 - 2.0 * (thumbs.len().saturating_sub(1)) as f32) / thumbs.len() as f32;
        for (i, path) in thumbs.iter().enumerate() {
            let image = s.edit.timeline.thumbs.image(&s.cache, path);
            strip = strip.child(
                el().key(format!("camera-thumb-{i}"))
                    .w(tile_w)
                    .h_full()
                    .pixels_fit(image.pixels(), SurfaceFit::Fill)
                    .child(el().w_full().h_full().fill(alpha(ui.accent, if hover { 0.12 } else { 0.0 }))),
            );
        }
    }
    let block = el()
        .key("camera-block")
        .role("button")
        .label("Camera settings")
        .help("Camera settings")
        .absolute(0.0, if hover { CAMERA_Y - 1.0 } else { CAMERA_Y })
        .size(w, BLOCK_H)
        .radius(control_radius(ui))
        .fill(ui.fill)
        .border(1.0, if hover { alpha(ui.accent, 0.8) } else { ui.stroke })
        .clip_rounded(control_radius(ui))
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut Preview| {
            s.edit.timeline.camera_selected = true;
        })
        .child(strip)
        .child(
            row()
                .absolute(8.0, (BLOCK_H - 22.0) / 2.0)
                .h(22.0)
                .px(8.0)
                .items_center()
                .radius(small_radius(ui))
                .fill(alpha(ui.canvas, 0.78))
                .inert()
                .child(text("Camera").font(label(11.0, 600.0)).color(ui.text_strong)),
        );
    hover_key(block, "camera-block".into())
}

// ---------------------------------------------------------------------------------------------
// The playhead

fn playhead(s: &Preview, ui: &Ui, px: f32, has_camera: bool) -> El {
    let tracks = if has_camera { 3.0 } else { 2.0 };
    let bottom = RULER_H + tracks * 48.0 + (tracks - 1.0) * 8.0;
    let dragging = hovered(s, "playhead-drag");
    let hover = hovered(s, "playhead-grip");
    let w = if dragging { 2.0 } else { 1.5 };
    let x = s.document.output_position() as f32 * px;
    let corner = if s.ui.radius > 0.0 { 2.5 } else { 0.0 };
    let mut tag = Shape::new();
    tag = tag.fill(vec![[corner, 0.0], [11.0 - corner, 0.0], [11.0, corner], [11.0, 9.0], [5.5, 14.0], [0.0, 9.0], [0.0, corner]], ui.record);
    let head_scale = if dragging { 1.2 } else if hover { 1.12 } else { 1.0 };
    let mut line = el()
        .key("playhead")
        .absolute(x - w / 2.0, 22.0)
        .size(w, bottom - 22.0 + 4.0)
        .fill(ui.record)
        .child(el().absolute(w / 2.0 - 5.5, -6.0).size(11.0, 14.0).shape(tag).scale(head_scale).origin(0.5, 1.0).inert());
    if dragging {
        line = line.child(el().absolute(w / 2.0 - 11.5, -11.0).size(23.0, 24.0).radius(8.0).fill(alpha(ui.record, 0.25)).inert());
    }
    if hover || dragging {
        line = line.child(
            row()
                .key("playhead-time")
                .absolute(10.0, -9.0)
                .h(18.0)
                .px(6.0)
                .items_center()
                .radius(small_radius(ui))
                .fill(ui.record)
                .inert()
                .transition(rune::Transition::opacity())
                .child(text(omareel_core::editor::format_time(s.document.output_position())).font(mono(11.0, 500.0)).color(Color::WHITE)),
        );
    }
    let grip = el()
        .key("playhead-grip")
        .absolute(w / 2.0 - 10.5, -8.0)
        .size(21.0, 22.0)
        .cursor(Cursor::ResizeColumn)
        .on_drag(|s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            let px = pps(s);
            match drag.phase {
                Phase::Down => {
                    s.edit.timeline.hover = Some("playhead-drag".into());
                    s.begin_scrub();
                    s.edit.timeline.gesture = Some(Gesture::Scrub);
                    let x = s.document.output_position() as f32 * px;
                    drag.data = x - 10.5 + drag.start_x;
                }
                Phase::Move => {
                    let t = f64::from((drag.data + drag.dx) / px);
                    s.scrub_to(t.clamp(0.0, s.document.duration()));
                }
                Phase::Up => {
                    let t = f64::from((drag.data + drag.dx) / px);
                    if !drag.cancelled() {
                        s.scrub_to(t.clamp(0.0, s.document.duration()));
                    }
                    s.end_scrub();
                    s.edit.timeline.gesture = None;
                    s.edit.timeline.hover = None;
                }
            }
            wake(tx);
        });
    line.child(hover_key(grip, "playhead-grip".into()))
}
