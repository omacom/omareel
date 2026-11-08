//! SidePanel.qml and its panels: Background, Frame (ShapePanel), Cursor, Camera, Zoom,
//! Keystrokes, Audio, Clip. The panel picker is the icon rail on the right in the QML.

use super::{edited, label, set};
use crate::kit;
use crate::preview::Preview;
use omareel_core::editor::Effect;
use rune::{Cx, El, KeyEvent, Phase, Tx, col, el, text};
use rune_kit::{Button, ButtonVariant, SegmentedControl};

/// The side panel's own UI state.
#[derive(Default)]
pub struct State {}

pub const INSPECTOR_W: f32 = 320.0;

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

/// A slider over a project value: drags coalesce into one undo step, ended on release.
fn project_slider(s: &Preview, key: &str, name: &str, path: &'static str, value: f64, min: f32, max: f32, format: impl Fn(f32) -> String) -> El {
    kit::slider(&s.ui, key, name, value as f32, min, max, format, move |s: &mut Preview, v: f32, phase: Phase| {
        set(s, path, f64::from(v).into(), !matches!(phase, Phase::Down), "");
        if matches!(phase, Phase::Up) {
            s.document.end_coalesced_edit();
        }
    })
}

pub fn view(s: &Preview, _cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let k = &s.kit;
    let p = s.document.project();
    let shape = match p.camera.shape.as_str() {
        "square" => 1,
        "rectangle" => 2,
        _ => 0,
    };
    let frame = kit::card(ui, "section-Frame")
        .child(kit::heading(ui, "Frame"))
        .child(project_slider(s, "padding", "Padding", "frame.padding", p.frame.padding, 0.0, 0.25, |v| format!("{:.0}%", v * 100.0)))
        .child(project_slider(s, "radius", "Corner radius", "frame.radius", p.frame.radius, 0.0, 48.0, |v| format!("{v:.0} px")))
        .child(kit::switch_row(k, ui, "Shadow", p.frame.shadow.enabled, |s: &mut Preview| {
            let on = !s.document.project().frame.shadow.enabled;
            set(s, "frame.shadow.enabled", on.into(), false, if on { "Shadow on" } else { "Shadow off" });
        }));
    let camera = kit::card(ui, "section-Camera")
        .child(kit::heading(ui, "Camera"))
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
        .child(project_slider(s, "camera-size", "Size", "camera.size", p.camera.size, 0.1, 0.5, |v| format!("{:.0}%", v * 100.0)))
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
        .child(kit::switch_row(k, ui, "Border", p.camera.border.enabled, |s: &mut Preview| {
            let on = !s.document.project().camera.border.enabled;
            set(s, "camera.border.enabled", on.into(), false, "");
        }));
    let selected = s.document.selected_zoom_id().to_owned();
    let zoom = s.document.project().zooms.iter().find(|z| z.id == selected);
    let mut zoom_section = kit::card(ui, "section-Zoom").child(kit::heading(ui, "Zoom"));
    zoom_section = match zoom {
        Some(z) => zoom_section
            .child(kit::slider(&s.ui, "zoom-level", "Level", z.level as f32, 1.0, 4.0, |v| format!("{v:.1}×"), |s: &mut Preview, v: f32, phase: Phase| {
                let id = s.document.selected_zoom_id().to_owned();
                if s.document.set_zoom_level(&id, f64::from(v), !matches!(phase, Phase::Down)) {
                    edited(s, "");
                }
                if matches!(phase, Phase::Up) {
                    s.document.end_coalesced_edit();
                }
            }))
            .child(
                Button::new("Delete zoom")
                    .variant(ButtonVariant::Outline)
                    .on_press(|s: &mut Preview| {
                        let id = s.document.selected_zoom_id().to_owned();
                        if s.document.remove_zoom(&id) {
                            edited(s, "Delete zoom");
                        }
                    })
                    .build(k),
            ),
        None => zoom_section.child(text("Select a zoom on the timeline").font(label(13.0, 400.0)).color(ui.text_faint)),
    };
    let keys = kit::card(ui, "section-Keystrokes").child(kit::heading(ui, "Keystrokes")).child(kit::switch_row(k, ui, "Show shortcuts", p.keystrokes.enabled, |s: &mut Preview| {
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
