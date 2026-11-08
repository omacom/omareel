//! KeystrokesPanel.qml: whether shortcuts show, a preview of a shortcut, where they show, their
//! size and how long each stays.

use super::widgets::{SliderSpec, dropdown, keycap, om, project_slider, separator, setting_row, toggle};
use crate::editor::set;
use crate::preview::Preview;
use rune::{El, col, el, row};

const POSITIONS: [(&str, &str); 6] = [
    ("Top left", "top-left"),
    ("Top center", "top-center"),
    ("Top right", "top-right"),
    ("Bottom left", "bottom-left"),
    ("Bottom center", "bottom-center"),
    ("Bottom right", "bottom-right"),
];

pub fn view(s: &Preview, _w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let k = &s.document.project().keystrokes;
    let on = k.enabled;
    let position = POSITIONS.iter().position(|(_, v)| *v == k.position).unwrap_or(4);
    let preview = el()
        .key("keys-preview")
        .w_full()
        .h(64.0)
        .radius(o.control_radius)
        .fill(ui.canvas)
        .border(1.0, ui.stroke)
        .center()
        .child(row().gap(4.0).items_center().child(keycap(ui, "Ctrl", 24.0)).child(keycap(ui, "Shift", 24.0)).child(keycap(ui, "P", 24.0)));
    let labels: Vec<String> = POSITIONS.iter().map(|(l, _)| (*l).to_owned()).collect();
    let settings = col()
        .w_full()
        .gap(o.panel_gap)
        .child(preview)
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(setting_row(
            ui,
            "Position",
            "",
            on,
            dropdown(s, "keys-position", &labels, Some(position), Some(150.0), on, false, |s, i| set(s, "keystrokes.position", POSITIONS[i].1.into(), false, "")),
        ))
        .child(project_slider(s, SliderSpec::new("keys-size", "Size", 0.5, 2.0, k.size, 0.05).decimals(2).suffix("×").enabled(on), "keystrokes.size"))
        .child(project_slider(s, SliderSpec::new("keys-hold", "Hold", 100.0, 3000.0, f64::from(k.hold_ms), 50.0).suffix(" ms").enabled(on), "keystrokes.holdMs"));
    col()
        .w_full()
        .gap(o.panel_gap)
        .child(toggle(s, "keys-enabled", "Show keystrokes", "Display shortcuts and special keys; typing is never recorded", on, true, |s, v| {
            set(s, "keystrokes.enabled", v.into(), false, if v { "Keystrokes on" } else { "Keystrokes off" })
        }))
        .child(if on { settings } else { settings.dim(0.42) })
}
