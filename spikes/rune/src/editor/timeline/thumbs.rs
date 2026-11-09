//! The filmstrips' frames and the camera row's strip: `Document::clip_thumbnail_url` queues a
//! frame it hasn't built (`Effect::StartNextVideoThumbnail`), workers build them with ffmpeg
//! (`ThumbnailJob`), and `finish_video_thumbnail` moves `video_thumbnail_revision`, after which
//! every tile asks again (QML binds the tiles' `source` to the revision).
//!
//! The view can't call the Document (it needs `&mut`), so it records the tiles it draws in
//! [`Thumbs::wanted`] and draws what [`Thumbs::urls`] last answered; poll asks the Document for
//! the wanted tiles whenever the revision moved or a tile is new.

use crate::preview::Preview;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};

/// One filmstrip tile: the clip, the source time (ms), and the span it shows (ms).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    pub clip: String,
    pub time: i64,
    pub from: i64,
    pub to: i64,
}

#[derive(Default)]
pub struct Thumbs {
    /// Tile → (the revision it was asked at, its file, "" for none yet).
    urls: RefCell<HashMap<Tile, (i32, String)>>,
    /// The tiles the last view drew.
    wanted: RefCell<Vec<Tile>>,
    /// The frames workers are building (several at once: `thumbnail_workers`).
    jobs: Vec<(omareel_core::editor::ThumbnailJob, Receiver<bool>)>,
    /// The camera strip: built once on open, again when the camera turns.
    pub camera: Vec<String>,
    camera_job: Option<Receiver<Vec<String>>>,
    camera_key: String,
    /// The decoded tiles the view draws, by file. Rune's cache only holds images weakly and an
    /// `El` keeps just the pixels, so a handle dropped after a view is decoded again, blank until
    /// done, on the next one: the strips flickered. Poll drops the ones no view wants any more.
    images: RefCell<HashMap<String, rune::Image>>,
}

/// `file:///a%20b.jpg` → `/a b.jpg`.
fn url_path(url: &str) -> String {
    let path = url.strip_prefix("file://").unwrap_or(url);
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&path[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Thumbs {
    /// The file to draw for a tile (possibly a neighbour's while its own is built), and notes
    /// that the view wants it.
    pub fn tile(&self, tile: Tile) -> Option<String> {
        let url = self.urls.borrow().get(&tile).map(|(_, u)| u.clone());
        self.wanted.borrow_mut().push(tile);
        url.filter(|u| !u.is_empty())
    }

    /// The decoded picture for a tile's or the camera strip's file, kept while the view shows it.
    pub fn image(&self, cache: &rune::ImageCache, path: &str) -> rune::Image {
        self.images.borrow_mut().entry(path.to_owned()).or_insert_with(|| cache.file(path, 0.0, 1.0)).clone()
    }

    /// Clears the wanted list before a view.
    pub fn begin_view(&self) {
        self.wanted.borrow_mut().clear();
    }

    pub fn busy(&self) -> bool {
        !self.jobs.is_empty() || self.camera_job.is_some()
    }
}

/// Answers the wanted tiles, lands finished frames and starts the next one. True when a tile's
/// picture changed.
pub fn poll(s: &mut Preview) -> bool {
    let mut changed = false;
    let mut index = 0;
    while index < s.edit.timeline.thumbs.jobs.len() {
        let result = s.edit.timeline.thumbs.jobs[index].1.try_recv();
        match result {
            Err(TryRecvError::Empty) => index += 1,
            Ok(ok) => {
                let (job, _) = s.edit.timeline.thumbs.jobs.remove(index);
                s.document.finish_video_thumbnail(&job, ok);
                super::super::handle_effects(s);
            }
            Err(TryRecvError::Disconnected) => {
                let (job, _) = s.edit.timeline.thumbs.jobs.remove(index);
                s.document.finish_video_thumbnail(&job, false);
            }
        }
    }
    if let Some(receive) = &s.edit.timeline.thumbs.camera_job {
        match receive.try_recv() {
            Ok(urls) => {
                s.edit.timeline.thumbs.camera_job = None;
                let paths: Vec<String> = urls.iter().map(|u| url_path(u)).collect();
                for path in &paths {
                    let image = s.edit.timeline.thumbs.image(&s.cache, path);
                    if rune::is_headless() {
                        let _ = image.wait();
                    }
                }
                s.edit.timeline.thumbs.camera = paths;
                changed = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => s.edit.timeline.thumbs.camera_job = None,
        }
    }
    let key = s.document.camera_thumbnail_key();
    if s.document.has_camera() && s.edit.timeline.thumbs.camera_key != key && s.edit.timeline.thumbs.camera_job.is_none() {
        s.edit.timeline.thumbs.camera_key = key;
        if let Some(job) = s.document.camera_thumbnail_job() {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(job.run());
            });
            s.edit.timeline.thumbs.camera_job = Some(receive);
        }
    }
    let revision = s.document.video_thumbnail_revision();
    let wanted: Vec<Tile> = s.edit.timeline.thumbs.wanted.borrow().clone();
    for tile in wanted {
        let known = s.edit.timeline.thumbs.urls.borrow().get(&tile).map(|(r, u)| (*r, u.clone()));
        if known.as_ref().is_some_and(|(r, _)| *r == revision) {
            continue;
        }
        let url = s.document.clip_thumbnail_url(&tile.clip, tile.time as f64 / 1000.0, tile.from as f64 / 1000.0, tile.to as f64 / 1000.0);
        let path = if url.is_empty() { String::new() } else { url_path(&url) };
        if known.as_ref().is_none_or(|(_, u)| *u != path) {
            changed = true;
            if !path.is_empty() {
                let image = s.edit.timeline.thumbs.image(&s.cache, &path);
                // Headless runs draw what is decoded at each frame: wait for the picture.
                if rune::is_headless() {
                    let _ = image.wait();
                }
            }
        }
        s.edit.timeline.thumbs.urls.borrow_mut().insert(tile, (revision, path));
    }
    // Keep only what the last view drew (or its neighbours' stand-ins) and the camera strip.
    {
        let thumbs = &s.edit.timeline.thumbs;
        let urls = thumbs.urls.borrow();
        let keep: std::collections::HashSet<String> = thumbs
            .wanted
            .borrow()
            .iter()
            .filter_map(|tile| urls.get(tile).map(|(_, u)| u.clone()))
            .chain(thumbs.camera.iter().cloned())
            .collect();
        thumbs.images.borrow_mut().retain(|path, _| keep.contains(path));
    }
    // Asking queued frames (Effect::StartNextVideoThumbnail).
    super::super::handle_effects(s);
    start_next(s);
    changed
}

/// `startNextVideoThumbnail()`: the queued frames, as many at once as the Document allows,
/// unless playing or scrubbing.
pub fn start_next(s: &mut Preview) {
    let (playing, scrubbing) = (s.playing, s.scrubbing);
    while let Some(job) = s.document.next_video_thumbnail_job(playing, scrubbing) {
        let (send, receive) = std::sync::mpsc::channel();
        let work = job.clone();
        std::thread::spawn(move || {
            let _ = send.send(work.run());
        });
        s.edit.timeline.thumbs.jobs.push((job, receive));
    }
}
