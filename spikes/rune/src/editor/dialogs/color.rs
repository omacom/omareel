//! The colour dialog the panels' colour wells and Background's custom colour open (QML's
//! `ColorDialog`, which is Qt's own picker): a saturation/value square, a hue strip, the old and
//! new colours side by side, and the hex code. Rune has no colour dialog, so this one is drawn
//! here; the square and the strip are CPU pixels so they show the exact colours picked.

use super::super::inspector::widgets::{self, Btn, act, mono, om};
use super::{Modal, close_modal, open_modal};
use crate::preview::Preview;
use omareel_core::compat::qcolor::QColor;
use rune::{Cursor, El, Phase, Pixels, TextEdit, Tx, col, el, row};
use std::rc::Rc;

pub struct Picker {
    title: String,
    original: QColor,
    /// Hue 0..360, saturation and value 0..1.
    hsv: [f32; 3],
    hex: TextEdit,
    square: Pixels,
    square_hue: f32,
    strip: Pixels,
    accepted: Rc<dyn Fn(&mut Preview, QColor)>,
}

/// Opens the dialog on `color`; `accepted` gets the colour chosen with OK.
pub fn open_color(s: &mut Preview, title: &str, color: &QColor, accepted: impl Fn(&mut Preview, QColor) + 'static) {
    let hsv = rgb_to_hsv(color.red() as f32 / 255.0, color.green() as f32 / 255.0, color.blue() as f32 / 255.0);
    let square = Pixels::new();
    paint_square(&square, hsv[0]);
    let strip = Pixels::new();
    let mut bytes = Vec::with_capacity(360 * 4);
    for x in 0..360 {
        let [r, g, b] = hsv_to_rgb(x as f32, 1.0, 1.0);
        bytes.extend_from_slice(&[to_byte(r), to_byte(g), to_byte(b), 255]);
    }
    strip.set(360, 1, bytes);
    open_modal(
        s,
        Modal::Color(Picker {
            title: title.to_owned(),
            original: color.clone(),
            hsv,
            hex: TextEdit::new(color.name()),
            square,
            square_hue: hsv[0],
            strip,
            accepted: Rc::new(accepted),
        }),
    );
}

fn picker(s: &mut Preview) -> Option<&mut Picker> {
    match &mut s.edit.dialogs.modal {
        Some(Modal::Color(p)) => Some(p),
        _ => None,
    }
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn paint_square(pixels: &Pixels, hue: f32) {
    const N: u32 = 128;
    let mut bytes = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        let v = 1.0 - y as f32 / (N - 1) as f32;
        for x in 0..N {
            let sat = x as f32 / (N - 1) as f32;
            let [r, g, b] = hsv_to_rgb(hue, sat, v);
            bytes.extend_from_slice(&[to_byte(r), to_byte(g), to_byte(b), 255]);
        }
    }
    pixels.set(N, N, bytes);
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.rem_euclid(360.0)) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn current(p: &Picker) -> QColor {
    let [r, g, b] = hsv_to_rgb(p.hsv[0], p.hsv[1], p.hsv[2]);
    QColor::parse(&format!("#{:02x}{:02x}{:02x}", to_byte(r), to_byte(g), to_byte(b)))
}

/// After the square or the strip moved: the square's hue and the hex field follow.
fn moved(p: &mut Picker) {
    if (p.square_hue - p.hsv[0]).abs() > 0.01 {
        paint_square(&p.square, p.hsv[0]);
        p.square_hue = p.hsv[0];
    }
    let name = current(p).name();
    p.hex.replace_text(name);
}

/// The hex field's colour, when it parses.
fn typed(p: &Picker) -> Option<QColor> {
    let t = p.hex.text.trim();
    let t = if t.starts_with('#') { t.to_owned() } else { format!("#{t}") };
    let c = QColor::parse(&t);
    (c.is_valid() && (t.len() == 7 || t.len() == 4)).then_some(c)
}

fn accept(s: &mut Preview) {
    let Some(p) = picker(s) else { return };
    let color = typed(p).unwrap_or_else(|| current(p));
    let accepted = p.accepted.clone();
    close_modal(s);
    accepted(s, color);
}

pub fn view(s: &Preview, p: &Picker) -> El {
    let ui = &s.ui;
    let o = om();
    let [h, sat, v] = p.hsv;
    let width = 360.0 - 2.0 * (o.popup_padding + 1.0);
    let square_h = 180.0;
    let marker = |x: f32, y: f32| {
        el().absolute(x - 7.0, y - 7.0).size(14.0, 14.0).radius(7.0).border(2.0, widgets::WHITE).shadow(0.0, 3.0, widgets::BLACK.alpha(0.5)).inert()
    };
    let square = el()
        .key("color-square")
        .role("slider")
        .label("Saturation and brightness")
        .w_full()
        .h(square_h)
        .radius(o.control_radius)
        .pixels(&p.square)
        .cursor(Cursor::Crosshair)
        .on_drag(|s: &mut Preview, drag, tx: &mut Tx| {
            if let Some(p) = picker(s) {
                p.hsv[1] = (drag.x / drag.w.max(1.0)).clamp(0.0, 1.0);
                p.hsv[2] = 1.0 - (drag.y / drag.h.max(1.0)).clamp(0.0, 1.0);
                moved(p);
            }
            if matches!(drag.phase, Phase::Down) {
                tx.instant();
            }
        })
        .child(marker(sat * width, (1.0 - v) * square_h));
    let strip = el()
        .key("color-hue")
        .role("slider")
        .label("Hue")
        .w_full()
        .h(14.0)
        .radius(7.0)
        .pixels(&p.strip)
        .cursor(Cursor::Pointer)
        .on_drag(|s: &mut Preview, drag, _: &mut Tx| {
            if let Some(p) = picker(s) {
                p.hsv[0] = (drag.x / drag.w.max(1.0)).clamp(0.0, 0.9999) * 360.0;
                moved(p);
            }
        })
        .child(marker(h / 360.0 * width, 7.0));
    let chosen = current(p);
    let swatches = row()
        .h(30.0)
        .w(76.0)
        .radius(o.control_radius)
        .clip_rounded(o.control_radius)
        .border(1.0, ui.stroke)
        .child(el().grow(1.0).h_full().fill(widgets::qcolor(&p.original)).help("Current"))
        .child(el().key("color-new").grow(1.0).h_full().fill(widgets::qcolor(&chosen)).help("New"));
    let hex = widgets::text_field(
        ui,
        "color-hex",
        &p.hex,
        "#rrggbb",
        |s: &mut Preview| match &mut s.edit.dialogs.modal {
            Some(Modal::Color(p)) => &mut p.hex,
            _ => unreachable!("the hex field lives in the colour dialog"),
        },
        |s: &mut Preview, _| {
            if let Some(p) = picker(s)
                && let Some(c) = typed(p)
            {
                p.hsv = rgb_to_hsv(c.red() as f32 / 255.0, c.green() as f32 / 255.0, c.blue() as f32 / 255.0);
                moved(p);
            }
        },
    )
    .font(mono(o.body, 400.0));
    col()
        .key("color-dialog")
        .w_full()
        .gap(14.0)
        .child(super::dialog_title(ui, &p.title, ""))
        .child(square)
        .child(strip)
        .child(row().w_full().gap(10.0).items_center().child(swatches).child(el().grow(1.0).child(hex)))
        .child(
            row()
                .w_full()
                .pt(4.0)
                .gap(8.0)
                .child(el().grow(1.0))
                .child(widgets::button(ui, "color-cancel", "Cancel", None, Btn::bordered(), act(|s, _| close_modal(s))))
                .child(widgets::button(ui, "color-ok", "OK", None, Btn { width: Some(72.0), ..Btn::primary() }, act(|s, _| accept(s)))),
        )
}
