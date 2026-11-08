//! BottomBar.qml: under the preview. Left: the aspect menu and Crop (one canvas menu button
//! when narrower than 900 pt). Middle: the position, go to start, play, go to end and the
//! duration. Right: Insert and Split (icons only under 1100 pt), the timeline's scale (out,
//! slider, in) and the shortcuts sheet's "?" key.

use super::popup::{ASPECTS, aspect};
use super::{Popup, control_radius, open_insert_dialog, set_scale};
use crate::editor::{edited, label, mono};
use crate::icons;
use crate::kit::icon;
use crate::preview::{Preview, tenths};
use crate::theme::Ui;
use rune::{Color, Cursor, Cx, Drag, El, Icon, Phase, Tx, el, row, text};
use std::time::Duration;

fn wake(tx: &mut Tx) {
    tx.wake_after(Duration::ZERO);
}

/// OmIconButton: a quiet square icon button with its tooltip.
fn icon_button(ui: &Ui, key: &str, i: &'static Icon, name: &str, size: f32, icon_size: f32, enabled: bool, press: impl Fn(&mut Preview, &mut Tx) + 'static) -> El {
    let mut b = el()
        .key(key.to_owned())
        .role("button")
        .label(name.to_owned())
        .help(name.to_owned())
        .size(size, size)
        .center()
        .radius(control_radius(ui))
        .child(icon(i, icon_size, if enabled { ui.foreground } else { ui.text_faint }));
    if enabled {
        b = b.hover(ui.fill_hover).press(ui.fill_pressed).cursor(Cursor::Pointer).on_press_tx(move |s: &mut Preview, tx: &mut Tx| {
            press(s, tx);
            wake(tx);
        });
    } else {
        b = b.disabled(true);
    }
    b
}

/// OmButton: icon and text (just the icon when `compact`), selected or not.
#[allow(clippy::too_many_arguments)]
fn tool_button(ui: &Ui, key: &str, i: &'static Icon, text_: &str, tooltip: &str, compact: bool, selected: bool, icon_color: Option<Color>, enabled: bool, press: impl Fn(&mut Preview, &mut Tx) + 'static) -> El {
    let fg = if enabled { ui.foreground } else { ui.text_faint };
    let mut b = row()
        .key(key.to_owned())
        .role("button")
        .label(if text_.is_empty() { tooltip.to_owned() } else { text_.to_owned() })
        .h(32.0)
        .gap(6.0)
        .items_center()
        .justify_center()
        .radius(control_radius(ui))
        .fill(if selected { ui.selected_fill } else { Color::TRANSPARENT })
        .child(icon(i, 16.0, icon_color.unwrap_or(fg)));
    if !tooltip.is_empty() {
        b = b.help(tooltip.to_owned());
    }
    b = if compact { b.w(32.0) } else { b.px(10.0).child(text(text_.to_owned()).font(label(13.0, 500.0)).color(fg)) };
    if enabled {
        b = b.hover(ui.fill_hover).press(ui.fill_pressed).cursor(Cursor::Pointer).on_press_tx(move |s: &mut Preview, tx: &mut Tx| {
            press(s, tx);
            wake(tx);
        });
    } else {
        b = b.disabled(true);
    }
    b
}

fn toggle_popup(s: &mut Preview, popup: Popup) {
    s.edit.timeline.popup = if s.edit.timeline.popup.as_ref() == Some(&popup) { None } else { Some(popup) };
}

/// The timeline's scale changed from the bar: about the middle of the tracks.
fn scale_to(s: &mut Preview, scale: f64) {
    let middle = s.edit.timeline.flick_w.get() / 2.0;
    set_scale(s, scale, middle);
}

pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let width = cx.layout_size("bottom-bar").map_or(1000.0, |[w, _]| w);
    s.edit.timeline.bar_width.set(width);
    let compact = width < 1100.0;
    let narrow = width < 900.0;

    // Left: canvas.
    let mut left = row().gap(6.0).items_center();
    if narrow {
        left = left.child(icon_button(ui, "canvas-options", &icons::SQUARE_DASHED, "Canvas options", 32.0, 18.0, true, |s, _| toggle_popup(s, Popup::Overflow)));
    } else {
        let current = aspect(s);
        let (name, value) = ASPECTS.iter().find(|(_, v)| *v == current).copied().unwrap_or(ASPECTS[0]);
        let shown = if compact { if value == "auto" { "Auto".to_owned() } else { value.to_owned() } } else { name.to_owned() };
        let open = s.edit.timeline.popup == Some(Popup::Aspect);
        let combo = row()
            .key("aspect-menu")
            .role("combobox")
            .label("Aspect")
            .w(if compact { 104.0 } else { 138.0 })
            .h(32.0)
            .px(10.0)
            .gap(6.0)
            .items_center()
            .radius(control_radius(ui))
            .fill(if open { ui.fill_pressed } else { ui.normal_fill })
            .border(1.0, ui.normal_border)
            .hover(ui.fill_hover)
            .cursor(Cursor::Pointer)
            .on_press_tx(|s: &mut Preview, tx: &mut Tx| {
                toggle_popup(s, Popup::Aspect);
                wake(tx);
            })
            .child(icon(&icons::SQUARE_DASHED, 15.0, ui.text_muted))
            .child(text(shown).font(label(13.0, 500.0)).color(ui.foreground).truncate().grow(1.0).min_w(0.0))
            .child(icon(&icons::CHEVRON_DOWN, 13.0, ui.text_muted));
        let crop = s.edit.timeline.crop_mode;
        left = left.child(combo).child(tool_button(
            ui,
            "crop-button",
            if crop { &icons::CHECK } else { &icons::CROP },
            if crop { "Done" } else { "Crop" },
            if crop { "" } else { "Crop the recording" },
            false,
            crop,
            crop.then_some(ui.accent),
            true,
            |s, _| s.edit.timeline.crop_mode = !s.edit.timeline.crop_mode,
        ));
    }
    let left = row().grow(1.0).min_w(if narrow { 36.0 } else if compact { 196.0 } else { 228.0 }).h_full().items_center().child(left);

    // Middle: the transport.
    let time_w = 58.0;
    let playing = s.playing;
    let play_fill = ui.text_strong;
    let play = el()
        .key("play-button")
        .role("button")
        .label(if playing { "Pause" } else { "Play" })
        .help(if playing { "Pause  Space" } else { "Play  Space" })
        .size(36.0, 36.0)
        .mx(2.0)
        .center()
        .radius(if ui.radius > 0.0 { 18.0 } else { 0.0 })
        .fill(play_fill)
        .hover(play_fill.mix(Color::WHITE, 0.08))
        .press_scale(0.92)
        .cursor(Cursor::Pointer)
        .on_press_tx(|s: &mut Preview, tx: &mut Tx| {
            s.toggle();
            wake(tx);
        })
        .child(icon(if playing { &icons::PAUSE } else { &icons::PLAY }, 16.0, ui.surface).ml(if playing { 0.0 } else { 1.0 }));
    let middle = row()
        .key("transport")
        .gap(4.0)
        .items_center()
        .child(row().w(time_w).mr(8.0).justify_end().child(text(tenths(s.document.output_position())).font(mono(12.0, 500.0)).color(ui.text_strong)))
        .child(icon_button(ui, "go-to-start", &icons::SKIP_BACK, "Go to start", 32.0, 18.0, true, |s, _| s.seek_boundary(-1)))
        .child(play)
        .child(icon_button(ui, "go-to-end", &icons::SKIP_FORWARD, "Go to end", 32.0, 18.0, true, |s, _| s.seek_boundary(1)))
        .child(row().w(time_w).ml(8.0).child(text(tenths(s.document.duration())).font(mono(12.0, 400.0)).color(ui.text_faint)));

    // Right: the tools.
    let importing = s.document.importing();
    let scale = s.edit.timeline.scale;
    let slider_w = if compact { 76.0 } else { 96.0 };
    let t = ((scale - 1.0) / 15.0).clamp(0.0, 1.0) as f32;
    let slider = el()
        .key("timeline-zoom-slider")
        .role("slider")
        .label("Timeline zoom")
        .w(slider_w)
        .h(20.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .on_drag(|s: &mut Preview, drag: &mut Drag, tx: &mut Tx| {
            let v = 1.0 + f64::from((drag.x / drag.w.max(1.0)).clamp(0.0, 1.0)) * 15.0;
            scale_to(s, v);
            if matches!(drag.phase, Phase::Up) {
                edited(s, "");
            }
            wake(tx);
        })
        .child(el().w_full().h(4.0).radius(2.0).fill(ui.normal_fill).child(el().h(4.0).w_frac(t).radius(2.0).fill(ui.accent)))
        .child(el().absolute(t * slider_w - 7.0, 3.0).size(14.0, 14.0).radius(7.0).fill(Color::WHITE).shadow(1.0, 4.0, Color::BLACK.alpha(0.35)));
    let sheet_open = s.edit.timeline.popup == Some(Popup::Shortcuts);
    let shortcuts = el()
        .key("shortcuts-button")
        .role("button")
        .label("Keyboard shortcuts")
        .help("Keyboard shortcuts")
        .size(28.0, 28.0)
        .ml(6.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(0.92)
        .on_press_tx(|s: &mut Preview, tx: &mut Tx| {
            toggle_popup(s, Popup::Shortcuts);
            wake(tx);
        })
        .child(keycap(ui, "?", !sheet_open));
    let right = row()
        .gap(4.0)
        .items_center()
        .child(tool_button(ui, "insert-clip", &icons::FILM, if compact { "" } else { "Insert" }, "Insert a video at the playhead  I", compact, false, None, !importing, |s, tx| open_insert_dialog(s, tx)))
        .child(tool_button(ui, "split-clip", &icons::SCISSORS, if compact { "" } else { "Split" }, "Split at playhead  S", compact, false, None, true, |s, _| {
            if s.document.split_at_playhead() {
                edited(s, "Split clip");
            }
        }))
        .child(el().size(1.0, 18.0).mx(6.0).fill(ui.separator))
        .child(icon_button(ui, "timeline-zoom-out", &icons::ZOOM_OUT, "Zoom timeline out", 26.0, 15.0, true, |s, _| {
            let v = (s.edit.timeline.scale / 1.25).max(1.0);
            scale_to(s, v);
        }))
        .child(slider)
        .child(icon_button(ui, "timeline-zoom-in", &icons::ZOOM_IN, "Zoom timeline in", 26.0, 15.0, true, |s, _| {
            let v = (s.edit.timeline.scale * 1.25).min(16.0);
            scale_to(s, v);
        }))
        .child(shortcuts);
    let right = row().grow(1.0).min_w(if compact { 270.0 } else { 372.0 }).h_full().items_center().justify_end().child(right);

    row()
        .key("bottom-bar")
        .w_full()
        .h(52.0)
        .px(12.0)
        .gap(8.0)
        .items_center()
        .fill(ui.surface)
        .child(el().absolute(-12.0, 0.0).w_full().h(1.0).fill(ui.hairline).inert())
        .child(left)
        .child(middle)
        .child(right)
}

/// OmKeycap: the "?" key, quiet until hovered or open.
fn keycap(ui: &Ui, key: &str, subtle: bool) -> El {
    row()
        .h(20.0)
        .min_w(20.0)
        .px(5.0)
        .center()
        .radius(control_radius(ui).min(4.0))
        .fill(if subtle { ui.fill } else { ui.raised })
        .border(1.0, ui.stroke_strong)
        .child(text(key.to_owned()).font(mono(11.0, 500.0)).color(if subtle { ui.text_muted } else { ui.text_strong }))
}
