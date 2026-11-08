//! ZoomPanel.qml: the selected zoom's level, focus (auto, or a point picked on the preview)
//! and removal; then every zoom's motion: the spring presets, edge snapping, motion blur, cut
//! instead of glide, and regenerating the automatic zooms.

use super::widgets::{Btn, SliderSpec, act, button, button_group, caption, empty_state, font, om, panel_slider, project_slider, section_header, separator, setting_row, toggle};
use crate::editor::{edited, set};
use crate::preview::Preview;
use omareel_core::editor::ZoomTarget;
use rune::{El, Phase, col, el, row, text};

const PRESETS: [[f64; 3]; 3] = [[2.6, 170.0, 44.0], [2.25, 200.0, 40.0], [1.4, 300.0, 34.0]];
const CAPTIONS: [&str; 3] = [
    "Smooth — relaxed movement with a gentle settle.",
    "Default — balanced movement for most recordings.",
    "Snappy — quick response with a crisp settle.",
];

pub fn view(s: &Preview, _w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let p = s.document.project();
    let selected_id = s.document.selected_zoom_id().to_owned();
    let selected = p.zooms.iter().find(|z| z.id == selected_id);
    let style = &p.zoom_style;
    let motion = PRESETS.iter().position(|[m, k, d]| (style.spring.mass - m).abs() < 0.001 && (style.spring.stiffness - k).abs() < 0.001 && (style.spring.damping - d).abs() < 0.001);
    let mut panel = col().w_full().gap(o.panel_gap);
    match selected {
        None => panel = panel.child(empty_state(ui, &crate::icons::ZOOM_IN, "No zoom selected", "Pick one on the timeline, or press", "Z")),
        Some(z) => {
            let picking = s.document.picking_zoom_target();
            let manual = picking || !z.automatic_target;
            let count = s.document.selected_zoom_ids().len();
            let level = panel_slider(
                s,
                SliderSpec::new("zoom-level", "Level", 1.0, 4.0, z.level, 0.05).decimals(1).suffix("×"),
                |s: &mut Preview, v: f64, phase: Phase| {
                    let id = s.document.selected_zoom_id().to_owned();
                    match phase {
                        Phase::Down => {
                            s.document.begin_coalesced_edit("zoom-level");
                            s.document.set_zoom_level(&id, v, true);
                        }
                        Phase::Up => s.document.end_coalesced_edit(),
                        _ => {
                            s.document.set_zoom_level(&id, v, true);
                        }
                    }
                    edited(s, "");
                },
            );
            panel = panel
                .child(level)
                .child(setting_row(
                    ui,
                    "Focus",
                    "",
                    true,
                    button_group(s, "zoom-focus", &["Auto", "Manual"], Some(usize::from(manual)), true, Some(150.0), |s, i| {
                        let id = s.document.selected_zoom_id().to_owned();
                        if i == 0 {
                            s.document.set_zoom_target(&id, ZoomTarget::Auto);
                        } else {
                            s.document.set_picking_zoom_target(true);
                        }
                        edited(s, "");
                    }),
                ))
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .child(button(
                            ui,
                            "zoom-pick",
                            if picking { "Click the preview…" } else { "Pick point" },
                            Some(&crate::icons::MOUSE_POINTER_2),
                            Btn { bordered: true, selected: picking, fill: true, ..Btn::default() },
                            act(|s, _| {
                                let on = !s.document.picking_zoom_target();
                                s.document.set_picking_zoom_target(on);
                                edited(s, "");
                            }),
                        ))
                        .child(button(
                            ui,
                            "zoom-remove",
                            &if count > 1 { format!("Remove {count}") } else { "Remove".to_owned() },
                            Some(&crate::icons::TRASH_2),
                            Btn { bordered: true, destructive: true, fill: true, ..Btn::default() },
                            act(|s, _| {
                                s.document.remove_selected_zooms();
                                edited(s, "Zoom removed");
                            }),
                        )),
                );
        }
    }
    panel
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(section_header(ui, "Motion"))
        .child(button_group(s, "zoom-motion", &["Smooth", "Default", "Snappy"], motion, true, None, |s, i| {
            let [m, k, d] = PRESETS[i];
            s.document.begin_coalesced_edit("zoom-spring");
            s.document.set_project_value("zoomStyle.spring.mass", m.into(), true);
            s.document.set_project_value("zoomStyle.spring.stiffness", k.into(), true);
            s.document.set_project_value("zoomStyle.spring.damping", d.into(), true);
            s.document.end_coalesced_edit();
            edited(s, "");
        }))
        .child(caption(ui, motion.map_or("Custom spring.", |i| CAPTIONS[i])))
        .child(project_slider(s, SliderSpec::new("zoom-snap", "Edge snapping", 0.0, 0.5, style.snap_to_edges_ratio, 0.01).decimals(2), "zoomStyle.snapToEdgesRatio"))
        .child(
            text("How close to the frame edge a zoom may look. 0 keeps the target centered; higher lets it slide to the edge.")
                .font(font(o.caption, 400.0))
                .color(ui.text_muted)
                .wrap()
                .w_full()
                .mt(-6.0),
        )
        .child(project_slider(s, SliderSpec::new("zoom-motion-blur", "Motion blur", 0.0, 1.0, style.motion_blur, 0.05).decimals(2), "zoomStyle.motionBlur"))
        .child(toggle(s, "zoom-instant", "Cut instead of glide", "Jump straight in and out instead of animating", style.instant_animation, true, |s, v| {
            set(s, "zoomStyle.instantAnimation", v.into(), false, "")
        }))
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(button(
            ui,
            "zoom-regenerate",
            "Regenerate zooms",
            Some(&crate::icons::SPARKLES),
            Btn { bordered: true, fill: true, ..Btn::default() },
            act(|s, _| {
                s.document.regenerate_zooms();
                edited(s, "Zooms regenerated");
            }),
        ))
}
