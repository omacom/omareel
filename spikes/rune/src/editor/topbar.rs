//! TopBar.qml: the bundle's name, presets, undo and redo, and the Export button that opens the
//! export dialog.

use super::{edited, label, mono};
use crate::preview::Preview;
use omareel_core::editor::Effect;
use rune::{Cx, El, KeyEvent, Tx, el, row, text};
use rune_kit::{Button, ButtonVariant};

/// The top bar's own UI state.
#[derive(Default)]
pub struct State {}

pub fn on_effect(_s: &mut Preview, _effect: &Effect) {}

pub fn poll(_s: &mut Preview) -> bool {
    false
}

pub fn busy(_s: &Preview) -> bool {
    false
}

pub fn on_key(_s: &mut Preview, _key: &KeyEvent, _tx: &mut Tx) -> bool {
    false
}

pub fn view(s: &Preview, _cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
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
        .child(Button::new("Export").variant(ButtonVariant::Contrast).on_press(|s: &mut Preview| super::dialogs::open_export(s)).build(k))
}
