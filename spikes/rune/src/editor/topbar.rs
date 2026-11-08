//! TopBar.qml: the reel and "omareel / <name>" with the name editable in place, the saved
//! state, Undo, Redo and Save with their enabled states, the Presets menu with its Save and
//! Delete dialogs, and Export.

use super::dialogs::{self, Modal};
use super::inspector::widgets::{self, Btn, MenuItem, act, font, mono, om};
use super::{edited, set};
use crate::preview::Preview;
use omareel_core::editor::Effect;
use rune::{Color, Cursor, Cx, El, Key, KeyEvent, TextEdit, Tx, col, el, row, text};
use std::rc::Rc;

/// assets/icons/reel.svg.
static REEL: rune::Icon = rune::Icon {
    name: "omareel-reel",
    view: 24.0,
    body: "<circle cx=\"11\" cy=\"11\" r=\"8.6\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2.6\"/>\
<circle cx=\"11\" cy=\"11\" r=\"2.1\" fill=\"currentColor\"/>\
<path d=\"M11 8.9V4.2M8.9 12.1 4.6 14.6M13.1 12.1l4.3 2.5\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2.4\" stroke-linecap=\"round\"/>\
<path d=\"M16.2 17.6c1.6 1.9 3.6 3 6.3 3\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2.4\" stroke-linecap=\"round\"/>",
};

/// The top bar's own UI state.
#[derive(Default)]
pub struct State {
    /// The project name field.
    name: TextEdit,
    name_was_focused: bool,
    /// "Saved HH:mm": when the project was last saved (the bar's creation, then each autosave).
    saved_time: String,
    /// The Save preset dialog's field and the Delete preset dialog's choice.
    preset_name: TextEdit,
    delete_choice: usize,
}

/// The local time as HH:mm (`Qt.formatTime(new Date(), "HH:mm")`).
fn now_hhmm() -> String {
    // SAFETY: localtime_r writes only the tm it is given.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

/// `commitProjectName()`: the field's text becomes the project's name when it differs.
pub fn commit_name(s: &mut Preview) {
    let typed = s.edit.topbar.name.text.clone();
    if !s.edit.topbar.name.text.is_empty() && typed != s.document.bundle_name() {
        set(s, "name", typed.into(), false, "");
    }
}

fn sync_name(s: &mut Preview) {
    if !s.edit.topbar.name.focused && s.edit.topbar.name.text != s.document.bundle_name() {
        let name = s.document.bundle_name().to_owned();
        s.edit.topbar.name.replace_text(name);
    }
}

pub fn on_effect(s: &mut Preview, effect: &Effect) {
    match effect {
        Effect::Autosaved(_) => s.edit.topbar.saved_time = now_hhmm(),
        Effect::ProjectChanged | Effect::HistoryChanged => sync_name(s),
        _ => {}
    }
}

pub fn poll(s: &mut Preview) -> bool {
    let mut changed = false;
    if s.edit.topbar.saved_time.is_empty() {
        s.edit.topbar.saved_time = now_hhmm();
        changed = true;
    }
    // onEditingFinished: the name commits when the field loses focus.
    let focused = s.edit.topbar.name.focused;
    if s.edit.topbar.name_was_focused && !focused {
        commit_name(s);
        changed = true;
    }
    s.edit.topbar.name_was_focused = focused;
    if !focused && s.edit.topbar.name.text.is_empty() && !s.document.bundle_name().is_empty() {
        sync_name(s);
        changed = true;
    }
    changed
}

pub fn busy(_s: &Preview) -> bool {
    false
}

/// Main.qml's Ctrl+E and Ctrl+S (which commits the name first).
pub fn on_key(s: &mut Preview, key: &KeyEvent, _tx: &mut Tx) -> bool {
    match key.key {
        Key::Command('e') => {
            dialogs::open_export(s);
            true
        }
        Key::Command('s') => {
            commit_name(s);
            s.document.save_now();
            edited(s, "Saved");
            true
        }
        _ => false,
    }
}

fn save(s: &mut Preview) {
    commit_name(s);
    s.document.save_now();
    edited(s, "Saved");
}

/// The Presets menu: save, the presets by name, delete.
fn presets_menu(s: &mut Preview) {
    let names = s.document.preset_names();
    let mut items = vec![MenuItem::new("Save current as preset…")];
    if !names.is_empty() {
        items.push(MenuItem::separator());
    }
    items.extend(names.iter().map(|n| MenuItem::new(n.clone())));
    items.push(MenuItem::separator());
    items.push(MenuItem { destructive: true, enabled: !names.is_empty(), ..MenuItem::new("Delete preset…") });
    let count = names.len();
    let names = Rc::new(names);
    dialogs::toggle_menu_end(
        s,
        "presets",
        210.0,
        items,
        Rc::new(move |s: &mut Preview, i: usize| {
            let first_name = if count > 0 { 2 } else { 1 };
            if i == 0 {
                // onOpened: an empty, focused field.
                s.edit.topbar.preset_name = TextEdit::new("");
                s.edit.topbar.preset_name.focused = true;
                dialogs::open_modal(s, Modal::SavePreset);
            } else if (first_name..first_name + count).contains(&i) {
                let name = names[i - first_name].clone();
                s.document.load_preset(&name);
                edited(s, &format!("Preset {name}"));
            } else if i == first_name + count + 1 {
                s.edit.topbar.delete_choice = 0;
                dialogs::open_modal(s, Modal::DeletePreset);
            }
        }),
    );
}

fn save_preset(s: &mut Preview) {
    let name = s.edit.topbar.preset_name.text.clone();
    if name.trim().is_empty() {
        return;
    }
    s.document.save_preset(&name);
    dialogs::close_modal(s);
    edited(s, "Preset saved");
}

/// The Save preset and Delete preset dialogs' contents (TopBar.qml's PresetDialogs).
pub fn preset_dialog(s: &Preview) -> El {
    let ui = &s.ui;
    let o = om();
    let buttons = |confirm: El| {
        row()
            .w_full()
            .pt(4.0)
            .gap(8.0)
            .child(el().grow(1.0))
            .child(widgets::button(ui, "preset-cancel", "Cancel", None, Btn::bordered(), act(|s, _| dialogs::close_modal(s))))
            .child(confirm)
    };
    if matches!(s.edit.dialogs.modal, Some(Modal::DeletePreset)) {
        let names = s.document.preset_names();
        let choice = s.edit.topbar.delete_choice.min(names.len().saturating_sub(1));
        let chosen = names.get(choice).cloned().unwrap_or_default();
        return col()
            .key("delete-preset-dialog")
            .w_full()
            .gap(14.0)
            .child(dialogs::dialog_title(ui, "Delete preset", "This can't be undone."))
            .child(widgets::dropdown(s, "delete-preset-name", &names, Some(choice), None, true, false, |s, i| s.edit.topbar.delete_choice = i))
            .child(buttons(widgets::button(
                ui,
                "preset-delete",
                "Delete",
                None,
                Btn { primary: true, destructive: true, ..Btn::default() },
                act(move |s, _| {
                    s.document.delete_preset(&chosen);
                    dialogs::close_modal(s);
                    edited(s, "Preset deleted");
                }),
            )));
    }
    let can_save = !s.edit.topbar.preset_name.text.trim().is_empty();
    col()
        .key("save-preset-dialog")
        .w_full()
        .gap(14.0)
        .child(dialogs::dialog_title(ui, "Save preset", "Background, frame, cursor, and camera styling."))
        .child(widgets::text_field(ui, "preset-name", &s.edit.topbar.preset_name, "Preset name", |s: &mut Preview| &mut s.edit.topbar.preset_name, |s, _| save_preset(s)))
        .child(buttons(widgets::button(
            ui,
            "preset-save",
            "Save preset",
            None,
            Btn { disabled: !can_save, ..Btn::primary() },
            act(|s, _| save_preset(s)),
        )))
        .child(text("").font(font(o.caption, 400.0)).h(0.0))
}

pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let o = om();
    let bar_w = cx.layout_size("top-bar").map_or(1440.0, |[w, _]| w);
    let t = &s.edit.topbar;
    let focused = t.name.focused;
    let logo_hot = widgets::is_hot(s, "logo");
    let logo = widgets::track_hot(
        el().key("logo").size(18.0, 18.0).child(rune::icon(&REEL, 18.0).color(if logo_hot { ui.accent } else { ui.text_strong }).rotate(if logo_hot { std::f32::consts::TAU } else { 0.0 })),
        "logo",
    );
    // TextMetrics' advance, near enough: the field grows with the name up to 360 points.
    let shown = if t.name.text.is_empty() { s.document.bundle_name() } else { t.name.text.as_str() };
    let name_w = (shown.chars().count() as f32 * o.body * 0.58 + 22.0).clamp(80.0, 360.0);
    let name_hot = widgets::is_hot(s, "project-name");
    let name_field = rune::text_input(&t.name, font(o.body, 600.0))
        .key("project-name")
        .label("Project name")
        .single_line()
        .color(ui.text_strong)
        .caret(ui.accent, ui.accent.alpha(0.35))
        .on_edit(|s: &mut Preview| &mut s.edit.topbar.name)
        .on_submit(|s: &mut Preview, tx: &mut Tx| {
            commit_name(s);
            tx.blur();
        })
        .on_input_key(|s: &mut Preview, key: &KeyEvent, tx: &mut Tx| {
            if matches!(key.key, Key::Escape) {
                let name = s.document.bundle_name().to_owned();
                s.edit.topbar.name.replace_text(name);
                tx.blur();
                return true;
            }
            false
        })
        .w(name_w)
        .px(8.0)
        .py(6.0)
        .radius(o.control_radius)
        .fill(if focused {
            ui.canvas
        } else if name_hot {
            ui.fill_hover
        } else {
            Color::TRANSPARENT
        })
        .border(if focused { 1.0 } else { 0.0 }, ui.accent);
    let name_field = widgets::track_hot(name_field, "project-name");
    let dirty = s.document.dirty();
    let mut bar = row()
        .key("top-bar")
        .w_full()
        .h(48.0)
        .pl(16.0)
        .pr(12.0)
        .gap(6.0)
        .items_center()
        .fill(ui.surface)
        .child(el().absolute(0.0, 47.0).w_full().h(1.0).fill(ui.hairline))
        .child(logo)
        .child(text("omareel").font(mono(o.body, 700.0)).color(ui.text_strong))
        .child(text("/").font(mono(o.body, 400.0)).color(ui.text_faint).ml(4.0))
        .child(row().grow(1.0).min_w(120.0).h(30.0).items_center().child(name_field));
    if bar_w >= 1000.0 {
        bar = bar.child(
            row()
                .key("save-state")
                .gap(6.0)
                .mr(6.0)
                .items_center()
                .child(el().size(6.0, 6.0).radius(3.0).fill(if dirty { ui.zoom_accent } else { ui.text_faint }))
                .child(
                    text(if dirty { "Unsaved changes".to_owned() } else { format!("Saved {}", t.saved_time) })
                        .font(font(o.small, 400.0))
                        .color(ui.text_muted),
                ),
        );
    }
    let presets_open = s.edit.dialogs.menu.as_ref().is_some_and(|m| m.anchor == "presets");
    bar.child(widgets::icon_button(s, "undo", &crate::icons::UNDO_2, "Undo", "Undo  Ctrl Z", s.document.can_undo(), false, act(|s, _| {
        s.document.undo();
        edited(s, "Undo");
    })))
    .child(widgets::icon_button(s, "redo", &crate::icons::REDO_2, "Redo", "Redo  Ctrl Shift Z", s.document.can_redo(), false, act(|s, _| {
        s.document.redo();
        edited(s, "Redo");
    })))
    .child(widgets::icon_button(s, "save", &crate::icons::SAVE, "Save project", "Save project  Ctrl S", dirty, false, act(|s, _| save(s))))
    .child(el().size(1.0, 18.0).mx(6.0).fill(ui.separator))
    .child(widgets::button(
        ui,
        "presets",
        "Presets",
        None,
        Btn { selected: presets_open, trailing: Some(&crate::icons::CHEVRON_DOWN), ..Btn::default() },
        act(|s, _| presets_menu(s)),
    ))
    .child(
        widgets::button(ui, "export", "Export", Some(&crate::icons::DOWNLOAD), Btn::primary(), act(|s, _| dialogs::open_export(s)))
            .ml(2.0)
            .help("Export video  Ctrl E")
            .cursor(Cursor::Pointer),
    )
}
