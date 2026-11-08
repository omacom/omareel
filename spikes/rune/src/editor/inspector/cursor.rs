//! CursorPanel.qml: show and size the pointer, its style, the motion presets, the click effect
//! and sound with the ring colour, hiding it when idle, and the spring.

use super::widgets::{self, SliderSpec, act, button_group, caption, dropdown, om, project_slider, section_header, separator, setting_row, toggle, track_hot};
use crate::editor::{dialogs, edited, set};
use crate::preview::Preview;
use omareel_core::compat::qjson::Value;
use rune::{Cursor, El, col, el, row};

const STYLES: [(&str, &str); 4] = [("Light arrow", "light-arrow"), ("Dark arrow", "dark-arrow"), ("Dot", "dot"), ("Hand", "hand")];
const CLICK_EFFECTS: [(&str, &str); 2] = [("None", "none"), ("Circle", "circle")];
const SOUNDS: [(&str, &str); 2] = [("None", "none"), ("Soft", "soft")];

/// `setMotion(mass, stiffness, damping)`: smoothing on and the spring, as one undo step.
fn set_motion(s: &mut Preview, mass: f64, stiffness: f64, damping: f64) {
    s.document.begin_coalesced_edit("cursor-motion");
    s.document.set_project_value("cursor.smoothing", true.into(), true);
    s.document.set_project_value("cursor.spring.mass", mass.into(), true);
    s.document.set_project_value("cursor.spring.stiffness", stiffness.into(), true);
    s.document.set_project_value("cursor.spring.damping", damping.into(), true);
    s.document.end_coalesced_edit();
    edited(s, "");
}

pub fn view(s: &Preview, w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let c = &s.document.project().cursor;
    let is_motion = |m: f64, k: f64, d: f64| c.smoothing && (c.spring.mass - m).abs() < 0.001 && (c.spring.stiffness - k).abs() < 0.001 && (c.spring.damping - d).abs() < 0.001;
    let motion = if is_motion(1.0, 900.0, 60.0) {
        Some(0)
    } else if is_motion(3.0, 470.0, 70.0) {
        Some(1)
    } else {
        None
    };
    // clickEffectIndex: "ripple" (the old name) is the circle.
    let effect = CLICK_EFFECTS.iter().position(|(_, v)| *v == c.click_effect).unwrap_or(usize::from(c.click_effect == "ripple"));
    let sound = usize::from(c.click_sound == "soft");

    let tile_w = (w - 3.0 * 6.0) / 4.0;
    let mut styles = row().w_full().gap(6.0);
    for (i, (label, value)) in STYLES.iter().enumerate() {
        let key = format!("cursor-style-{value}");
        let selected = c.style == *value;
        let hot = widgets::is_hot(s, &key);
        let pressed = s.edit.inspector.pressed.as_deref() == Some(key.as_str());
        let glyph = if *value == "dot" {
            el().size(20.0, 20.0).radius(10.0).fill(ui.accent.alpha(0.68)).border(1.0, ui.stroke_strong)
        } else {
            let image = &s.media.cursors[[0, 1, 0, 2][i]];
            el().size(24.0, 24.0).pixels(image.pixels())
        };
        let tile = el()
            .key(key.clone())
            .role("radio")
            .label((*label).to_owned())
            .checked(selected)
            .w(tile_w)
            .h(58.0)
            .radius(o.control_radius)
            .fill(if selected {
                ui.accent.alpha(0.12)
            } else if hot {
                ui.fill_hover
            } else {
                ui.fill
            })
            .border(if selected { 1.5 } else { 1.0 }, if selected { ui.accent } else { ui.stroke })
            .center()
            .cursor(Cursor::Pointer)
            .help((*label).to_owned())
            .on_press(move |s: &mut Preview| set(s, "cursor.style", (*value).into(), false, ""))
            .child(el().size(26.0, 26.0).center().scale(if pressed { 0.9 } else if hot { 1.08 } else { 1.0 }).child(glyph));
        styles = styles.child(track_hot(tile, &key));
    }

    let ring = c.ring_color.clone();
    let idle = c.hide_when_idle_ms >= 0;
    let mut panel = col()
        .w_full()
        .gap(o.panel_gap)
        .child(toggle(s, "cursor-visible", "Show cursor", "Draw the smoothed pointer over the video", c.visible, true, |s, v| {
            set(s, "cursor.visible", v.into(), false, "")
        }))
        .child(project_slider(s, SliderSpec::new("cursor-size", "Size", 0.5, 3.0, c.size, 0.05).decimals(2).suffix("×"), "cursor.size"))
        .child(section_header(ui, "Style").mt(4.0))
        .child(styles)
        .child(section_header(ui, "Motion").mt(4.0))
        .child(button_group(s, "cursor-motion", &["Natural", "Smooth"], motion, true, None, |s, i| {
            if i == 0 { set_motion(s, 1.0, 900.0, 60.0) } else { set_motion(s, 3.0, 470.0, 70.0) }
        }))
        .child(caption(
            ui,
            if motion.is_none() { "Using a custom spring — tune it below." } else { "Natural stays close to raw movement; Smooth uses a softer spring." },
        ))
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(section_header(ui, "Clicks"))
        .child(setting_row(
            ui,
            "Effect",
            "",
            true,
            dropdown(s, "cursor-click-effect", &CLICK_EFFECTS.map(|(l, _)| l.to_owned()), Some(effect), Some(120.0), true, false, |s, i| {
                set(s, "cursor.clickEffect", CLICK_EFFECTS[i].1.into(), false, "")
            }),
        ))
        .child(setting_row(
            ui,
            "Sound",
            "",
            true,
            dropdown(s, "cursor-click-sound", &SOUNDS.map(|(l, _)| l.to_owned()), Some(sound), Some(120.0), true, false, |s, i| {
                set(s, "cursor.clickSound", SOUNDS[i].1.into(), false, "")
            }),
        ))
        .child(setting_row(
            ui,
            "Ring color",
            "",
            true,
            widgets::color_well(s, "cursor-ring-color", &c.ring_color, act(move |s, _| {
                dialogs::open_color(s, "Ring color", &ring, |s, c| set(s, "cursor.ringColor", dialogs::color_value(&c).into(), false, ""));
            })),
        ))
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(toggle(s, "cursor-idle", "Hide when idle", "Fade the cursor out when it stops moving", idle, true, |s, v| {
            set(s, "cursor.hideWhenIdleMs", if v { Value::from(1500) } else { Value::Null }, false, "")
        }));
    if idle {
        panel = panel.child(project_slider(
            s,
            SliderSpec::new("cursor-idle-delay", "Idle delay", 250.0, 5000.0, f64::from(c.hide_when_idle_ms), 250.0).suffix(" ms"),
            "cursor.hideWhenIdleMs",
        ));
    }
    panel
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(section_header(ui, "Spring"))
        .child(project_slider(s, SliderSpec::new("cursor-mass", "Mass", 0.2, 6.0, c.spring.mass, 0.05).decimals(2), "cursor.spring.mass"))
        .child(project_slider(s, SliderSpec::new("cursor-stiffness", "Stiffness", 50.0, 1000.0, c.spring.stiffness, 5.0), "cursor.spring.stiffness"))
        .child(project_slider(s, SliderSpec::new("cursor-damping", "Damping", 5.0, 120.0, c.spring.damping, 1.0), "cursor.spring.damping"))
}
