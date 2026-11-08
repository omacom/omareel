//! The editor's dialogs, drawn over the window: ExportDialog.qml (format, quality, size, where
//! to, progress, open the result: `dialogs/export.rs`), InsertClipDialog.qml with the import it
//! starts, the colour dialog the panels' colour wells open (`dialogs/color.rs`), the top bar's
//! preset dialogs, OmConfirm, and every pop-up menu (OmMenu, OmDropdown's list).
//!
//! Rune mounts overlays at the window's root, so this one layer (the last child of the window,
//! `editor::view`) draws all of them, from the state here.

mod color;
mod export;

use super::inspector::widgets::{self, Btn, MenuItem, font, om};
use crate::preview::Preview;
use omareel_core::compat::qcolor::QColor;
use omareel_core::editor::{Effect, ImportedInsert};
use rune::{Align, Cursor, Cx, El, Key, KeyEvent, Side, Tx, col, el, row, text};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

pub use color::open_color;
pub use export::open_export;

/// An open pop-up menu: its rows, anchored under the element keyed `anchor`.
pub struct Menu {
    pub anchor: String,
    pub width: f32,
    pub items: Vec<MenuItem>,
    pub mono: bool,
    /// Right-aligned to its anchor (the Presets menu); else left-aligned.
    pub align_end: bool,
    pub pick: Rc<dyn Fn(&mut Preview, usize)>,
}

/// OmConfirm: a message, Cancel and a destructive confirm button.
pub struct Confirm {
    pub message: String,
    pub cancel_text: String,
    pub confirm_text: String,
    pub confirmed: Rc<dyn Fn(&mut Preview)>,
}

/// The modal dialog that is open.
pub enum Modal {
    Export,
    SavePreset,
    DeletePreset,
    Color(color::Picker),
    Confirm(Confirm),
}

#[derive(Default)]
pub struct State {
    pub menu: Option<Menu>,
    pub modal: Option<Modal>,
    /// The export dialog's fields and the export it runs.
    pub export: export::State,
    /// The import [`Effect::StartImport`] started.
    import: Option<Receiver<ImportedInsert>>,
}

impl State {
    pub fn export_open(&self) -> bool {
        matches!(self.modal, Some(Modal::Export))
    }
}

/// Opens (or, when it is open, closes) the menu anchored at `anchor`.
pub fn toggle_menu(s: &mut Preview, anchor: &str, width: f32, items: Vec<MenuItem>, mono: bool, pick: Rc<dyn Fn(&mut Preview, usize)>) {
    if s.edit.dialogs.menu.as_ref().is_some_and(|m| m.anchor == anchor) {
        s.edit.dialogs.menu = None;
        return;
    }
    s.edit.dialogs.menu = Some(Menu { anchor: anchor.to_owned(), width, items, mono, align_end: false, pick });
}

/// A menu that hangs from the right edge of its anchor (TopBar's Presets).
pub fn toggle_menu_end(s: &mut Preview, anchor: &str, width: f32, items: Vec<MenuItem>, pick: Rc<dyn Fn(&mut Preview, usize)>) {
    toggle_menu(s, anchor, width, items, false, pick);
    if let Some(menu) = &mut s.edit.dialogs.menu {
        menu.align_end = true;
    }
}

pub fn open_modal(s: &mut Preview, modal: Modal) {
    s.edit.dialogs.menu = None;
    s.edit.dialogs.modal = Some(modal);
}

pub fn close_modal(s: &mut Preview) {
    s.edit.dialogs.modal = None;
}

/// OmConfirm over the window; `confirmed` runs on the confirm button.
#[allow(dead_code)]
pub fn confirm(s: &mut Preview, message: &str, confirm_text: &str, confirmed: impl Fn(&mut Preview) + 'static) {
    open_modal(
        s,
        Modal::Confirm(Confirm { message: message.to_owned(), cancel_text: "Cancel".into(), confirm_text: confirm_text.to_owned(), confirmed: Rc::new(confirmed) }),
    );
}

/// InsertClipDialog.qml: picks a video and inserts it at the playhead (`insertMedia`), unless
/// an import is running (BottomBar's `openInsertDialog`).
pub fn open_insert(s: &mut Preview, tx: &mut Tx) {
    if s.document.importing() {
        return;
    }
    let options = rune::FileDialogOptions { title: "Insert a video at the playhead".into(), ..Default::default() }
        .filter("Videos", &["mp4", "mov", "mkv", "webm", "m4v", "avi"])
        .filter("All files", &["*"]);
    tx.open_file(options, |s: &mut Preview, result, _tx| {
        if let Ok(Some(path)) = result {
            insert_media(s, &path.to_string_lossy());
        }
    });
}

/// `insertMedia(url)`: pauses, then starts the import, as the bridge does.
pub fn insert_media(s: &mut Preview, source: &str) {
    if !s.document.can_insert_media(source) {
        return;
    }
    if s.playing {
        s.toggle();
    }
    s.document.start_import(source);
    super::edited(s, "");
}

pub fn on_effect(s: &mut Preview, effect: &Effect) {
    match effect {
        Effect::StartImport(source) => {
            let job = s.document.import_job(source);
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(job.run());
            });
            s.edit.dialogs.import = Some(receive);
        }
        _ => export::on_effect(s, effect),
    }
}

pub fn poll(s: &mut Preview) -> bool {
    let mut changed = export::poll(s);
    if let Some(receive) = &s.edit.dialogs.import {
        match receive.try_recv() {
            Ok(imported) => {
                s.edit.dialogs.import = None;
                s.document.finish_import(imported);
                super::edited(s, "");
                changed = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => s.edit.dialogs.import = None,
        }
    }
    changed
}

pub fn busy(s: &Preview) -> bool {
    s.edit.dialogs.import.is_some() || export::busy(s)
}

/// An open dialog or menu takes the keys: Escape closes it (not a running export), the rest
/// stay away from the editor's shortcuts.
pub fn on_key(s: &mut Preview, key: &KeyEvent, _tx: &mut Tx) -> bool {
    if s.edit.dialogs.menu.is_some() {
        if matches!(key.key, Key::Escape) {
            s.edit.dialogs.menu = None;
        }
        return true;
    }
    if s.edit.dialogs.modal.is_none() {
        return false;
    }
    if matches!(key.key, Key::Escape) {
        dismiss(s);
    }
    true
}

/// Escape, the scrim or a close button: closes the dialog unless an export is running.
fn dismiss(s: &mut Preview) {
    if s.edit.dialogs.export_open() && s.document.export_state().exporting {
        return;
    }
    close_modal(s);
}

/// The open dialog and menu with their scrims, or None.
pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> Option<El> {
    if s.edit.dialogs.menu.is_none() && s.edit.dialogs.modal.is_none() {
        return None;
    }
    let ui = &s.ui;
    let mut layer = el().key("dialog-layer").absolute(0.0, 0.0).w_full().h_full();
    if let Some(modal) = &s.edit.dialogs.modal {
        let (scrim, card, width) = match modal {
            Modal::Export => (ui.canvas.alpha(0.65), export::view(s, cx), 460.0),
            Modal::SavePreset | Modal::DeletePreset => (ui.canvas.alpha(0.6), super::topbar::preset_dialog(s), 380.0),
            Modal::Color(picker) => (ui.canvas.alpha(0.6), color::view(s, picker), 360.0),
            Modal::Confirm(c) => (ui.menu_scrim, confirm_view(s, c), 370.0),
        };
        layer = layer
            .child(el().key("dialog-scrim").absolute(0.0, 0.0).w_full().h_full().fill(scrim).transition(rune::Transition::opacity()).a11y_hidden())
            .child(
                el().key("dialog-center")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .padding(16.0)
                    .center()
                    .child(popup_card(ui, card, width).key("dialog-card").role("dialog").modal(true, dismiss).transition(
                        rune::Transition::scale(0.96).combined(rune::Transition::opacity()),
                    )),
            );
    }
    if let Some(menu) = &s.edit.dialogs.menu {
        layer = layer.child(menu_view(s, menu));
    }
    Some(layer)
}

/// OmPopupCard: the raised surface of popovers, menus and dialogs.
pub fn popup_card(ui: &crate::theme::Ui, content: El, width: f32) -> El {
    let o = om();
    col()
        .w(width)
        .padding(o.popup_padding + 1.0)
        .radius(o.card_radius)
        .fill(ui.raised)
        .border(1.0, ui.stroke_strong)
        .shadow(8.0, 28.0, ui.shadow)
        .child(content)
}

/// OmMenu with OmMenuItems (and OmDropdown's list): rows `popupRowHeight + 2` high, a tick on
/// the checked row, destructive rows in the record red.
fn menu_view(s: &Preview, menu: &Menu) -> El {
    let ui = &s.ui;
    let o = om();
    let mut rows = col().w_full();
    for (i, item) in menu.items.iter().enumerate() {
        if item.separator {
            rows = rows.child(el().w_full().py(4.0).child(widgets::separator(ui)));
            continue;
        }
        let color = if !item.enabled {
            ui.text_faint
        } else if item.destructive {
            ui.record
        } else if item.checked {
            ui.text_strong
        } else {
            ui.foreground
        };
        let style = if menu.mono { widgets::mono(o.body, if item.checked { 500.0 } else { 400.0 }) } else { font(o.body, if item.checked { 500.0 } else { 400.0 }) };
        let mut r = row()
            .key(format!("menu-item-{i}"))
            .role("menuitem")
            .label(item.label.clone())
            .w_full()
            .h(o.row_h + 2.0)
            .px(10.0)
            .gap(8.0)
            .items_center()
            .radius(o.control_radius)
            .disabled(!item.enabled)
            .child(text(item.label.clone()).font(style).color(color).grow(1.0).truncate());
        if item.checked {
            r = r.child(rune::icon(&crate::icons::CHECK, 14.0).color(ui.accent));
        }
        if item.enabled {
            let pick = menu.pick.clone();
            r = r
                .hover(if item.destructive { ui.record.alpha(0.14) } else { ui.fill_hover })
                .press(ui.fill_pressed)
                .cursor(Cursor::Pointer)
                .on_press(move |s: &mut Preview| {
                    s.edit.dialogs.menu = None;
                    pick(s, i);
                });
        }
        rows = rows.child(r);
    }
    let surface = col()
        .key("menu")
        .role("menu")
        .w(menu.width)
        .padding(5.0)
        .radius(o.card_radius)
        .fill(ui.raised)
        .border(1.0, ui.stroke_strong)
        .shadow(8.0, 28.0, ui.shadow)
        .child(rows)
        .modal(true, |s: &mut Preview| s.edit.dialogs.menu = None)
        .anchored(menu.anchor.clone(), Side::Bottom, if menu.align_end { Align::End } else { Align::Start }, if menu.align_end { 6.0 } else { 4.0 })
        .transition(rune::Transition::scale(0.96).combined(rune::Transition::opacity()));
    // Outside clicks close it without reaching what is under it.
    el().key("menu-layer")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .child(el().key("menu-scrim").absolute(0.0, 0.0).w_full().h_full().on_press(|s: &mut Preview| s.edit.dialogs.menu = None))
        .child(surface)
}

/// OmConfirm's card.
fn confirm_view(s: &Preview, c: &Confirm) -> El {
    let ui = &s.ui;
    let o = om();
    let confirmed = c.confirmed.clone();
    col()
        .w_full()
        .gap(o.popup_padding)
        .child(text(c.message.clone()).font(font(o.title, 400.0)).color(ui.foreground).wrap().w_full())
        .child(
            row()
                .w_full()
                .gap(o.xl)
                .child(el().grow(1.0))
                .child(widgets::button(ui, "confirm-cancel", &c.cancel_text, None, Btn::bordered(), widgets::act(|s, _| close_modal(s))))
                .child(widgets::button(
                    ui,
                    "confirm-ok",
                    &c.confirm_text,
                    None,
                    Btn { bordered: true, primary: true, destructive: true, ..Btn::default() },
                    widgets::act(move |s, _| {
                        close_modal(s);
                        confirmed(s);
                    }),
                )),
        )
}

/// A heading and a muted line, as the preset dialogs open.
pub fn dialog_title(ui: &crate::theme::Ui, title: &str, detail: &str) -> El {
    let o = om();
    let mut c = col().gap(4.0).child(text(title.to_owned()).font(font(o.title, 600.0)).color(ui.text_strong));
    if !detail.is_empty() {
        c = c.child(text(detail.to_owned()).font(font(o.small, 400.0)).color(ui.text_muted));
    }
    c
}

/// A colour as a project value: "#rrggbb", or "#aarrggbb" when translucent.
pub fn color_value(c: &QColor) -> String {
    if c.alpha() < 255 { c.name_argb() } else { c.name() }
}
