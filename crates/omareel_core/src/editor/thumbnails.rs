//! Filmstrip frames (`videoThumbnailUrl`, `videoThumbnailUrlForClip`, `clipThumbnailUrl`,
//! `thumbnailSource`, `thumbnailUrl`, `startNextVideoThumbnail`, `buildVideoThumbnail`,
//! `cachedThumbnails`, `thumbnailCacheDirectory`), the camera strip (`buildCameraThumbnails`,
//! `startCameraThumbnailBuild`), and the wallpaper picker (`wallpaperGroups`).

use super::{Document, Effect};
use crate::compat::cpp;
use crate::compat::qdir::{self, FileInfo, Filters, SortFlags};
use crate::compat::qnumber;
use crate::compat::qprocess;
use crate::compat::qround::q_round;
use crate::compat::qurl;
use crate::omarchy_paths;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

/// How many filmstrip frames may wait to be built.
pub const THUMBNAIL_QUEUE_LIMIT: usize = 96;
/// How many filmstrip frames a cache directory keeps (the newest).
pub const THUMBNAIL_CACHE_LIMIT: usize = 512;
/// The camera strip's frame count.
pub const CAMERA_THUMBNAIL_COUNT: usize = 8;

/// `int(v)` as the compiled C++ converts a double: x86-64 gives `INT_MIN` out of range (and for
/// NaN); aarch64 saturates and makes NaN 0, as Rust's `as` does.
fn to_int(v: f64) -> i32 {
    if cfg!(any(target_arch = "x86_64", target_arch = "x86")) {
        if v > -2_147_483_649.0 && v < 2_147_483_648.0 { v as i32 } else { i32::MIN }
    } else {
        v as i32
    }
}

/// `qCeil`.
fn q_ceil(v: f64) -> i32 {
    to_int(v.ceil())
}

/// `qFloor`.
fn q_floor(v: f64) -> i32 {
    to_int(v.floor())
}

/// The 16-character fingerprint that keys a cache to this exact file (canonical path, size,
/// modification time in ms), so a re-recorded bundle never shows stale frames.
pub fn video_fingerprint(video_path: &str) -> String {
    let info = FileInfo::new(video_path);
    let identity = format!("{}|{}|{}", info.canonical_file_path(), info.size(), info.last_modified_msecs());
    qdir::sha256_hex(identity.as_bytes())[..16].to_owned()
}

/// `thumbnailCacheDirectory(videoPath, suffix)`: `<cache>/clip-filmstrips-v2/<fingerprint><suffix>`;
/// the suffix separates differently filtered frames of the same file.
pub fn thumbnail_cache_directory(cache_location: &str, video_path: &str, suffix: &str) -> String {
    qdir::file_path(cache_location, &format!("clip-filmstrips-v2/{}{suffix}", video_fingerprint(video_path)))
}

/// `startCameraThumbnailBuild`'s cache: `<cache>/camera-filmstrips/<fingerprint>`.
pub fn camera_thumbnail_cache_directory(cache_location: &str, video_path: &str) -> String {
    qdir::file_path(cache_location, &format!("camera-filmstrips/{}", video_fingerprint(video_path)))
}

/// A filmstrip frame's file name: its time in milliseconds, ten digits.
pub fn thumbnail_file_name(milliseconds: i32) -> String {
    format!("{milliseconds:010}.jpg")
}

const THUMBNAIL_FILTER: &str = "??????????.jpg";

/// `cachedThumbnails(cacheDirectory)`: the frames already built (named by their time, not
/// empty).
pub fn cached_thumbnails(cache_directory: &str) -> BTreeSet<i32> {
    qdir::entry_info_list(cache_directory, &[THUMBNAIL_FILTER], Filters::FILES, SortFlags::NO_SORT)
        .iter()
        .filter(|file| file.size() > 0)
        .filter_map(|file| qnumber::to_int(&file.complete_base_name()))
        .collect()
}

/// The `ffmpeg` arguments `buildVideoThumbnail` runs: the frame at `milliseconds`, filtered,
/// covering 160x90.
pub fn video_thumbnail_args(video_path: &str, filter: &str, output: &str, milliseconds: i32) -> Vec<String> {
    let scale = "scale=160:90:force_original_aspect_ratio=increase,crop=160:90";
    let vf = if filter.is_empty() { scale.to_owned() } else { format!("{filter},{scale}") };
    [
        "-v",
        "error",
        "-ss",
        &qnumber::number_f64(f64::from(milliseconds) / 1000.0, 'f', 3),
        "-i",
        video_path,
        "-frames:v",
        "1",
        "-an",
        "-vf",
        &vf,
        "-q:v",
        "4",
        "-y",
        output,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// `buildVideoThumbnail(videoPath, filter, path, milliseconds)` (off the GUI thread): ffmpeg
/// writes a temporary file beside `path` (20 s at most), which is renamed into place unless a
/// frame is already there, and the directory is trimmed to its newest 512 frames. As in Qt, the
/// rename never replaces a file, so an empty file at `path` makes it fail and leaves the
/// temporary behind.
pub fn build_video_thumbnail(video_path: &str, filter: &str, path: &str, milliseconds: i32) -> bool {
    let target = FileInfo::new(path);
    if !qdir::mkpath(&target.absolute_path()) {
        return false;
    }
    let temporary = format!("{path}.{}.tmp.jpg", std::process::id());
    let args = video_thumbnail_args(video_path, filter, &temporary, milliseconds);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(finished) = qprocess::run("ffmpeg", &args, 20000) else {
        let _ = std::fs::remove_file(&temporary);
        return false;
    };
    if finished.exit_code != 0 || FileInfo::new(temporary.as_str()).size() == 0 {
        let _ = std::fs::remove_file(&temporary);
        return false;
    }
    if FileInfo::new(path).size() > 0 {
        let _ = std::fs::remove_file(&temporary);
    } else if !super::bundle::rename_no_replace(&temporary, path) {
        return false;
    }
    trim_thumbnail_cache(&target.path());
    true
}

/// The newest-512 cleanup: `??????????.jpg` files by modification time, newest first, and every
/// one after the 512th removed.
pub fn trim_thumbnail_cache(directory: &str) {
    let cached = qdir::entry_info_list(directory, &[THUMBNAIL_FILTER], Filters::FILES, SortFlags::TIME);
    for file in cached.iter().skip(THUMBNAIL_CACHE_LIMIT) {
        let _ = std::fs::remove_file(file.absolute_file_path());
    }
}

/// The `ffmpeg` arguments for one camera strip frame at `timestamp` seconds, covering 112x72.
pub fn camera_thumbnail_args(video_path: &str, timestamp: f64, output: &str) -> Vec<String> {
    [
        "-v",
        "error",
        "-ss",
        &qnumber::number_f64(timestamp, 'f', 3),
        "-i",
        video_path,
        "-frames:v",
        "1",
        "-vf",
        "scale=112:72:force_original_aspect_ratio=increase,crop=112:72",
        "-q:v",
        "5",
        "-y",
        output,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// `buildCameraThumbnails(videoPath, duration, cacheDirectory)` (off the GUI thread): eight
/// frames from the middles of eight equal stretches, built where missing (15 s each), as file
/// URLs; none if the directory can't be made or a frame fails.
pub fn build_camera_thumbnails(video_path: &str, duration: f64, cache_directory: &str) -> Vec<String> {
    if !qdir::dir_exists(cache_directory) && !qdir::mkpath(&qdir::file_path(cache_directory, ".")) {
        return Vec::new();
    }
    let thumbnail_path =
        |index: usize| qdir::file_path(cache_directory, &format!("camera-{:02}.jpg", index + 1));
    let complete = (0..CAMERA_THUMBNAIL_COUNT).all(|index| FileInfo::new(thumbnail_path(index)).size() > 0);
    if !complete {
        for index in 0..CAMERA_THUMBNAIL_COUNT {
            if FileInfo::new(thumbnail_path(index)).size() > 0 {
                continue;
            }
            let timestamp = duration * (index as f64 + 0.5) / CAMERA_THUMBNAIL_COUNT as f64;
            let args = camera_thumbnail_args(video_path, timestamp, &thumbnail_path(index));
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            match qprocess::run("ffmpeg", &args, 15000) {
                Some(finished) if finished.exit_code == 0 => {}
                _ => return Vec::new(),
            }
        }
    }
    (0..CAMERA_THUMBNAIL_COUNT).map(|index| qurl::from_local_file(&thumbnail_path(index))).collect()
}

/// What `startCameraThumbnailBuild` runs off the GUI thread; its result is `cameraThumbnails`.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraThumbnailJob {
    pub video_path: String,
    pub duration: f64,
    pub cache_directory: String,
}

impl CameraThumbnailJob {
    pub fn run(&self) -> Vec<String> {
        build_camera_thumbnails(&self.video_path, self.duration, &self.cache_directory)
    }
}

/// One filmstrip frame for `startNextVideoThumbnail` to build off the GUI thread; give its
/// result to [`Document::finish_video_thumbnail`].
#[derive(Clone, Debug, PartialEq)]
pub struct ThumbnailJob {
    pub video_path: String,
    pub filter: String,
    pub path: String,
    pub milliseconds: i32,
}

impl ThumbnailJob {
    pub fn run(&self) -> bool {
        build_video_thumbnail(&self.video_path, &self.filter, &self.path, self.milliseconds)
    }
}

/// `Editor::ThumbnailSource`: a video the filmstrip samples frames from, with its own cache
/// directory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThumbnailSource {
    pub video_path: String,
    pub cache_directory: String,
    pub filter: String,
    pub duration: f64,
    pub fps: f64,
    pub pending: HashSet<i32>,
    pub failed: HashSet<i32>,
    pub ready: BTreeSet<i32>,
}

/// What `thumbnailSource` reads of the document to make a source.
pub(crate) struct ThumbnailContext {
    pub cache_location: String,
    pub video_path: String,
    pub source_duration: f64,
    pub fps: f64,
    pub camera: Option<(String, f64, f64)>,
}

#[derive(Debug, Default)]
pub(crate) struct Thumbnails {
    pub sources: BTreeMap<String, ThumbnailSource>,
    pub queue: VecDeque<(String, i32)>,
    pub running: bool,
    pub revision: i32,
}

impl Thumbnails {
    /// `thumbnailSource(key)` for the screen and camera keys ("screen", "camera-r<rotation>[f]");
    /// media keys need the document's probe and are made by `Document::thumbnail_source`.
    pub fn ensure_source(&mut self, context: &ThumbnailContext, key: &str) -> bool {
        if self.sources.contains_key(key) {
            return true;
        }
        let mut source = ThumbnailSource::default();
        if key == "screen" {
            source.video_path = context.video_path.clone();
            source.duration = context.source_duration;
            source.fps = context.fps;
            source.cache_directory = thumbnail_cache_directory(&context.cache_location, &context.video_path, "");
        } else if let (Some((camera_path, camera_duration, camera_fps)), Some(spec)) =
            (&context.camera, key.strip_prefix("camera-r"))
        {
            // Thumbnails show the camera the way the composition turns it: flipped first, then
            // rotated clockwise, matching CameraOverlay's transform order.
            let flipped = key.ends_with('f');
            let rotation = qnumber::to_int(spec.strip_suffix('f').filter(|_| flipped).unwrap_or(spec)).unwrap_or(0);
            let mut filters: Vec<&str> = Vec::new();
            if flipped {
                filters.push("hflip");
            }
            match rotation {
                90 => filters.push("transpose=clock"),
                180 => filters.push("hflip,vflip"),
                270 => filters.push("transpose=cclock"),
                _ => {}
            }
            source.video_path = camera_path.clone();
            source.filter = filters.join(",");
            source.duration = *camera_duration;
            source.fps = if *camera_fps > 0.0 { *camera_fps } else { 30.0 };
            source.cache_directory = thumbnail_cache_directory(&context.cache_location, camera_path, &format!("-{key}"));
        } else {
            return false;
        }
        self.insert(key, source);
        true
    }

    pub fn insert(&mut self, key: &str, mut source: ThumbnailSource) {
        source.ready = cached_thumbnails(&source.cache_directory);
        self.sources.insert(key.to_owned(), source);
    }
}

/// One wallpaper in the picker.
#[derive(Clone, Debug, PartialEq)]
pub struct Wallpaper {
    /// The file name without its suffix.
    pub name: String,
    pub file: String,
    pub path: String,
    pub url: String,
    /// The cached thumbnail's URL once it exists, the wallpaper's own until then.
    pub thumbnail_url: String,
}

/// One theme's wallpapers (`wallpaperGroups` entries: `name`, `label`, `current` and
/// `wallpapers`, each a map of the fields of [`Wallpaper`] with `thumbnailUrl`).
#[derive(Clone, Debug, PartialEq)]
pub struct WallpaperGroup {
    pub name: String,
    pub label: String,
    pub current: bool,
    pub wallpapers: Vec<Wallpaper>,
}

#[derive(Debug, Default)]
pub(crate) struct WallpaperCache {
    pub valid: bool,
    pub groups: Vec<WallpaperGroup>,
    pub pending: HashSet<String>,
}

impl Document {
    pub(crate) fn thumbnail_context(&self) -> ThumbnailContext {
        let camera = self.recording.camera.map(|camera| (self.camera_video_path.clone(), camera.duration, camera.fps));
        ThumbnailContext {
            cache_location: self.cache_location(),
            video_path: self.video_path.clone(),
            source_duration: self.recording.source_duration,
            fps: self.recording.fps,
            camera,
        }
    }

    /// `thumbnailSource(key)`: made on first use. An inserted video ("media:<path>") that does not
    /// probe has none.
    fn thumbnail_source(&mut self, key: &str) -> Option<&mut ThumbnailSource> {
        if !self.thumbnails.sources.contains_key(key) {
            if let Some(path) = key.strip_prefix("media:") {
                let info = self.insert_info(path);
                if !info.is_valid() {
                    return None;
                }
                let video_path = qdir::file_path(&self.recording.bundle_path, path);
                let source = ThumbnailSource {
                    cache_directory: thumbnail_cache_directory(&self.cache_location(), &video_path, ""),
                    video_path,
                    duration: info.duration,
                    fps: info.fps,
                    ..ThumbnailSource::default()
                };
                self.thumbnails.insert(key, source);
            } else {
                let context = self.thumbnail_context();
                if !self.thumbnails.ensure_source(&context, key) {
                    return None;
                }
            }
        }
        self.thumbnails.sources.get_mut(key)
    }

    /// `videoThumbnailRevision`: moves when a filmstrip frame was built or failed.
    pub fn video_thumbnail_revision(&self) -> i32 {
        self.thumbnails.revision
    }

    /// `videoThumbnailUrl(sourceTime)`: the screen's frame anywhere in the recording.
    pub fn video_thumbnail_url(&mut self, source_time: f64) -> String {
        self.video_thumbnail_url_for_clip(source_time, 0.0, self.recording.source_duration)
    }

    /// `videoThumbnailUrlForClip(sourceTime, start, end)`.
    pub fn video_thumbnail_url_for_clip(&mut self, source_time: f64, source_start: f64, source_end: f64) -> String {
        self.thumbnail_url("screen", source_time, source_start, source_end)
    }

    /// `cameraThumbnailKey()`: the camera turned as the project turns it.
    pub fn camera_thumbnail_key(&self) -> String {
        format!("camera-r{}{}", self.project.camera.rotation, if self.project.camera.flip_horizontal { "f" } else { "" })
    }

    /// `clipThumbnailUrl(clipId, sourceTime, start, end)`: an inserted clip shows its video, a
    /// camera-layout clip the webcam (held on its first frame before the camera started, as the
    /// preview holds it), any other the screen.
    pub fn clip_thumbnail_url(&mut self, clip_id: &str, source_time: f64, source_start: f64, source_end: f64) -> String {
        let index = self.clip_index(clip_id);
        if let Some(clip) = self.clip(index).filter(|clip| clip.is_media()) {
            let key = format!("media:{}", clip.media);
            return self.thumbnail_url(&key, source_time, source_start, source_end);
        }
        if index < 0 || !self.has_camera() || self.project.clips[index as usize].layout != "camera" {
            return self.video_thumbnail_url_for_clip(source_time, source_start, source_end);
        }
        let camera = self.camera();
        let camera_time = |screen_time: f64| cpp::clamp(screen_time - camera.offset, 0.0, camera.duration);
        let from = camera_time(source_start);
        let to = cpp::max(camera_time(source_end), cpp::min(camera.duration, from + 0.1));
        let from = cpp::min(from, cpp::max(0.0, to - 0.1));
        let key = self.camera_thumbnail_key();
        self.thumbnail_url(&key, camera_time(source_time), from, to)
    }

    /// `thumbnailUrl(key, time, start, end)`: the frame at `time` (to a tenth of a second) inside
    /// the span, if it is built; otherwise it is queued and the nearest built frame in the span
    /// stands in, or "" when there is none. The C++ took the first of two equally near frames in
    /// `QSet` order, which its random hash seed decides; here the earlier one.
    fn thumbnail_url(&mut self, key: &str, time: f64, start: f64, end: f64) -> String {
        if !time.is_finite() || !start.is_finite() || !end.is_finite() {
            return String::new();
        }
        let queue_length = self.thumbnails.queue.len();
        let Some(source) = self.thumbnail_source(key) else { return String::new() };
        let start = cpp::clamp(start, 0.0, source.duration);
        let end = cpp::clamp(end, start, source.duration);
        if end - start <= 0.001 {
            return String::new();
        }
        let first_sample = q_ceil(start * 1000.0);
        let tail_margin = cpp::min((end - start) / 2.0, 1.5 / cpp::max(1.0, source.fps));
        let last_sample = cpp::max(first_sample, q_floor((end - tail_margin) * 1000.0));
        let milliseconds = cpp::clamp(q_round(time * 10.0).wrapping_mul(100), first_sample, last_sample);
        let cache = source.cache_directory.clone();
        let path = qdir::file_path(&cache, &thumbnail_file_name(milliseconds));
        if FileInfo::new(path.as_str()).size() > 0 {
            source.ready.insert(milliseconds);
            return qurl::from_local_file(&path);
        }
        let mut enqueued = false;
        if !source.pending.contains(&milliseconds)
            && !source.failed.contains(&milliseconds)
            && queue_length < THUMBNAIL_QUEUE_LIMIT
        {
            source.pending.insert(milliseconds);
            enqueued = true;
        }
        let mut nearest: i32 = -1;
        for &ready in source.ready.range(first_sample..=last_sample) {
            if nearest < 0 || (ready - milliseconds).abs() < (nearest - milliseconds).abs() {
                nearest = ready;
            }
        }
        if enqueued {
            self.thumbnails.queue.push_back((key.to_owned(), milliseconds));
            self.emit(Effect::StartNextVideoThumbnail);
        }
        if nearest < 0 {
            return String::new();
        }
        qurl::from_local_file(&qdir::file_path(&cache, &thumbnail_file_name(nearest)))
    }

    /// `startNextVideoThumbnail()`: the next queued frame to build, unless playing, scrubbing,
    /// one is being built, or none is queued. Entries whose source is gone are dropped.
    pub fn next_video_thumbnail_job(&mut self, playing: bool, scrubbing: bool) -> Option<ThumbnailJob> {
        if playing || scrubbing || self.thumbnails.running {
            return None;
        }
        while let Some((key, milliseconds)) = self.thumbnails.queue.front().cloned() {
            let Some(source) = self.thumbnail_source(&key) else {
                self.thumbnails.queue.pop_front();
                continue;
            };
            let job = ThumbnailJob {
                video_path: source.video_path.clone(),
                filter: source.filter.clone(),
                path: qdir::file_path(&source.cache_directory, &thumbnail_file_name(milliseconds)),
                milliseconds,
            };
            self.thumbnails.running = true;
            return Some(job);
        }
        None
    }

    /// The end of a [`ThumbnailJob`]: the frame is ready (rescanning the directory once more
    /// than 512 are known, since the build trims it) or failed for good; the revision moves and
    /// the next frame may start.
    pub fn finish_video_thumbnail(&mut self, succeeded: bool) {
        self.thumbnails.running = false;
        if let Some((key, finished)) = self.thumbnails.queue.pop_front() {
            if let Some(source) = self.thumbnail_source(&key) {
                source.pending.remove(&finished);
                if !succeeded {
                    source.failed.insert(finished);
                } else {
                    source.ready.insert(finished);
                    // buildVideoThumbnail keeps the newest 512 files; resync once more are known.
                    if source.ready.len() > THUMBNAIL_CACHE_LIMIT {
                        source.ready = cached_thumbnails(&source.cache_directory);
                    }
                }
            }
        }
        self.thumbnails.revision = self.thumbnails.revision.wrapping_add(1);
        self.emit(Effect::VideoThumbnailRevisionChanged);
        self.emit(Effect::StartNextVideoThumbnail);
    }

    /// `startCameraThumbnailBuild()`: the camera strip to build off the GUI thread, if there is a
    /// camera.
    pub fn camera_thumbnail_job(&self) -> Option<CameraThumbnailJob> {
        let camera = self.recording.camera?;
        Some(CameraThumbnailJob {
            video_path: self.camera_video_path.clone(),
            duration: camera.duration,
            cache_directory: camera_thumbnail_cache_directory(&self.cache_location(), &self.camera_video_path),
        })
    }

    /// `wallpaperGroups`: every theme's wallpapers. Missing picker thumbnails are queued for
    /// generation once each ([`Effect::GenerateWallpaperThumbnail`]); the list is cached until one
    /// arrives or the theme refreshes.
    pub fn wallpaper_groups(&self) -> Vec<WallpaperGroup> {
        let mut cache = self.wallpapers.borrow_mut();
        if cache.valid {
            return cache.groups.clone();
        }
        let mut result = Vec::new();
        for group in omarchy_paths::theme_background_groups() {
            let mut wallpapers = Vec::new();
            for path in &group.paths {
                let file = FileInfo::new(path.as_str());
                let thumbnail = omarchy_paths::wallpaper_thumbnail_path(path);
                let ready = FileInfo::new(thumbnail.as_str()).is_file();
                wallpapers.push(Wallpaper {
                    name: file.complete_base_name(),
                    file: file.file_name(),
                    path: file.absolute_file_path(),
                    url: qurl::from_local_file(&file.absolute_file_path()),
                    thumbnail_url: qurl::from_local_file(if ready { &thumbnail } else { path }),
                });
                if !ready && !cache.pending.contains(&thumbnail) {
                    cache.pending.insert(thumbnail.clone());
                    self.emit(Effect::GenerateWallpaperThumbnail { path: path.clone(), thumbnail });
                }
            }
            result.push(WallpaperGroup { name: group.name, label: group.label, current: group.current, wallpapers });
        }
        cache.groups = result.clone();
        cache.valid = true;
        result
    }

    /// A picker thumbnail [`Effect::GenerateWallpaperThumbnail`] asked for was written (or
    /// failed).
    pub fn wallpaper_thumbnail_ready(&mut self, thumbnail: &str) {
        let mut cache = self.wallpapers.borrow_mut();
        cache.pending.remove(thumbnail);
        cache.valid = false;
        drop(cache);
        self.emit(Effect::WallpapersChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_arguments_match_editor_cpp() {
        assert_eq!(
            video_thumbnail_args("/b/screen.mp4", "", "/c/0000001300.jpg.7.tmp.jpg", 1300),
            [
                "-v", "error", "-ss", "1.300", "-i", "/b/screen.mp4", "-frames:v", "1", "-an", "-vf",
                "scale=160:90:force_original_aspect_ratio=increase,crop=160:90", "-q:v", "4", "-y",
                "/c/0000001300.jpg.7.tmp.jpg"
            ]
        );
        assert_eq!(video_thumbnail_args("v", "hflip,transpose=clock", "o", 5)[10], "hflip,transpose=clock,scale=160:90:force_original_aspect_ratio=increase,crop=160:90");
        assert_eq!(video_thumbnail_args("v", "", "o", 5)[3], "0.005");
        assert_eq!(
            camera_thumbnail_args("/b/camera.mp4", 0.125, "/c/camera-01.jpg"),
            [
                "-v", "error", "-ss", "0.125", "-i", "/b/camera.mp4", "-frames:v", "1", "-vf",
                "scale=112:72:force_original_aspect_ratio=increase,crop=112:72", "-q:v", "5", "-y", "/c/camera-01.jpg"
            ]
        );
        assert_eq!(thumbnail_file_name(1300), "0000001300.jpg");
        assert_eq!(thumbnail_file_name(0), "0000000000.jpg");
    }

    #[test]
    fn rounding_helpers_convert_like_cpp() {
        assert_eq!(q_ceil(1.0000001), 2);
        assert_eq!(q_floor(-0.5), -1);
        assert_eq!(q_ceil(-0.0), 0);
        if cfg!(target_arch = "x86_64") {
            assert_eq!(q_ceil(f64::NAN), i32::MIN);
            assert_eq!(q_floor(3e9), i32::MIN);
        }
    }

    #[test]
    fn cache_directories_follow_the_file() {
        let dir = crate::compat::qdir::tests::TempDir::new("editor-thumbs");
        let video = dir.join("screen.mp4");
        std::fs::write(&video, b"12345").unwrap();
        let info = FileInfo::new(video.as_str());
        let expected = qdir::sha256_hex(
            format!("{}|5|{}", info.canonical_file_path(), info.last_modified_msecs()).as_bytes(),
        )[..16]
            .to_owned();
        assert_eq!(thumbnail_cache_directory("/cache", &video, ""), format!("/cache/clip-filmstrips-v2/{expected}"));
        assert_eq!(
            thumbnail_cache_directory("/cache", &video, "-camera-r90f"),
            format!("/cache/clip-filmstrips-v2/{expected}-camera-r90f")
        );
        assert_eq!(camera_thumbnail_cache_directory("/cache/", &video), format!("/cache/camera-filmstrips/{expected}"));
        // A missing file still has a fingerprint: no path, size 0, time 0.
        assert_eq!(video_fingerprint("/nonexistent/x.mp4"), qdir::sha256_hex(b"|0|0")[..16]);
    }

    #[test]
    fn cache_scan_and_trim() {
        let dir = crate::compat::qdir::tests::TempDir::new("editor-thumb-cache");
        for (name, contents) in [
            ("0000000100.jpg", &b"x"[..]),
            ("0000000200.JPG", b"x"),
            ("0000000300.jpg", b""),
            ("000000040.jpg", b"x"),
            ("00000005x0.jpg", b"x"),
        ] {
            std::fs::write(dir.join(name), contents).unwrap();
        }
        assert_eq!(cached_thumbnails(&dir.0).into_iter().collect::<Vec<_>>(), [100, 200]);

        let many = crate::compat::qdir::tests::TempDir::new("editor-thumb-trim");
        let now = std::time::SystemTime::now();
        for i in 0..515 {
            let path = many.join(&thumbnail_file_name(i));
            std::fs::write(&path, b"x").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(now - std::time::Duration::from_secs(1000 - i as u64)).unwrap();
        }
        std::fs::write(many.join("keep.jpg"), b"x").unwrap();
        trim_thumbnail_cache(&many.0);
        let left = cached_thumbnails(&many.0);
        assert_eq!(left.len(), THUMBNAIL_CACHE_LIMIT);
        // The oldest three went.
        assert_eq!(left.iter().next(), Some(&3));
        assert!(FileInfo::new(many.join("keep.jpg")).is_file());
    }
}
