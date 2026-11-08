//! The timeline's pop-ups, drawn over the window (OmMenu, OmPopup): the clip badge's speed and
//! layout menu, the aspect menu, the narrow bar's canvas menu and the shortcuts sheet. Rune's
//! native `popup_menu` exists only on macOS, so these are in-app surfaces anchored to the
//! element that opened them, over a scrim that closes them (Escape too, through `modal`).

use super::{Popup, card_radius, control_radius};
use crate::editor::{edited, label, mono, set};
use crate::icons;
use crate::kit::{icon, keycap};
use crate::preview::Preview;
use rune::{Align, Color, Cursor, Cx, El, Side, Tx, col, el, row, text};
use std::time::Duration;

pub const ASPECTS: [(&str, &str); 7] = [
    ("Auto", "auto"),
    ("Wide 16:9", "16:9"),
    ("Vertical 9:16", "9:16"),
    ("Square 1:1", "1:1"),
    ("Portrait 4:5", "4:5"),
    ("Classic 4:3", "4:3"),
    ("Tall 3:4", "3:4"),
];

pub const SPEEDS: [f64; 13] = [0.5, 0.75, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 3.0, 4.0, 8.0, 16.0, 24.0];

pub const SHORTCUTS: [(&str, &str); 16] = [
    ("Space", "Play / pause"),
    ("← →", "Step one frame"),
    ("Shift ← →", "Jump one second"),
    ("S", "Split at playhead"),
    ("Z", "Add zoom"),
    ("I", "Insert a video"),
    ("Delete", "Remove selection"),
    ("Backspace", "Remove range"),
    ("Ctrl Z", "Undo"),
    ("Ctrl Shift Z", "Redo"),
    ("Ctrl S", "Save"),
    ("Ctrl E", "Export"),
    ("Scroll", "Zoom timeline at pointer"),
    ("Middle drag", "Pan timeline"),
    ("Esc", "Cancel point picking"),
    ("?", "Show shortcuts"),
];

/// The project's aspect as the menu stores it ("auto" for none).
pub fn aspect(s: &Preview) -> String {
    let a = &s.document.project().aspect;
    if a.is_empty() { "auto".into() } else { a.clone() }
}

fn close(s: &mut Preview) {
    s.edit.timeline.popup = None;
}

/// OmPopupCard: the raised surface.
fn card(ui: &crate::theme::Ui, key: &str) -> El {
    col()
        .key(key.to_owned())
        .radius(card_radius(ui))
        .fill(ui.raised)
        .border(1.0, ui.stroke_strong)
        .shadow(8.0, 28.0, ui.shadow)
        .transition(rune::Transition::opacity().combined(rune::Transition::scale(0.96)))
}

/// OmMenuItem: a row with its text and a check when `checked`.
fn item(ui: &crate::theme::Ui, key: String, name: &str, checked: bool, monospace: bool, press: impl Fn(&mut Preview) + 'static) -> El {
    let font = if monospace { mono(13.0, 400.0) } else { label(13.0, 400.0) };
    let mut r = row()
        .key(key)
        .role("menuitem")
        .label(name.to_owned())
        .h(30.0)
        .px(10.0)
        .gap(8.0)
        .items_center()
        .radius(control_radius(ui))
        .hover(ui.fill_hover)
        .press(ui.fill_pressed)
        .focus_highlight(ui.fill_hover)
        .cursor(Cursor::Pointer)
        .on_press_tx(move |s: &mut Preview, tx: &mut Tx| {
            press(s);
            tx.wake_after(Duration::ZERO);
        })
        .child(text(name.to_owned()).font(font).color(if checked { ui.text_strong } else { ui.foreground }).grow(1.0));
    if checked {
        r = r.child(icon(&icons::CHECK, 14.0, ui.accent));
    } else {
        r = r.child(el().size(14.0, 14.0));
    }
    r
}

fn separator(ui: &crate::theme::Ui, key: String) -> El {
    el().key(key).h(1.0).my(4.0).mx(4.0).fill(ui.separator)
}

/// The open pop-up with its scrim, at the window's root; None when none is open.
pub fn overlay(s: &Preview, _cx: &mut Cx<Preview>) -> Option<El> {
    let ui = &s.ui;
    let popup = s.edit.timeline.popup.clone()?;
    let scrim = el().key("tl-popup-scrim").absolute(0.0, 0.0).w_full().h_full().on_press(close);
    let surface = match &popup {
        Popup::Speed(id) => {
            let clip = s.document.project().clips.iter().find(|c| &c.id == id)?.clone();
            let layouts = s.document.has_camera() && !clip.is_media();
            let mut menu = card(ui, "speed-menu").role("menu").focus_group().w(if layouts { 168.0 } else { 120.0 }).padding(4.0);
            if layouts {
                let current = if clip.layout.is_empty() { "default".to_owned() } else { clip.layout.clone() };
                for (value, name) in [("default", "Screen & camera"), ("camera", "Camera"), ("screen", "Screen")] {
                    let id = clip.id.clone();
                    menu = menu.child(item(ui, format!("layout-{value}"), name, current == value, false, move |s: &mut Preview| {
                        close(s);
                        if s.document.set_clip_layout(&id, value) {
                            edited(s, "Clip layout");
                        }
                    }));
                }
                menu = menu.child(separator(ui, "speed-sep".into()));
            }
            for speed in SPEEDS {
                let id = clip.id.clone();
                let name = format!("{}×", format_speed(speed));
                menu = menu.child(item(ui, format!("speed-{}", format_speed(speed)), &name, clip.speed == speed, true, move |s: &mut Preview| {
                    close(s);
                    if s.document.set_clip_speed(&id, speed) {
                        edited(s, "Clip speed");
                    }
                }));
            }
            menu.anchored(format!("clip-badge-{}", clip.id), Side::Bottom, Align::Center, 4.0)
        }
        Popup::Aspect => {
            let current = aspect(s);
            let mut menu = card(ui, "aspect-list").role("menu").focus_group().w(180.0).padding(4.0);
            for (name, value) in ASPECTS {
                menu = menu.child(item(ui, format!("aspect-{value}"), name, current == value, false, move |s: &mut Preview| {
                    close(s);
                    set(s, "aspect", value.into(), false, "");
                }));
            }
            menu.anchored("aspect-menu", Side::Top, Align::Start, 6.0)
        }
        Popup::Overflow => {
            let current = aspect(s);
            let mut menu = card(ui, "overflow-menu").role("menu").focus_group().w(224.0).padding(8.0).gap(2.0);
            menu = menu.child(text("Canvas").font(label(12.0, 600.0)).color(ui.text_muted).ml(8.0).mt(4.0).mb(4.0));
            for (name, value) in ASPECTS {
                menu = menu.child(item(ui, format!("overflow-aspect-{value}"), name, current == value, false, move |s: &mut Preview| {
                    close(s);
                    set(s, "aspect", value.into(), false, "");
                }));
            }
            menu = menu.child(separator(ui, "overflow-sep".into()));
            let crop = s.edit.timeline.crop_mode;
            menu = menu.child(
                row()
                    .key("overflow-crop")
                    .role("button")
                    .h(32.0)
                    .px(10.0)
                    .gap(8.0)
                    .items_center()
                    .radius(control_radius(ui))
                    .border(1.0, ui.stroke)
                    .fill(if crop { ui.selected_fill } else { Color::TRANSPARENT })
                    .hover(ui.fill_hover)
                    .cursor(Cursor::Pointer)
                    .on_press(|s: &mut Preview| {
                        s.edit.timeline.crop_mode = !s.edit.timeline.crop_mode;
                        close(s);
                    })
                    .child(icon(&icons::CROP, 16.0, ui.foreground))
                    .child(text(if crop { "Finish crop" } else { "Crop" }).font(label(13.0, 500.0)).color(ui.foreground)),
            );
            menu.anchored("canvas-options", Side::Top, Align::Start, 8.0)
        }
        Popup::Shortcuts => {
            let mut grid = col().gap(10.0);
            for pair in SHORTCUTS.chunks(2) {
                let mut line = row().w_full().gap(28.0);
                for (keys, action) in pair {
                    let mut caps = row().gap(3.0);
                    for k in keys.split(' ') {
                        caps = caps.child(keycap(ui, k));
                    }
                    line = line.child(
                        row()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(8.0)
                            .items_center()
                            .child(text(action.to_string()).font(label(13.0, 400.0)).color(ui.foreground).truncate().grow(1.0).min_w(0.0))
                            .child(caps),
                    );
                }
                grid = grid.child(line);
            }
            let head = row()
                .w_full()
                .items_center()
                .child(text("Keyboard shortcuts").font(label(14.0, 600.0)).color(ui.text_strong))
                .child(el().grow(1.0))
                .child(
                    el().key("shortcuts-close")
                        .role("button")
                        .label("Close shortcuts")
                        .size(24.0, 24.0)
                        .center()
                        .radius(control_radius(ui))
                        .hover(ui.fill_hover)
                        .cursor(Cursor::Pointer)
                        .on_press(close)
                        .child(icon(&icons::X, 14.0, ui.text_muted)),
                );
            let width = (s.edit.timeline.bar_width.get() - 24.0).clamp(240.0, 560.0);
            card(ui, "shortcuts-sheet").w(width).padding(20.0).gap(16.0).child(head).child(grid).anchored("shortcuts-button", Side::Top, Align::End, 8.0)
        }
    };
    let surface = surface.modal(true, close);
    Some(el().key("tl-popups").absolute(0.0, 0.0).w_full().h_full().child(scrim).child(surface))
}

/// `Number(speed).toFixed(speed % 1 ? 2 : 0)` for the badge, and the menu's plain number.
pub fn format_speed(speed: f64) -> String {
    let s = format!("{speed}");
    s.trim_end_matches(".0").to_owned()
}

/// The badge's text: two decimals unless whole.
pub fn badge_speed(speed: f64) -> String {
    if speed.fract() != 0.0 { format!("{speed:.2}×") } else { format!("{speed:.0}×") }
}
