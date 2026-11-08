//! ClipPanel.qml: the selected clip's layout (screen and camera, camera, screen), or for an
//! inserted video its name, fit and order; its speed; resetting its trims and deleting it; and
//! inserting a video at the playhead.

use super::widgets::{Btn, act, button, button_group, caption, dropdown, empty_state, font, om, panel_label, separator, setting_row};
use crate::editor::{dialogs, edited};
use crate::preview::Preview;
use rune::{El, col, el, row, text};

const LAYOUTS: [&str; 3] = ["default", "camera", "screen"];
const FITS: [&str; 3] = ["fit", "fill", "framed"];
const SPEEDS: [f64; 13] = [0.5, 0.75, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 3.0, 4.0, 8.0, 16.0, 24.0];

/// A number as JavaScript prints it (1, 1.2, 0.75).
fn js_number(v: f64) -> String {
    let s = format!("{v}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_owned)
}

pub fn view(s: &Preview, _w: f32) -> El {
    let ui = &s.ui;
    let o = om();
    let d = &s.document;
    let clips = &d.project().clips;
    let selected_id = d.selected_clip_id().to_owned();
    let index = clips.iter().position(|c| c.id == selected_id);
    let selected = index.map(|i| &clips[i]);
    let mut panel = col().w_full().gap(o.panel_gap);
    match selected {
        None => {
            panel = panel.child(empty_state(
                ui,
                &crate::icons::FILM,
                "No clip selected",
                "Click a clip on the timeline to trim it or change its speed. Press S to split at the playhead.",
                "",
            ))
        }
        Some(clip) => {
            let i = index.unwrap_or(0);
            let insert = clip.is_media();
            let recording_clips = clips.iter().filter(|c| !c.is_media()).count();
            let camera = d.has_camera();
            if camera && !insert {
                let layout = LAYOUTS.iter().position(|l| *l == clip.layout).unwrap_or(0);
                let text_for = match clip.layout.as_str() {
                    "camera" => "The webcam fills the frame. Choose Fill or Square in Camera.",
                    "screen" => "The screen alone, without the camera bubble.",
                    _ if d.project().camera.enabled => "The screen with the camera bubble.",
                    _ => "The screen alone: the camera bubble is off in Camera.",
                };
                panel = panel
                    .child(panel_label(ui, "Layout", true))
                    .child(button_group(s, "clipLayoutGroup", &["Screen & camera", "Camera", "Screen"], Some(layout), true, None, |s, i| {
                        let id = s.document.selected_clip_id().to_owned();
                        if s.document.set_clip_layout(&id, LAYOUTS[i]) {
                            edited(s, "");
                        }
                    }))
                    .child(caption(ui, text_for));
            }
            if insert {
                let info = d.inserts().get(&clip.media).copied().unwrap_or_default();
                let missing = !(info.width > 0 && info.height > 0 && info.duration > 0.0);
                let name = std::path::Path::new(&clip.media).file_name().map_or_else(|| clip.media.clone(), |n| n.to_string_lossy().into_owned());
                let fit = FITS.iter().position(|f| *f == clip.fit).unwrap_or(0);
                panel = panel
                    .child(
                        text(if missing { format!("{name} is missing from the recording.") } else { name })
                            .font(font(o.body, 500.0))
                            .color(if missing { ui.record } else { ui.foreground })
                            .truncate_middle()
                            .w_full(),
                    )
                    .child(setting_row(
                        ui,
                        "Fit",
                        "",
                        true,
                        button_group(s, "clipFitGroup", &["Fit", "Fill", "Framed"], Some(fit), true, Some(176.0), |s, i| {
                            let id = s.document.selected_clip_id().to_owned();
                            if s.document.set_clip_fit(&id, FITS[i]) {
                                edited(s, "");
                            }
                        }),
                    ))
                    .child(caption(
                        ui,
                        match clip.fit.as_str() {
                            "fill" => "Covers the whole frame, cropping what does not fit.",
                            "framed" => "Sits in the frame over the background, like the screen.",
                            _ => "Shows the whole video over a blurred copy of itself.",
                        },
                    ))
                    .child(
                        row()
                            .w_full()
                            .gap(8.0)
                            .child(button(ui, "clip-earlier", "Move earlier", None, Btn { bordered: true, fill: true, disabled: i == 0, ..Btn::default() }, act(|s, _| reorder(s, -1))))
                            .child(button(
                                ui,
                                "clip-later",
                                "Move later",
                                None,
                                Btn { bordered: true, fill: true, disabled: i + 1 >= clips.len(), ..Btn::default() },
                                act(|s, _| reorder(s, 1)),
                            )),
                    );
            }
            if insert || camera {
                panel = panel.child(separator(ui));
            }
            let speed = SPEEDS.iter().position(|v| (*v - clip.speed).abs() < 1e-9);
            let labels: Vec<String> = SPEEDS.iter().map(|v| format!("{}×", js_number(*v))).collect();
            let description = if (clip.speed - 1.0).abs() > 1e-9 { format!("Plays {}× as fast", js_number(clip.speed)) } else { "Original speed".into() };
            panel = panel
                .child(setting_row(
                    ui,
                    "Speed",
                    &description,
                    true,
                    dropdown(s, "clip-speed", &labels, speed, Some(96.0), true, true, |s, i| {
                        let id = s.document.selected_clip_id().to_owned();
                        if s.document.set_clip_speed(&id, SPEEDS[i]) {
                            edited(s, "");
                        }
                    }),
                ))
                .child(separator(ui))
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .child(button(
                            ui,
                            "clip-reset-trims",
                            "Reset trims",
                            Some(&crate::icons::UNDO_2),
                            Btn { bordered: true, fill: true, ..Btn::default() },
                            act(|s, _| {
                                let id = s.document.selected_clip_id().to_owned();
                                if s.document.reset_clip_trims(&id) {
                                    edited(s, "Trims reset");
                                }
                            }),
                        ))
                        .child(button(
                            ui,
                            "clip-delete",
                            "Delete clip",
                            Some(&crate::icons::TRASH_2),
                            // The recording always keeps one clip.
                            Btn { bordered: true, destructive: true, fill: true, disabled: !(clips.len() > 1 && (insert || recording_clips > 1)), ..Btn::default() },
                            act(|s, _| {
                                let id = s.document.selected_clip_id().to_owned();
                                if s.document.remove_clip(&id) {
                                    edited(s, "Clip deleted");
                                }
                            }),
                        )),
                );
        }
    }
    let importing = d.importing();
    panel
        .child(el().w_full().pt(2.0).child(separator(ui)))
        .child(button(
            ui,
            "insertClipButton",
            if importing { "Adding clip…" } else { "Insert video at playhead…" },
            Some(&crate::icons::PLUS),
            Btn { bordered: true, fill: true, disabled: importing, ..Btn::default() },
            act(|s, tx| dialogs::open_insert(s, tx)),
        ))
        .child(caption(ui, "Add a demo or a retake between clips. It is copied into the recording, splitting the clip at the playhead."))
}

/// `reorderClip(id, direction)`, pausing first as the bridge does.
fn reorder(s: &mut Preview, direction: i32) {
    let id = s.document.selected_clip_id().to_owned();
    if !s.document.can_reorder_clip(&id, direction) {
        return;
    }
    if s.playing {
        s.toggle();
    }
    s.document.apply_reorder_clip(&id, direction);
    edited(s, "");
}
