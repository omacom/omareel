//! The shared widgets every window uses (launcher, recording bar, editor), on top of rune-kit:
//! the Om* components of the QML (`src/ui/qml/ui/`) where rune-kit has no equivalent. Generic
//! over the window's state `T`, so handlers get `&mut T`.
//!
//! Text styles: `label(size, weight)` is the interface face (Inter), `mono(size, weight)` the
//! Omarchy monospace for times, numbers and keys.

use crate::theme::Ui;
use rune::tok::{Family, TextStyle};
use rune::{Color, Cursor, El, Icon, Phase, Tx, col, el, row, text};
use std::time::Duration;

pub fn label(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

pub fn mono(size: f32, weight: f32) -> TextStyle {
    TextStyle { family: Family::SystemMono, ..TextStyle::new(size, weight) }
}

/// rune-kit's components themed from the Omarchy palette.
pub fn rune_kit(ui: &Ui) -> rune_kit::Kit {
    let mut k = rune_kit::Kit::from_palette(rune_kit::PaletteSeed { bg: ui.surface, fg: ui.foreground, accent: ui.accent });
    k.radius.sm = ui.radius.min(6.0);
    k
}

/// A Lucide icon (`crate::icons::PLAY`) at `size` points in `color`.
pub fn icon(i: &'static Icon, size: f32, color: Color) -> El {
    rune::icon(i, size).color(color)
}

/// A square icon button (OmIconButton): hover and press fills, a tooltip label for a11y.
pub fn icon_button<T: 'static>(ui: &Ui, key: &str, i: &'static Icon, name: &str, on: bool, press: impl Fn(&mut T, &mut Tx) + 'static) -> El {
    let fill = if on { ui.selected_fill } else { Color::TRANSPARENT };
    el().key(key.to_owned())
        .role("button")
        .label(name.to_owned())
        .size(32.0, 32.0)
        .radius(ui.radius.clamp(4.0, 8.0))
        .center()
        .fill(fill)
        .hover(ui.hover_fill)
        .press(ui.pressed_fill)
        .cursor(Cursor::Pointer)
        .on_press_tx(press)
        .child(icon(i, 18.0, if on { ui.text_strong } else { ui.text_muted }))
}

/// A panel section header (PanelHeading / OmSectionHeader).
pub fn heading(ui: &Ui, title: &str) -> El {
    text(title.to_owned()).font(label(12.0, 600.0)).color(ui.text_muted)
}

/// A card holding a group of controls (the inspector's sections).
pub fn card(ui: &Ui, key: &str) -> El {
    col().key(key.to_owned()).w_full().gap(12.0).padding(14.0).radius(ui.radius.clamp(6.0, 12.0)).fill(ui.card)
}

/// A name on the left and `control` on the right (OmSettingRow).
pub fn setting_row(ui: &Ui, name: &str, control: El) -> El {
    row().w_full().items_center().gap(8.0).child(text(name.to_owned()).font(label(13.0, 500.0)).color(ui.foreground)).child(el().grow(1.0)).child(control)
}

/// A switch with its name (OmToggle in a row).
pub fn switch_row<T: 'static>(k: &rune_kit::Kit, ui: &Ui, name: &str, on: bool, toggle: impl Fn(&mut T) + 'static) -> El {
    setting_row(ui, name, rune_kit::Switch::new(on).on_toggle(toggle).build(k).label(name.to_owned()))
}

/// A key cap (OmKeycap): "Ctrl", "S", ...
pub fn keycap(ui: &Ui, key: &str) -> El {
    row().h(22.0).px(7.0).items_center().radius(5.0).fill(ui.normal_fill).border(1.0, ui.hairline).child(text(key.to_owned()).font(mono(11.0, 600.0)).color(ui.foreground))
}

/// A labelled slider (OmSlider / PanelSlider). `apply` gets the value and the drag phase: Down is
/// the press (rune springs the knob there: `tx.animated()` is called for it), Move the drag, Up
/// the release. Editors coalesce Move into one undo step and end it on Up.
#[allow(clippy::too_many_arguments)]
pub fn slider<T: 'static>(
    ui: &Ui,
    key: &str,
    name: &str,
    value: f32,
    min: f32,
    max: f32,
    format: impl Fn(f32) -> String,
    apply: impl Fn(&mut T, f32, Phase) + 'static,
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
        .on_drag(move |s: &mut T, drag, tx: &mut Tx| {
            let v = min + (drag.x / drag.w.max(1.0)).clamp(0.0, 1.0) * (max - min);
            if matches!(drag.phase, Phase::Down) {
                tx.animated();
            }
            apply(s, v, drag.phase);
            tx.wake_after(Duration::ZERO);
        })
        .child(el().w_full().h(4.0).radius(2.0).fill(ui.normal_fill).child(el().key(format!("{key}-fill")).h(4.0).w_frac(t).radius(2.0).fill(ui.accent)))
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
                .child(text(name.to_owned()).font(label(13.0, 500.0)).color(ui.foreground))
                .child(el().grow(1.0))
                .child(text(format(value)).font(mono(12.0, 500.0)).color(ui.text_muted)),
        )
        .child(track)
}

/// A primary (accent) or quiet button with an optional leading icon (OmButton).
pub fn button<T: 'static>(ui: &Ui, key: &str, name: &str, i: Option<&'static Icon>, primary: bool, press: impl Fn(&mut T, &mut Tx) + 'static) -> El {
    let (fill, fg) = if primary { (ui.accent, ui.accent_foreground) } else { (ui.normal_fill, ui.foreground) };
    let mut b = row()
        .key(key.to_owned())
        .role("button")
        .label(name.to_owned())
        .h(32.0)
        .px(12.0)
        .gap(6.0)
        .items_center()
        .radius(ui.radius.clamp(4.0, 8.0))
        .fill(fill)
        .hover(if primary { ui.accent_hover } else { ui.hover_fill })
        .press(if primary { ui.accent_pressed } else { ui.pressed_fill })
        .cursor(Cursor::Pointer)
        .on_press_tx(press);
    if let Some(i) = i {
        b = b.child(icon(i, 16.0, fg));
    }
    b.child(text(name.to_owned()).font(label(13.0, 600.0)).color(fg))
}
