//! CameraPanel.qml: show the webcam; its corner on a miniature canvas, size, shape and corner
//! radius, shrinking while zoomed, shadow and border; then what applies to camera clips too:
//! the full-screen look, rotation and mirror.

use super::widgets::{self, SliderSpec, act, button_group, caption, empty_state, om, project_slider, section_header, separator, setting_row, toggle, track_hot};
use crate::editor::{dialogs, set};
use crate::preview::Preview;
use rune::{Color, Cursor, El, col, el};

const SHAPES: [&str; 3] = ["round", "square", "rectangle"];
const ROTATIONS: [i32; 4] = [0, 90, 180, 270];

/// The position pad: click a quadrant and the bubble glides to that corner.
fn position_pad(s: &Preview, w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let c = &s.document.project().camera;
    let h = (w * 9.0 / 16.0 * 0.62).round();
    let inset = 10.0;
    let mut pad = el().key("camera-position-pad").w(w).h(h).radius(o.control_radius).fill(ui.fill).border(1.0, ui.stroke);
    for corner in ["top-left", "top-right", "bottom-left", "bottom-right"] {
        let right = corner.ends_with("right");
        let bottom = corner.starts_with("bottom");
        let key = format!("camera-quadrant-{corner}");
        let current = c.position == corner;
        let hot = widgets::is_hot(s, &key);
        let (qw, qh) = (w / 2.0, h / 2.0);
        let dot = el()
            .absolute(if right { qw - 6.0 - inset - 4.0 } else { inset + 4.0 }, if bottom { qh - 6.0 - inset - 4.0 } else { inset + 4.0 })
            .size(6.0, 6.0)
            .radius(3.0)
            .fill(ui.text_faint)
            .dim(if current { 0.0 } else { 1.0 });
        let quadrant = el()
            .key(key.clone())
            .role("button")
            .label(corner.to_owned())
            .absolute(if right { qw } else { 0.0 }, if bottom { qh } else { 0.0 })
            .size(qw, qh)
            .radius(o.control_radius)
            .fill(if hot && !current { ui.fill_hover } else { Color::TRANSPARENT })
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut Preview| set(s, "camera.position", corner.into(), false, ""))
            .child(dot);
        pad = pad.child(track_hot(quadrant, &key));
    }
    let right = c.position.ends_with("right");
    let bottom = c.position.starts_with("bottom");
    let bw = (h * (0.22 + c.size as f32 * 0.6)).max(16.0);
    let bh = if c.shape == "rectangle" { bw * 9.0 / 16.0 } else { bw };
    let radius = if c.shape == "round" { bw / 2.0 } else { (bw / 3.0).min(c.radius as f32 / 4.0) };
    let mut bubble = el()
        .key("camera-position-bubble")
        .absolute(if right { w - bw - inset } else { inset }, if bottom { h - bh - inset } else { inset })
        .size(bw, bh)
        .radius(radius)
        .fill(ui.accent.alpha(0.85))
        .inert()
        .motion(rune::Spring::new(0.32, 0.25));
    if c.border.enabled {
        bubble = bubble.border(2.0, widgets::qcolor(&c.border.color));
    }
    pad.child(bubble)
}

pub fn view(s: &Preview, w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let d = &s.document;
    let c = &d.project().camera;
    let mut panel = col().w_full().gap(o.panel_gap);
    if !d.has_camera() {
        return panel.child(empty_state(
            ui,
            &crate::icons::VIDEO_OFF,
            "No webcam in this recording",
            "Turn on Webcam in the launcher before recording to add a camera bubble.",
            "",
        ));
    }
    let status = d.camera_status();
    if !status.is_empty() {
        panel = panel.child(caption(ui, &status));
    }
    panel = panel.child(toggle(s, "camera-enabled", "Show camera", "Overlay the webcam on the recording", c.enabled, true, |s, v| {
        set(s, "camera.enabled", v.into(), false, "")
    }));
    let on = c.enabled;
    let shape = SHAPES.iter().position(|v| *v == c.shape);
    let border_color = c.border.color.clone();
    let mut bubble = col()
        .w_full()
        .gap(o.panel_gap)
        .child(section_header(ui, "Position").mt(2.0))
        .child(position_pad(s, w))
        .child(project_slider(s, SliderSpec::new("camera-size", "Size", 0.1, 0.5, c.size, 0.01).decimals(2).enabled(on), "camera.size"))
        .child(setting_row(
            ui,
            "Shape",
            "",
            on,
            button_group(s, "cameraShapeGroup", &["Round", "Square", "Rectangle"], shape, on, Some(212.0), |s, i| {
                set(s, "camera.shape", SHAPES[i].into(), false, "")
            }),
        ));
    if c.shape != "round" {
        bubble = bubble.child(project_slider(s, SliderSpec::new("camera-radius", "Corner radius", 0.0, 80.0, c.radius, 1.0).suffix(" px").enabled(on), "camera.radius"));
    }
    bubble = bubble
        .child(project_slider(s, SliderSpec::new("camera-shrink", "Shrink while zoomed", 0.4, 1.0, c.scale_during_zoom, 0.01).decimals(2).enabled(on), "camera.scaleDuringZoom"))
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(toggle(s, "camera-shadow", "Shadow", "", c.shadow.enabled, on, |s, v| set(s, "camera.shadow.enabled", v.into(), false, "")));
    if c.shadow.enabled {
        bubble = bubble
            .child(project_slider(s, SliderSpec::new("camera-shadow-intensity", "Intensity", 0.0, 1.0, c.shadow.intensity, 0.01).decimals(2).enabled(on), "camera.shadow.intensity"))
            .child(project_slider(s, SliderSpec::new("camera-shadow-blur", "Blur", 0.0, 64.0, c.shadow.blur, 1.0).enabled(on), "camera.shadow.blur"))
            .child(project_slider(s, SliderSpec::new("camera-shadow-distance", "Distance", 0.0, 80.0, c.shadow.distance, 1.0).enabled(on), "camera.shadow.distance"));
    }
    bubble = bubble
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(toggle(s, "camera-border", "Border", "", c.border.enabled, on, |s, v| set(s, "camera.border.enabled", v.into(), false, "")));
    if c.border.enabled {
        bubble = bubble
            .child(project_slider(s, SliderSpec::new("camera-border-width", "Width", 0.0, 16.0, c.border.width, 1.0).suffix(" px").enabled(on), "camera.border.width"))
            .child(project_slider(s, SliderSpec::new("camera-border-alpha", "Opacity", 0.0, 1.0, c.border.alpha, 0.01).decimals(2).enabled(on), "camera.border.alpha"))
            .child(setting_row(
                ui,
                "Color",
                "",
                on,
                widgets::color_well(s, "camera-border-color", &c.border.color, act(move |s, _| {
                    if s.document.project().camera.enabled {
                        dialogs::open_color(s, "Border color", &border_color, |s, c| set(s, "camera.border.color", dialogs::color_value(&c).into(), false, ""));
                    }
                })),
            ));
    }
    panel = panel.child(if on { bubble } else { bubble.dim(0.42) });

    // Orientation and the full-screen look apply to camera clips too, so they stay available
    // with the bubble switched off.
    let rotation = ROTATIONS.iter().position(|r| *r == c.rotation);
    panel
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(section_header(ui, "Full screen"))
        .child(button_group(s, "cameraFullFrameGroup", &["Fill frame", "Square + blur"], Some(usize::from(c.full_frame == "square")), true, None, |s, i| {
            set(s, "camera.fullFrame", (if i == 1 { "square" } else { "fill" }).into(), false, "")
        }))
        .child(caption(ui, "Clips with the Camera layout show the webcam full screen. Set a clip's layout in Clip."))
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(setting_row(
            ui,
            "Rotation",
            "",
            true,
            button_group(s, "camera-rotation", &["0°", "90°", "180°", "270°"], rotation, true, Some(176.0), |s, i| {
                set(s, "camera.rotation", ROTATIONS[i].into(), false, "")
            }),
        ))
        .child(toggle(s, "camera-mirror", "Mirror", "Flip the camera horizontally", c.flip_horizontal, true, |s, v| {
            set(s, "camera.flipHorizontal", v.into(), false, "")
        }))
}
