//! The editor's dialogs, drawn over the window: ExportDialog.qml (format, quality, size, where
//! to, progress, open the result), InsertClipDialog.qml and confirmations.

use crate::preview::Preview;
use omareel_core::editor::Effect;
use rune::{Cx, El, KeyEvent, Tx};

/// Which dialog is open and its fields.
#[derive(Default)]
pub struct State {
    pub export_open: bool,
}

/// TopBar's Export button.
pub fn open_export(s: &mut Preview) {
    s.edit.dialogs.export_open = true;
    super::edited(s, "Export: not ported yet");
}

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

/// The open dialog with its scrim, or None.
pub fn view(_s: &Preview, _cx: &mut Cx<Preview>) -> Option<El> {
    None
}
