//! omareel's editor in Rune (`edit <bundle>`): the port of `Main.qml` and everything under it,
//! over omareel_core's `Document` (what `crates/omareel_qt/src/editor.rs` exposes to QML).
//!
//! The window is split by area, each in its own file with its own state:
//! - `topbar`: TopBar.qml (bundle name, presets, undo/redo, export button).
//! - `inspector`: SidePanel.qml and its panels (Background, Frame/Shape, Cursor, Camera, Zoom,
//!   Keystrokes, Audio, Clip).
//! - `timeline`: BottomBar.qml (transport and tools) and Timeline.qml with ClipTrack, ZoomTrack
//!   and the keys row.
//! - `dialogs`: ExportDialog.qml, InsertClipDialog.qml and confirmations, drawn over the window.
//! - The preview itself is `crate::preview::pane` (PreviewPane.qml); playback, decoding and the
//!   composition live in `crate::preview` and `crate::comp`.
//!
//! This file holds what they share: [`EditState`], [`edited`] (call it after every Document edit),
//! the Document's side effects ([`handle_effects`]), autosave, global keys and the layout.

pub mod dialogs;
pub mod inspector;
pub mod timeline;
pub mod topbar;

use crate::preview::Preview;
use omareel_core::compat::qjson::Value;
use omareel_core::editor::Effect;
use rune::{Cx, El, Key, KeyEvent, Tx, col, row, text};
use std::time::Duration;

pub use crate::kit::{label, mono};

/// The editor's UI state beside the Document: one part per area (each area's file defines and
/// owns its `State`), the toast and autosave.
#[derive(Default)]
pub struct EditState {
    pub topbar: topbar::State,
    pub inspector: inspector::State,
    pub timeline: timeline::State,
    pub dialogs: dialogs::State,
    /// The last edit's description and when (poll's clock), for the toast.
    pub toast: Option<(String, f64)>,
    /// When the pending autosave is due (poll's clock): the Document asks for one 500 ms after
    /// an edit (`Effect::StartAutosaveTimer`).
    pub autosave_at: Option<f64>,
    /// The background image the preview shows, to notice when the project changes it.
    pub background_path: String,
    /// The last error the Document reported (`Effect::ErrorOccurred`), shown as a toast.
    pub error: Option<String>,
}

pub fn kit(ui: &crate::theme::Ui) -> rune_kit::Kit {
    crate::kit::rune_kit(ui)
}

// ---------------------------------------------------------------------------------------------
// Edits and effects

/// After any Document edit: run its side effects, refresh what the composition reads, and show
/// `what` in the toast (empty for none).
pub fn edited(s: &mut Preview, what: &str) {
    handle_effects(s);
    refresh_settings(s);
    if !what.is_empty() {
        s.edit.toast = Some((what.to_owned(), s.now));
    }
}

/// `set_project_value` and [`edited`].
pub fn set(s: &mut Preview, path: &str, value: Value, coalesce: bool, what: &str) {
    s.document.set_project_value(path, value, coalesce);
    edited(s, what);
}

/// The composition's settings from the project, and the background image when it changed.
pub fn refresh_settings(s: &mut Preview) {
    let camera_available = s.settings.camera_available;
    s.settings = crate::comp::Settings::new(
        s.document.project(),
        (s.document.source_width(), s.document.source_height()),
        s.document.capture_scale(),
        s.document.has_camera().then(|| (s.document.camera_source_width(), s.document.camera_source_height())),
    );
    s.settings.camera_available = camera_available;
    let path = background_path(&s.document);
    if path != s.edit.background_path {
        s.edit.background_path = path.clone();
        s.media.background = (!path.is_empty()).then(|| s.cache.file(&path, 0.0, 1.0));
    }
}

/// The image file behind the project's background ("" for a colour or gradient).
pub fn background_path(document: &omareel_core::editor::Document) -> String {
    let b = &document.project().background;
    match b.type_.as_str() {
        "image" => b.image.clone(),
        "wallpaper" if b.wallpaper == "omarchy:current" => omareel_core::omarchy_paths::current_background(),
        "wallpaper" => b.wallpaper.clone(),
        _ => String::new(),
    }
}

/// The Document's queued side effects (what `Editor::apply_effects` does in the Qt bridge). The
/// ones every area shares are handled here; each is then offered to every area's `on_effect`
/// (exports, imports, thumbnails, wallpapers are theirs).
pub fn handle_effects(s: &mut Preview) {
    for effect in s.document.take_effects() {
        match &effect {
            Effect::StartMotionTimer => {
                if s.motion.is_none()
                    && let Some(job) = s.document.motion_job()
                {
                    let (send, receive) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = send.send(job.run());
                    });
                    s.motion = Some(receive);
                }
            }
            Effect::StartAutosaveTimer => s.edit.autosave_at = Some(s.now + 0.5),
            Effect::Seek(t) => s.seek(*t),
            Effect::Pause => {
                if s.playing {
                    s.toggle();
                }
            }
            Effect::ErrorOccurred(message) => {
                s.edit.error = Some(message.clone());
                s.edit.toast = Some((message.clone(), s.now));
            }
            _ => {}
        }
        topbar::on_effect(s, &effect);
        inspector::on_effect(s, &effect);
        timeline::on_effect(s, &effect);
        dialogs::on_effect(s, &effect);
    }
}

/// Called from the preview's poll each frame: a finished motion track lands, autosave fires, and
/// each area's own poll runs (exports, imports, thumbnails). True when the view changed.
pub fn poll(s: &mut Preview) -> bool {
    let mut changed = false;
    if let Some(receive) = &s.motion {
        match receive.try_recv() {
            Ok(track) => {
                s.motion = None;
                s.document.apply_motion(track);
                edited(s, "");
                changed = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => s.motion = None,
        }
    }
    if let Some(receive) = &s.frame_times {
        match receive.try_recv() {
            Ok(times) => {
                s.frame_times = None;
                s.document.set_screen_frame_times(times);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => s.frame_times = None,
        }
    }
    if s.edit.autosave_at.is_some_and(|at| s.now >= at) {
        s.edit.autosave_at = None;
        s.document.save_now();
        handle_effects(s);
        changed = true;
    }
    changed |= topbar::poll(s);
    changed |= inspector::poll(s);
    changed |= timeline::poll(s);
    changed |= dialogs::poll(s);
    changed
}

/// True while something an area runs needs frames (an export's progress, a drag), so poll keeps
/// being called.
pub fn busy(s: &Preview) -> bool {
    s.edit.autosave_at.is_some() || s.motion.is_some() || s.frame_times.is_some() || dialogs::busy(s) || inspector::busy(s) || timeline::busy(s) || topbar::busy(s)
}

/// The window's keys (Main.qml's Shortcuts). An open dialog takes keys first, then the areas;
/// what none of them takes falls to the preview (Space, arrows).
pub fn on_key(s: &mut Preview, key: &KeyEvent, tx: &mut Tx) -> bool {
    if dialogs::on_key(s, key, tx) || timeline::on_key(s, key, tx) || inspector::on_key(s, key, tx) || topbar::on_key(s, key, tx) {
        return true;
    }
    match &key.key {
        Key::Command('z') if key.mods.shift => {
            s.document.redo();
            edited(s, "Redo");
            true
        }
        Key::Command('z') => {
            s.document.undo();
            edited(s, "Undo");
            true
        }
        Key::Command('s') => {
            s.document.save_now();
            edited(s, "Saved");
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// The window

pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    // Main.qml: the bottom bar sits under the preview, beside the inspector; the timeline runs
    // the window's width below them.
    let preview_column = col()
        .key("preview-column")
        .grow(1.0)
        .min_w(528.0)
        .h_full()
        .child({
            // Rune draws the camera bubble (a rounded, shadowed video surface) above a translucent
            // scrim drawn later, so under a dialog the preview dims itself instead of relying on
            // the scrim.
            let pane = crate::preview::pane(s, cx, 64.0, 56.0);
            if s.edit.dialogs.modal.is_some() { pane.dim(0.35) } else { pane }
        })
        .child(timeline::bottom_bar(s, cx));
    let middle = row().key("middle").w_full().grow(1.0).min_h(0.0).child(preview_column).child(inspector::view(s, cx));
    let mut window = col()
        .w_full()
        .h_full()
        .fill(ui.canvas)
        .child(topbar::view(s, cx))
        .child(middle)
        .child(timeline::view(s, cx))
        .child(toast(s));
    if let Some(layer) = timeline::overlay(s, cx) {
        window = window.child(layer);
    }
    if let Some(layer) = dialogs::view(s, cx) {
        window = window.child(layer);
    }
    window
}

/// The last edit, in a pill that springs up from the bottom and fades after two seconds.
fn toast(s: &Preview) -> El {
    let ui = &s.ui;
    let showing = s.edit.toast.as_ref().filter(|(_, at)| s.now - at < 2.0);
    let mut layer = col().key("toast-layer").absolute(0.0, 0.0).w_full().h_full().inert().items_center().justify_end().pb(250.0);
    if let Some((what, at)) = showing {
        layer = layer.child(
            row()
                .key(format!("toast-{at}"))
                .h(32.0)
                .px(14.0)
                .items_center()
                .radius(16.0)
                .fill(ui.raised)
                .border(1.0, ui.hairline)
                .shadow(6.0, 18.0, ui.shadow)
                .transition(rune::Transition::offset(0.0, 12.0).combined(rune::Transition::opacity()))
                .child(text(what.clone()).font(label(13.0, 500.0)).color(ui.text_strong)),
        );
    }
    layer
}

/// Keeps poll running while a toast is up, so it fades.
pub fn toast_showing(s: &Preview) -> bool {
    s.edit.toast.as_ref().is_some_and(|(_, at)| s.now - at < 2.2)
}

#[allow(dead_code)]
const WAKE: Duration = Duration::ZERO;
