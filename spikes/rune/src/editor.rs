//! A slice of the editor in Rune, to see what the interface could become: the preview with an
//! inspector and a timeline, over omareel_core's `Document` (every edit is the Document's own:
//! `set_project_value`, `move_zoom`, `split_at_playhead`, `undo`, ...).
//!
//! What it tries that the QML does not do today:
//! - **Edits spring.** Paused, a change in the inspector (camera shape, padding, corner radius,
//!   size) moves the preview to its new layout on a spring instead of jumping, because the
//!   composition is an ordinary Rune view diffed against the last frame. Playing, it lands at once.
//! - **The timeline moves physically.** Zoom blocks drag with the pointer and their neighbours
//!   and the selection follow on springs; the selection ring flies between items
//!   (`El::matched`); the playhead glides on seeks.
//! - **It can be checked without a compositor.** `editor <bundle> -- --snap out.png`,
//!   `-- --script s.json --video out.mp4` and `-- --tree tree.json` render, record and dump the
//!   same UI headless (Rune's agent tooling), with the real video frames.
//!
//! The project is never saved: edits stay in memory.

use crate::preview::{Preview, play_glyph, tenths};
use crate::theme::Ui;
use omareel_core::compat::qjson::Value;
use omareel_core::editor::Effect;
use rune::tok::{Family, TextStyle};
use rune::{Color, Cursor, Cx, El, Key, KeyEvent, Phase, Spring, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, PaletteSeed, SegmentedControl, Switch};
use std::time::Duration;

/// The editor's transient state (what is being dragged, which section is open).
#[derive(Default)]
pub struct EditState {
    /// A zoom being dragged: its id, its source start when the drag began, and the timeline's
    /// points per second (handlers have no layout; the block's own width gives it).
    zoom_drag: Option<(String, f64, f32)>,
    /// Seconds of the last undoable edit, for the toast.
    toast: Option<(String, f64)>,
}

pub fn kit(ui: &Ui) -> Kit {
    let mut k = Kit::from_palette(PaletteSeed { bg: ui.surface, fg: ui.text, accent: ui.accent });
    k.radius.sm = ui.radius.min(6.0);
    k
}

const INSPECTOR_W: f32 = 320.0;
const TRACK_H: f32 = 34.0;
const HEADER_W: f32 = 84.0;

fn mono(size: f32, weight: f32) -> TextStyle {
    TextStyle { family: Family::SystemMono, ..TextStyle::new(size, weight) }
}

fn label(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

// ---------------------------------------------------------------------------------------------
// Edits

/// After any edit: the settings the composition reads, the motion track, the toast.
fn edited(s: &mut Preview, what: &str) {
    for effect in s.document.take_effects() {
        if matches!(effect, Effect::StartMotionTimer)
            && s.motion.is_none()
            && let Some(job) = s.document.motion_job()
        {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(job.run());
            });
            s.motion = Some(receive);
        }
    }
    let camera_available = s.settings.camera_available;
    s.settings = crate::comp::Settings::new(
        s.document.project(),
        (s.document.source_width(), s.document.source_height()),
        s.document.capture_scale(),
        s.document.has_camera().then(|| (s.document.camera_source_width(), s.document.camera_source_height())),
    );
    s.settings.camera_available = camera_available;
    if !what.is_empty() {
        s.edit.toast = Some((what.to_owned(), s.now));
    }
}

fn set(s: &mut Preview, path: &str, value: Value, coalesce: bool, what: &str) {
    s.document.set_project_value(path, value, coalesce);
    edited(s, what);
}

/// Called from the preview's poll: a finished motion track lands.
pub fn poll(s: &mut Preview) -> bool {
    let Some(receive) = &s.motion else { return false };
    match receive.try_recv() {
        Ok(track) => {
            s.motion = None;
            s.document.apply_motion(track);
            edited(s, "");
            true
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            s.motion = None;
            false
        }
    }
}

pub fn on_key(s: &mut Preview, key: &KeyEvent, tx: &mut Tx) -> bool {
    match &key.key {
        Key::Command('z') if key.mods.shift => {
            s.document.redo();
            edited(s, "Redo");
            true
        }
        Key::Command('z') => {
            s.document.undo();
            edited(s, "Undo");
            true
        }
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

// ---------------------------------------------------------------------------------------------
// The window

pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let middle = row()
        .key("middle")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(crate::preview::pane(s, cx, 64.0, 48.0))
        .child(inspector(s, ui));
    col()
        .w_full()
        .h_full()
        .fill(ui.canvas)
        .child(top_bar(s, ui))
        .child(middle)
        .child(timeline(s, cx, ui))
        .child(toast(s, ui))
}

fn top_bar(s: &Preview, ui: &Ui) -> El {
    let k = &s.kit;
    let name = s.document.bundle_name().to_owned();
    let dot = el().size(10.0, 10.0).radius(5.0).fill(ui.record);
    let history = row()
        .gap(6.0)
        .child(
            Button::new("Undo")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Preview| {
                    s.document.undo();
                    edited(s, "Undo");
                })
                .build(k),
        )
        .child(
            Button::new("Redo")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut Preview| {
                    s.document.redo();
                    edited(s, "Redo");
                })
                .build(k),
        );
    row()
        .key("top-bar")
        .w_full()
        .h(48.0)
        .px(16.0)
        .gap(12.0)
        .items_center()
        .fill(ui.surface)
        .child(dot)
        .child(text(name).font(label(14.0, 600.0)).color(ui.text_strong))
        .child(text(format!("{}×{}", s.document.output_width(), s.document.output_height())).font(mono(12.0, 500.0)).color(ui.text_faint))
        .child(el().grow(1.0))
        .child(history)
        .child(Button::new("Export").variant(ButtonVariant::Contrast).on_press(|s: &mut Preview| edited(s, "Export is the spike's `export` command")).build(k))
}

/// The last edit, in a pill that springs up from the bottom and fades after two seconds.
fn toast(s: &Preview, ui: &Ui) -> El {
    let showing = s.edit.toast.as_ref().filter(|(_, at)| s.now - at < 2.0);
    let mut layer = col().key("toast-layer").absolute(0.0, 0.0).w_full().h_full().inert().items_center().justify_end().pb(250.0);
    if let Some((what, at)) = showing {
        layer = layer.child(
            row()
                .key(format!("toast-{at}"))
                .h(32.0)
                .px(14.0)
                .items_center()
                .radius(16.0)
                .fill(ui.raised)
                .border(1.0, ui.hairline)
                .shadow(6.0, 18.0, ui.shadow)
                .transition(rune::Transition::offset(0.0, 12.0).combined(rune::Transition::opacity()))
                .child(text(what.clone()).font(label(13.0, 500.0)).color(ui.text_strong)),
        );
    }
    layer
}

// ---------------------------------------------------------------------------------------------
// The inspector

fn inspector(s: &Preview, ui: &Ui) -> El {
    let k = &s.kit;
    let p = s.document.project();
    let shape = match p.camera.shape.as_str() {
        "square" => 1,
        "rectangle" => 2,
        _ => 0,
    };
    let frame = section(ui, "Frame")
        .child(slider(ui, "padding", "Padding", p.frame.padding as f32, 0.0, 0.25, |v| format!("{:.0}%", v * 100.0), |s, v, coalesce| {
            set(s, "frame.padding", f64::from(v).into(), coalesce, "")
        }))
        .child(slider(ui, "radius", "Corner radius", p.frame.radius as f32, 0.0, 48.0, |v| format!("{v:.0} px"), |s, v, coalesce| {
            set(s, "frame.radius", f64::from(v).into(), coalesce, "")
        }))
        .child(switch_row(k, ui, "Shadow", p.frame.shadow.enabled, |s: &mut Preview| {
            let on = !s.document.project().frame.shadow.enabled;
            set(s, "frame.shadow.enabled", on.into(), false, if on { "Shadow on" } else { "Shadow off" });
        }));
    let camera = section(ui, "Camera")
        .child(el().w_full().child(
            SegmentedControl::new(&["Round", "Square", "Rectangle"], Some(shape))
                .label("Camera shape")
                .width(None)
                .on_pick(|s: &mut Preview, i: usize, _: &mut Tx| {
                    let shape = ["round", "square", "rectangle"][i.min(2)];
                    set(s, "camera.shape", shape.into(), false, "Camera shape");
                })
                .build(k)
                .key("camera-shape"),
        ))
        .child(slider(ui, "camera-size", "Size", p.camera.size as f32, 0.1, 0.5, |v| format!("{:.0}%", v * 100.0), |s, v, coalesce| {
            set(s, "camera.size", f64::from(v).into(), coalesce, "")
        }))
        .child(el().w_full().child(
            SegmentedControl::new(&["Fill", "Square + blur"], Some(usize::from(p.camera.full_frame == "square")))
                .label("Full-frame layout")
                .width(None)
                .on_pick(|s: &mut Preview, i: usize, _: &mut Tx| {
                    set(s, "camera.fullFrame", (if i == 1 { "square" } else { "fill" }).into(), false, "Full-frame layout");
                })
                .build(k)
                .key("full-frame"),
        ))
        .child(switch_row(k, ui, "Border", p.camera.border.enabled, |s: &mut Preview| {
            let on = !s.document.project().camera.border.enabled;
            set(s, "camera.border.enabled", on.into(), false, "");
        }));
    let selected = s.document.selected_zoom_id().to_owned();
    let zoom = s.document.project().zooms.iter().find(|z| z.id == selected);
    let mut zoom_section = section(ui, "Zoom");
    zoom_section = match zoom {
        Some(z) => zoom_section
            .child(slider(ui, "zoom-level", "Level", z.level as f32, 1.0, 4.0, |v| format!("{v:.1}×"), |s, v, coalesce| {
                let id = s.document.selected_zoom_id().to_owned();
                if s.document.set_zoom_level(&id, f64::from(v), coalesce) {
                    edited(s, "");
                }
            }))
            .child(Button::new("Delete zoom").variant(ButtonVariant::Outline).on_press(|s: &mut Preview| {
                let id = s.document.selected_zoom_id().to_owned();
                if s.document.remove_zoom(&id) {
                    edited(s, "Delete zoom");
                }
            }).build(k)),
        None => zoom_section.child(text("Select a zoom on the timeline").font(label(13.0, 400.0)).color(ui.text_faint)),
    };
    let keys = section(ui, "Keystrokes").child(switch_row(k, ui, "Show shortcuts", p.keystrokes.enabled, |s: &mut Preview| {
        let on = !s.document.project().keystrokes.enabled;
        set(s, "keystrokes.enabled", on.into(), false, if on { "Keystrokes on" } else { "Keystrokes off" });
    }));
    col()
        .key("inspector")
        .w(INSPECTOR_W)
        .h_full()
        .fill(ui.surface)
        .px(16.0)
        .py(12.0)
        .gap(10.0)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(frame)
        .child(camera)
        .child(zoom_section)
        .child(keys)
}

fn section(ui: &Ui, title: &str) -> El {
    col()
        .key(format!("section-{title}"))
        .w_full()
        .gap(12.0)
        .padding(14.0)
        .radius(ui.radius.clamp(6.0, 12.0))
        .fill(ui.card)
        .child(text(title.to_owned()).font(label(12.0, 600.0)).color(ui.text_muted))
}

fn switch_row(k: &Kit, ui: &Ui, name: &str, on: bool, toggle: impl Fn(&mut Preview) + 'static) -> El {
    row()
        .w_full()
        .items_center()
        .child(text(name.to_owned()).font(label(13.0, 500.0)).color(ui.text))
        .child(el().grow(1.0))
        .child(Switch::new(on).on_toggle(toggle).build(k).label(name.to_owned()))
}

/// A labelled slider. Drags land at once (direct manipulation, one coalesced undo step); a click
/// on the track springs the knob there.
#[allow(clippy::too_many_arguments)]
fn slider(
    ui: &Ui,
    key: &str,
    name: &str,
    value: f32,
    min: f32,
    max: f32,
    format: impl Fn(f32) -> String,
    apply: impl Fn(&mut Preview, f32, bool) + 'static,
) -> El {
    let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
    let track = el()
        .key(format!("{key}-track"))
        .role("slider")
        .label(name.to_owned())
        .w_full()
        .h(20.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut Preview, drag, tx: &mut Tx| {
            let v = min + (drag.x / drag.w.max(1.0)).clamp(0.0, 1.0) * (max - min);
            let coalesce = !matches!(drag.phase, Phase::Down);
            if matches!(drag.phase, Phase::Down) {
                // The press springs the knob (and the preview) to where it landed; the drag's
                // moves then follow the pointer at once.
                tx.animated();
            }
            apply(s, v, coalesce);
            if matches!(drag.phase, Phase::Up) {
                s.document.end_coalesced_edit();
            }
            tx.wake_after(Duration::ZERO);
        })
        .child(
            el().w_full()
                .h(4.0)
                .radius(2.0)
                .fill(ui.normal_fill)
                .child(el().key(format!("{key}-fill")).h(4.0).w_frac(t).radius(2.0).fill(ui.accent)),
        )
        .child(
            row()
                .absolute(0.0, 2.0)
                .w_full()
                .child(el().w_frac(t))
                .child(el().key(format!("{key}-knob")).size(16.0, 16.0).ml(-8.0).radius(8.0).fill(Color::WHITE).shadow(1.0, 4.0, Color::BLACK.alpha(0.35))),
        );
    col()
        .key(format!("slider-{key}"))
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .child(text(name.to_owned()).font(label(13.0, 500.0)).color(ui.text))
                .child(el().grow(1.0))
                .child(text(format(value)).font(mono(12.0, 500.0)).color(ui.text_muted)),
        )
        .child(track)
}

// ---------------------------------------------------------------------------------------------
// The timeline

fn timeline(s: &Preview, cx: &mut Cx<Preview>, ui: &Ui) -> El {
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

fn track_label(ui: &Ui, name: &str) -> El {
    row().h(TRACK_H).items_center().child(text(name.to_owned()).font(label(12.0, 600.0)).color(ui.text_muted))
}

fn clip_track(s: &Preview, ui: &Ui, pps: f32) -> El {
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
fn selection_ring(ui: &Ui, w: f32) -> El {
    el().key("selection").matched("selection").absolute(-3.0, -3.0).size(w + 6.0, TRACK_H + 6.0).radius(11.0).border(2.0, ui.text_strong).inert()
}

fn zoom_track(s: &Preview, ui: &Ui, pps: f32) -> El {
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
        let dragging = s.edit.zoom_drag.as_ref().is_some_and(|(d, ..)| *d == zoom.id);
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
                        s.edit.zoom_drag = Some((id.clone(), start, pps));
                        s.document.begin_coalesced_edit("zoom-drag");
                    }
                    Phase::Move => {
                        if let Some((_, from, pps)) = &s.edit.zoom_drag {
                            let to = (from + f64::from(drag.dx / pps.max(1.0))).max(0.0);
                            s.document.move_zoom(&id, to);
                        }
                    }
                    Phase::Up => {
                        s.edit.zoom_drag = None;
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

fn key_track(s: &Preview, ui: &Ui, pps: f32) -> El {
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
