//! ShapePanel.qml: the frame's padding and roundness, its shadow and its border. The settings
//! stay saved but do nothing while the background is None, so they dim then.

use super::widgets::{self, SliderSpec, act, caption, om, project_slider, section_header, separator, setting_row, toggle};
use crate::editor::{dialogs, set};
use crate::preview::Preview;
use rune::{El, col, el};

pub fn view(s: &Preview, _w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let frame = &s.document.project().frame;
    let on = s.document.project().background.type_ != "none";
    let mut panel = col().w_full().gap(o.panel_gap);
    if !on {
        panel = panel.child(caption(ui, "Shape settings stay saved but are ignored while the background type is None."));
    }
    let border_color = frame.border.color.clone();
    let settings = col()
        .w_full()
        .gap(o.panel_gap)
        .child(section_header(ui, "Frame"))
        .child(project_slider(s, SliderSpec::new("frame-padding", "Padding", 0.0, 0.3, frame.padding, 0.005).decimals(2).enabled(on), "frame.padding"))
        .child(project_slider(s, SliderSpec::new("frame-radius", "Roundness", 0.0, 80.0, frame.radius, 1.0).suffix(" px").enabled(on), "frame.radius"))
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(toggle(s, "frame-shadow", "Shadow", "Drop shadow under the recording", frame.shadow.enabled, on, |s, v| {
            set(s, "frame.shadow.enabled", v.into(), false, "")
        }))
        .child(project_slider(s, SliderSpec::new("frame-shadow-intensity", "Intensity", 0.0, 1.0, frame.shadow.intensity, 0.01).decimals(2).enabled(on), "frame.shadow.intensity"))
        .child(project_slider(s, SliderSpec::new("frame-shadow-blur", "Blur", 0.0, 100.0, frame.shadow.blur, 1.0).enabled(on), "frame.shadow.blur"))
        .child(project_slider(s, SliderSpec::new("frame-shadow-distance", "Distance", 0.0, 100.0, frame.shadow.distance, 1.0).enabled(on), "frame.shadow.distance"))
        .child(project_slider(s, SliderSpec::new("frame-shadow-angle", "Angle", 0.0, 360.0, frame.shadow.angle, 1.0).suffix("°").enabled(on), "frame.shadow.angle"))
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(toggle(s, "frame-border", "Border", "Inset outline around the frame", frame.border.enabled, on, |s, v| {
            set(s, "frame.border.enabled", v.into(), false, "")
        }))
        .child(project_slider(s, SliderSpec::new("frame-border-width", "Width", 0.0, 16.0, frame.border.width, 1.0).suffix(" px").enabled(on), "frame.border.width"))
        .child(setting_row(
            ui,
            "Color",
            "",
            on,
            widgets::color_well(s, "frame-border-color", &frame.border.color, act(move |s, _| {
                dialogs::open_color(s, "Border color", &border_color, |s, c| set(s, "frame.border.color", dialogs::color_value(&c).into(), false, ""));
            })),
        ))
        .child(project_slider(s, SliderSpec::new("frame-border-alpha", "Opacity", 0.0, 1.0, frame.border.alpha, 0.01).decimals(2).enabled(on), "frame.border.alpha"));
    panel.child(if on { settings } else { settings.dim(0.42) })
}
