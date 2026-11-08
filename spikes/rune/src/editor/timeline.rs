//! BottomBar.qml and Timeline.qml (with ClipTrack.qml, ZoomTrack.qml and OmTrimHandle.qml), and
//! the window's shortcuts from Main.qml.
//!
//! - `bar`: the bar under the preview: aspect and crop (or the canvas menu when narrow), the
//!   transport (position, go to start, play, go to end, duration), and the tools (insert,
//!   split, the timeline's scale, the shortcuts sheet).
//! - `tracks`: the timeline: the ruler (scrub, Shift-drag a playback range), the playhead, the
//!   clip track (filmstrips, trims, Ctrl-drag to slip, the speed and layout menu), the zoom
//!   track (double-click to add, drag to move, edges to resize, marquee to select) and the
//!   camera row; wheel zooms the timeline at the pointer, middle-drag pans it.
//! - `popup`: the menus and the shortcuts sheet, drawn over the window.
//! - `thumbs`: the filmstrips' frames and the camera strip, built by workers.
//!
//! The transport itself (play, pause, seek, scrub, step, the clock, decoders and sound) is in
//! `crate::preview`.

mod bar;
mod popup;
mod thumbs;
mod tracks;

pub use bar::view as bottom_bar;
pub use popup::overlay;

use super::edited;
use crate::preview::Preview;
use omareel_core::editor::Effect;
use rune::{Cx, El, Key, KeyEvent, Tx, col};
use std::cell::Cell;
use std::time::Duration;

/// An open pop-up.
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    /// The clip badge's speed (and layout) menu, for this clip.
    Speed(String),
    Aspect,
    /// The narrow bar's canvas menu (aspect and crop).
    Overflow,
    Shortcuts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
}

/// A pointer gesture in progress on the timeline. Positions are in the timeline's content
/// points (seconds × points per second, from the start of the tracks).
#[derive(Clone, Debug, PartialEq)]
pub enum Gesture {
    /// Dragging on the ruler, the clip track or the playhead: the playhead follows.
    Scrub,
    /// Shift-dragging a playback range, or moving one of its edges.
    Range,
    /// Ctrl-dragging a clip: it slides over its footage (`moveClip`) on release.
    ClipMove { id: String, press_block_x: f32, original_in: f64, speed: f64, x: f32, w: f32, dragged: bool, was_selected: bool },
    /// A clip's edge: the in (or out) point follows, committed on release.
    ClipTrim { id: String, edge: Edge, press_block_x: f32, press_w: f32, original: f64, preview: f64, x: f32, w: f32 },
    /// A zoom's body: the selection moves with it on release.
    ZoomMove { id: String, press_block_x: f32, x: f32, w: f32 },
    /// A zoom's edge.
    ZoomResize { id: String, edge: Edge, press_block_x: f32, press_right: f32, press_w: f32, x: f32, w: f32 },
    /// Dragging on the empty zoom track: a marquee that selects the zooms it covers.
    Marquee { origin: f32, current: f32, added: bool },
    /// Middle-dragging the tracks: they pan.
    Pan { from: f32 },
}

/// The timeline's own UI state.
pub struct State {
    /// `window.timelineScale`: 1 shows the whole timeline, up to 16.
    pub scale: f64,
    /// `flick.contentX`: how far the tracks are scrolled.
    pub content_x: f32,
    /// `window.cropMode`: the preview shows the crop handles.
    pub crop_mode: bool,
    pub popup: Option<Popup>,
    /// The camera row was clicked: the inspector shows its Camera panel
    /// (`Timeline.cameraSelected` → `sidePanel.section = 5`). The inspector takes it.
    pub camera_selected: bool,
    pub gesture: Option<Gesture>,
    /// The element under the pointer that changes how something looks (a zoom shows its
    /// handles, a badge its chevron, the playhead its time).
    pub hover: Option<String>,
    // The playback range being drawn (Timeline.qml's rangeAnchor, rangeHead and friends).
    range_anchor: f64,
    range_head: f64,
    selecting_range: bool,
    resizing_range: bool,
    resize_edge: Edge,
    resize_offset: f64,
    prior_range: (f64, f64),
    /// Laid-out sizes the handlers need (they get no layout): the tracks' viewport and the bar.
    pub flick_w: Cell<f32>,
    pub bar_width: Cell<f32>,
    pub thumbs: thumbs::Thumbs,
}

impl Default for State {
    fn default() -> State {
        State {
            scale: 1.0,
            content_x: 0.0,
            crop_mode: false,
            popup: None,
            camera_selected: false,
            gesture: None,
            hover: None,
            range_anchor: -1.0,
            range_head: -1.0,
            selecting_range: false,
            resizing_range: false,
            resize_edge: Edge::Left,
            resize_offset: 0.0,
            prior_range: (-1.0, -1.0),
            flick_w: Cell::new(1000.0),
            bar_width: Cell::new(1000.0),
            thumbs: thumbs::Thumbs::default(),
        }
    }
}

pub const LABEL_W: f32 = 72.0;
pub const PADDING: f32 = 40.0;

/// Theme radii (omareel_core's `Radii`): cards, controls and small parts.
pub fn card_radius(ui: &crate::theme::Ui) -> f32 {
    if ui.radius <= 0.0 { 0.0 } else { ui.radius.clamp(6.0, 12.0) }
}
pub fn control_radius(ui: &crate::theme::Ui) -> f32 {
    if ui.radius <= 0.0 { 0.0 } else { (ui.radius * 0.6).round().clamp(3.0, 8.0) }
}
pub fn small_radius(ui: &crate::theme::Ui) -> f32 {
    if ui.radius <= 0.0 { 0.0 } else { (ui.radius * 0.4).round().clamp(2.0, 5.0) }
}

impl State {
    pub fn range_start(&self) -> f64 {
        self.range_anchor.min(self.range_head)
    }
    pub fn range_end(&self) -> f64 {
        self.range_anchor.max(self.range_head)
    }
    /// `hasRange`.
    pub fn has_range(&self) -> bool {
        self.range_anchor >= 0.0 && self.range_head >= 0.0 && self.range_end() - self.range_start() > 0.000001
    }
}

/// Points per second at the timeline's scale (`basePixels * scaleFactor`).
pub fn pps(s: &Preview) -> f32 {
    let base = (s.edit.timeline.flick_w.get() - PADDING).max(1.0) / (s.document.duration().max(0.001) as f32);
    base * s.edit.timeline.scale as f32
}

/// The tracks' full width and how far they can scroll.
pub fn content_w(s: &Preview) -> f32 {
    (s.document.duration() as f32 * pps(s) + PADDING).max(s.edit.timeline.flick_w.get())
}

pub fn content_x(s: &Preview) -> f32 {
    s.edit.timeline.content_x.clamp(0.0, (content_w(s) - s.edit.timeline.flick_w.get()).max(0.0))
}

/// `zoomBy` and the scale slider: a new scale, keeping the time under `anchor_x` (points from
/// the tracks' left edge) where it is.
pub fn set_scale(s: &mut Preview, scale: f64, anchor_x: f32) {
    let next = scale.clamp(1.0, 16.0);
    if (next - s.edit.timeline.scale).abs() < 0.000001 {
        return;
    }
    let anchor_x = anchor_x.clamp(0.0, s.edit.timeline.flick_w.get());
    let time = (content_x(s) + anchor_x) / pps(s);
    s.edit.timeline.scale = next;
    let max = (content_w(s) - s.edit.timeline.flick_w.get()).max(0.0);
    s.edit.timeline.content_x = (time * pps(s) - anchor_x).clamp(0.0, max);
}

// ---------------------------------------------------------------------------------------------
// The playback range (Timeline.qml's functions of the same names)

pub fn clear_range(s: &mut Preview) {
    let t = &mut s.edit.timeline;
    t.selecting_range = false;
    t.resizing_range = false;
    t.range_anchor = -1.0;
    t.range_head = -1.0;
    s.document.clear_playback_range();
}

pub fn begin_range(s: &mut Preview, time: f64) {
    s.begin_scrub();
    s.document.set_selected_clip_id("");
    s.document.clear_playback_range();
    let time = time.clamp(0.0, s.document.duration());
    let t = &mut s.edit.timeline;
    t.range_anchor = time;
    t.range_head = time;
    t.selecting_range = true;
    t.resizing_range = false;
    s.edit.timeline.gesture = Some(Gesture::Range);
}

pub fn begin_range_resize(s: &mut Preview, edge: Edge, pointer_time: f64) {
    if !s.edit.timeline.has_range() {
        return;
    }
    s.begin_scrub();
    let t = &mut s.edit.timeline;
    t.prior_range = (t.range_start(), t.range_end());
    t.resize_edge = edge;
    t.resize_offset = (if edge == Edge::Left { t.prior_range.0 } else { t.prior_range.1 }) - pointer_time;
    t.resizing_range = true;
    t.selecting_range = true;
    t.gesture = Some(Gesture::Range);
}

pub fn update_range(s: &mut Preview, time: f64) {
    if !s.edit.timeline.selecting_range {
        return;
    }
    let duration = s.document.duration();
    let t = &mut s.edit.timeline;
    t.range_head = time.clamp(0.0, duration);
    if t.resizing_range {
        t.range_anchor = if t.resize_edge == Edge::Left { t.prior_range.1 } else { t.prior_range.0 };
    }
    if t.has_range() {
        let (a, b) = (t.range_start(), t.range_end());
        s.document.set_playback_range(a, b);
    } else {
        s.document.clear_playback_range();
    }
    let head = s.edit.timeline.range_head;
    s.scrub_to(head);
}

pub fn finish_range(s: &mut Preview, time: f64) {
    update_range(s, time);
    s.edit.timeline.selecting_range = false;
    s.edit.timeline.gesture = None;
    let t = &s.edit.timeline;
    if ((t.range_end() - t.range_start()) as f32) * pps(s) < 2.0 {
        clear_range(s);
        let p = s.document.output_position();
        s.seek(p);
    } else {
        let (a, b, head, resizing) = (t.range_start(), t.range_end(), t.range_head, t.resizing_range);
        s.document.set_playback_range(a, b);
        s.seek(if resizing { head } else { a });
    }
    s.edit.timeline.resizing_range = false;
}

pub fn cancel_range(s: &mut Preview) {
    s.edit.timeline.gesture = None;
    if s.edit.timeline.resizing_range {
        let t = &mut s.edit.timeline;
        t.range_anchor = t.prior_range.0;
        t.range_head = t.prior_range.1;
        t.selecting_range = false;
        t.resizing_range = false;
        let (a, b) = (t.range_start(), t.range_end());
        s.document.set_playback_range(a, b);
        s.seek(a);
    } else {
        clear_range(s);
        let p = s.document.output_position();
        s.seek(p);
    }
}

pub fn delete_selected_range(s: &mut Preview) -> bool {
    let t = &s.edit.timeline;
    if !t.has_range() {
        return false;
    }
    let (a, b) = (t.range_start(), t.range_end());
    // Editor::deleteOutputRange pauses first.
    s.pause();
    if !s.document.delete_output_range(a, b) {
        return false;
    }
    clear_range(s);
    edited(s, "Delete range");
    true
}

// ---------------------------------------------------------------------------------------------
// Hooks

/// BottomBar.openInsertDialog: the insert dialog is the dialogs area's; until it has an
/// `open_insert`, say so.
pub fn open_insert_dialog(s: &mut Preview) {
    if s.document.importing() {
        return;
    }
    s.edit.toast = Some(("Insert video: the dialog isn't ported yet".into(), s.now));
}

pub fn on_effect(s: &mut Preview, effect: &Effect) {
    match effect {
        Effect::DurationChanged => clear_range(s),
        Effect::StartNextVideoThumbnail => thumbs::start_next(s),
        Effect::SetScreenAudio { .. } | Effect::UpdateInsertAudio | Effect::ProjectChanged => s.audio_edited(),
        _ => {}
    }
}

pub fn poll(s: &mut Preview) -> bool {
    thumbs::poll(s)
}

pub fn busy(s: &Preview) -> bool {
    s.edit.timeline.gesture.is_some() || s.edit.timeline.thumbs.busy()
}

/// Main.qml's Shortcuts (Space and the arrows are the preview's; Ctrl+Z, Ctrl+Shift+Z and Ctrl+S
/// the editor's).
pub fn on_key(s: &mut Preview, key: &KeyEvent, tx: &mut Tx) -> bool {
    let plain = !key.mods.cmd && !key.mods.ctrl && !key.mods.alt;
    let handled = match &key.key {
        Key::Text(t) if plain && t == "s" => {
            if s.document.split_at_playhead() {
                edited(s, "Split clip");
            }
            true
        }
        Key::Text(t) if plain && t == "z" => {
            let at = s.document.output_position();
            if !s.document.add_zoom_at(at, 2.0).is_empty() {
                edited(s, "Add zoom");
            }
            true
        }
        Key::Text(t) if plain && t == "i" => {
            open_insert_dialog(s);
            true
        }
        Key::Text(t) if plain && t == "?" => {
            s.edit.timeline.popup = if s.edit.timeline.popup == Some(Popup::Shortcuts) { None } else { Some(Popup::Shortcuts) };
            true
        }
        Key::Command('e') => {
            super::dialogs::open_export(s);
            true
        }
        Key::Delete if plain => {
            if s.edit.timeline.has_range() {
                delete_selected_range(s);
            } else if !s.document.selected_zoom_ids().is_empty() {
                if s.document.remove_selected_zooms() > 0 {
                    edited(s, "Delete zoom");
                }
            } else {
                let id = s.document.selected_clip_id().to_owned();
                if s.document.remove_clip(&id) {
                    edited(s, "Delete clip");
                }
            }
            true
        }
        Key::Backspace if plain && s.edit.timeline.has_range() => {
            delete_selected_range(s);
            true
        }
        Key::Escape if s.edit.timeline.popup.is_some() => {
            s.edit.timeline.popup = None;
            true
        }
        Key::Escape => {
            if s.document.picking_zoom_target() {
                s.document.set_picking_zoom_target(false);
                edited(s, "");
                true
            } else if s.edit.timeline.has_range() {
                clear_range(s);
                edited(s, "");
                true
            } else if !s.document.selected_clip_id().is_empty() || !s.document.selected_zoom_id().is_empty() {
                s.document.set_selected_clip_id("");
                edited(s, "");
                true
            } else {
                false
            }
        }
        _ => false,
    };
    if handled {
        tx.wake_after(Duration::ZERO);
    }
    handled
}

/// The timeline (the bar is placed under the preview by the editor's layout).
pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    s.edit.timeline.thumbs.begin_view();
    col().key("timeline-area").w_full().child(tracks::view(s, cx))
}
