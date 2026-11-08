//! SidePanel.qml: the rail of panel icons with its gliding indicator, the panel's title and
//! what it is for (OmHero), and the panel itself in a scroll view, sliding in from the side the
//! new one is on. The panels are in `inspector/`: Background, Shape, Cursor, Zoom, Clip, Camera,
//! Keystrokes, Audio (SidePanel's order). `widgets` holds the Om* components they share with the
//! top bar and the dialogs.

mod audio;
mod background;
mod camera;
mod clip;
mod cursor;
mod keystrokes;
mod shape;
pub mod widgets;
mod zoom;

use crate::preview::Preview;
use omareel_core::editor::{Effect, WallpaperGroup};
use rune::{Color, Cursor, Cx, El, KeyEvent, Tx, col, el};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use widgets::{hero, om};

/// The rail (56) and the panel (320), as Main.qml lays SidePanel out.
#[allow(dead_code)]
pub const INSPECTOR_W: f32 = 376.0;
const RAIL_W: f32 = 56.0;
const PANEL_W: f32 = 320.0;

/// SidePanel's `sections`: name, what it is for, icon.
const SECTIONS: [(&str, &str, &str); 8] = [
    ("Background", "Canvas behind the video", "palette"),
    ("Shape", "Frame, shadow, and border", "square"),
    ("Cursor", "Pointer, clicks, and motion", "mouse-pointer-2"),
    ("Zoom", "Focus and camera motion", "zoom-in"),
    ("Clip", "Trim and speed", "film"),
    ("Camera", "Webcam bubble", "camera"),
    ("Keystrokes", "Shortcut overlay", "keyboard"),
    ("Audio", "Tracks and volume", "volume-2"),
];

/// Two decoders, as the Qt bridge runs: one per core held a whole wallpaper's pixels each.
const THUMBNAIL_WORKERS: usize = 2;

#[derive(Default)]
struct ThumbnailQueue {
    jobs: VecDeque<(String, String)>,
    workers: usize,
}

/// The side panel's own UI state.
#[derive(Default)]
pub struct State {
    /// The panel shown (SidePanel's `section`).
    pub section: usize,
    /// Where the panel slides in from: +10 (a later panel) or -10.
    slide_from: f32,
    /// The control under the pointer and the one pressed, by key (widgets' hover looks).
    pub hot: Option<String>,
    pub pressed: Option<String>,
    /// BackgroundPanel's "show more" toggles.
    pub show_all_wallpapers: bool,
    pub show_all_gradients: bool,
    /// `wallpaperGroups`, read again after a thumbnail lands or the theme changes.
    wallpapers: Option<Vec<WallpaperGroup>>,
    /// The picker thumbnails' image handles, kept so Rune keeps them decoded.
    thumbs: RefCell<HashMap<String, rune::Image>>,
    thumbnail_queue: Arc<Mutex<ThumbnailQueue>>,
    thumbnail_done: Option<(Sender<String>, Receiver<String>)>,
    thumbnails_pending: usize,
}

/// Shows panel `index` (the timeline selects Zoom, Clip or Camera this way).
pub fn show_section(s: &mut Preview, index: usize) {
    let index = index.min(SECTIONS.len() - 1);
    let state = &mut s.edit.inspector;
    if state.section != index {
        state.slide_from = if index > state.section { 10.0 } else { -10.0 };
        state.section = index;
        state.hot = None;
    }
}

pub fn on_effect(s: &mut Preview, effect: &Effect) {
    match effect {
        // Connections { onSelectionChanged }: a selected zoom shows Zoom, a selected clip Clip.
        Effect::SelectionChanged => {
            if !s.document.selected_zoom_id().is_empty() {
                show_section(s, 3);
            } else if !s.document.selected_clip_id().is_empty() {
                show_section(s, 4);
            }
        }
        Effect::WallpapersChanged => s.edit.inspector.wallpapers = None,
        Effect::GenerateWallpaperThumbnail { path, thumbnail } => queue_thumbnail(s, path.clone(), thumbnail.clone()),
        _ => {}
    }
}

/// `generateWallpaperThumbnail`, off the UI thread: the wallpaper decoded, scaled to cover
/// 400 x 240, centre-cropped and saved as JPEG (quality 86), as EditorBase.cpp does.
fn queue_thumbnail(s: &mut Preview, path: String, thumbnail: String) {
    let state = &mut s.edit.inspector;
    let done = state.thumbnail_done.get_or_insert_with(std::sync::mpsc::channel).0.clone();
    state.thumbnails_pending += 1;
    let queue = state.thumbnail_queue.clone();
    let start = {
        let mut q = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        q.jobs.push_back((path, thumbnail));
        let start = q.workers < THUMBNAIL_WORKERS;
        if start {
            q.workers += 1;
        }
        start
    };
    if !start {
        return;
    }
    std::thread::spawn(move || {
        loop {
            let job = {
                let mut q = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let job = q.jobs.pop_front();
                if job.is_none() {
                    q.workers -= 1;
                }
                job
            };
            let Some((path, thumbnail)) = job else { break };
            if let Err(e) = generate_thumbnail(&path, &thumbnail) {
                eprintln!("wallpaper thumbnail {path}: {e}");
            }
            if done.send(thumbnail).is_err() {
                break;
            }
        }
    });
}

fn generate_thumbnail(path: &str, thumbnail: &str) -> Result<(), String> {
    if std::path::Path::new(thumbnail).is_file() {
        return Ok(());
    }
    let image = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?;
    let image = image.resize_to_fill(400, 240, image::imageops::FilterType::Triangle).to_rgb8();
    if let Some(dir) = std::path::Path::new(thumbnail).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let temporary = format!("{thumbnail}.{}.tmp", std::process::id());
    {
        let file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 86);
        encoder.encode_image(&image).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&temporary, thumbnail).map_err(|e| e.to_string())
}

pub fn poll(s: &mut Preview) -> bool {
    let mut changed = false;
    let mut landed = Vec::new();
    if let Some((_, receive)) = &s.edit.inspector.thumbnail_done {
        while let Ok(thumbnail) = receive.try_recv() {
            landed.push(thumbnail);
        }
    }
    for thumbnail in landed {
        s.edit.inspector.thumbnails_pending = s.edit.inspector.thumbnails_pending.saturating_sub(1);
        s.document.wallpaper_thumbnail_ready(&thumbnail);
        changed = true;
    }
    if changed {
        super::handle_effects(s);
    }
    if s.edit.inspector.section == 0 && s.edit.inspector.wallpapers.is_none() {
        // Reading the groups queues the missing thumbnails (GenerateWallpaperThumbnail).
        s.edit.inspector.wallpapers = Some(s.document.wallpaper_groups());
        super::handle_effects(s);
        changed = true;
    }
    changed
}

pub fn busy(s: &Preview) -> bool {
    let state = &s.edit.inspector;
    state.thumbnails_pending > 0 || (state.section == 0 && state.wallpapers.is_none()) || state.pressed.is_some()
}

pub fn on_key(_s: &mut Preview, _key: &KeyEvent, _tx: &mut Tx) -> bool {
    false
}

/// The wallpapers flattened (BackgroundPanel's `wallpapers`): theme label and the wallpaper.
pub(crate) fn wallpapers(s: &Preview) -> Vec<(String, omareel_core::editor::Wallpaper)> {
    s.edit.inspector.wallpapers.iter().flatten().flat_map(|g| g.wallpapers.iter().map(|w| (g.label.clone(), w.clone()))).collect()
}

/// A picker thumbnail's image, decoded once and kept.
pub(crate) fn thumbnail(s: &Preview, path: &str) -> rune::Image {
    let mut thumbs = s.edit.inspector.thumbs.borrow_mut();
    thumbs
        .entry(path.to_owned())
        .or_insert_with(|| {
            let image = s.cache.file(path, 0.0, 1.0);
            if rune::is_headless() {
                let _ = image.wait();
            }
            image
        })
        .clone()
}

pub fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    let ui = &s.ui;
    let o = om();
    let section = s.edit.inspector.section.min(SECTIONS.len() - 1);

    // The rail: one indicator glides between the sections.
    let mut rail = col()
        .key("rail")
        .w(RAIL_W)
        .h_full()
        .fill(ui.surface)
        .child(el().absolute(0.0, 0.0).w(1.0).h_full().fill(ui.hairline))
        .child(el().absolute(RAIL_W - 1.0, 0.0).w(1.0).h_full().fill(ui.hairline))
        .child(
            el().key("rail-indicator")
                .absolute(0.0, 10.0 + section as f32 * 40.0 + 9.0)
                .size(2.0, 18.0)
                .radius(1.0)
                .fill(ui.accent)
                .motion(rune::Spring::SNAPPY),
        );
    let mut buttons = col().w_full().pt(10.0).gap(4.0).items_center();
    for (i, (name, _, icon)) in SECTIONS.iter().enumerate() {
        let key = format!("railButton-{name}");
        let checked = i == section;
        let color = if checked {
            ui.accent
        } else if widgets::is_hot(s, &key) {
            ui.text_strong
        } else {
            ui.text_muted
        };
        let b = el()
            .key(key.clone())
            .role("tab")
            .label((*name).to_owned())
            .checked(checked)
            .size(36.0, 36.0)
            .radius(o.control_radius)
            .center()
            .fill(if checked { ui.fill_pressed } else { Color::TRANSPARENT })
            .hover(ui.fill_hover)
            .press(ui.fill_pressed)
            .press_scale(0.97)
            .cursor(Cursor::Pointer)
            .help((*name).to_owned())
            .on_press(move |s: &mut Preview| show_section(s, i))
            .child(rune::icon(crate::icons::named(icon).unwrap_or(&crate::icons::SQUARE), 17.0).color(color));
        buttons = buttons.child(widgets::track_hot(b, &key));
    }
    rail = rail.child(buttons);

    let content_w = PANEL_W - 2.0 * o.panel_padding;
    let panel = match section {
        0 => background::view(s, cx, content_w),
        1 => shape::view(s, content_w),
        2 => cursor::view(s, content_w),
        3 => zoom::view(s, content_w),
        4 => clip::view(s, content_w),
        5 => camera::view(s, content_w),
        6 => keystrokes::view(s, content_w),
        _ => audio::view(s, content_w),
    };
    let slide = s.edit.inspector.slide_from;
    let panel = col()
        .key(format!("panel-{}", SECTIONS[section].0))
        .w(content_w)
        .gap(o.panel_gap)
        .child(panel)
        .transition(rune::Transition::asymmetric(rune::Transition::offset(slide, 0.0).combined(rune::Transition::opacity()), rune::Transition::none()));
    let scroller = col()
        .key("inspector-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(col().w_full().pl(o.panel_padding).pt(16.0).pb(20.0 + o.panel_padding * 2.0 - 16.0).child(panel));
    let inspector = col()
        .key("inspector")
        .w(PANEL_W)
        .h_full()
        .fill(ui.surface)
        .child(el().w_full().px(o.panel_padding).pt(18.0).pb(4.0).child(hero(ui, SECTIONS[section].0, SECTIONS[section].1)))
        .child(scroller);
    rune::row().key("side-panel").w(INSPECTOR_W).h_full().fill(ui.surface).child(rail).child(inspector)
}
