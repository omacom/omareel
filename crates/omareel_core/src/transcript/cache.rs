//! The speech measurement (`vad::speech_regions`) and the screen check of its silences
//! (`stillness::check_windows`) kept between editor runs: both read the whole recording, which
//! takes minutes for a long 4K one, and neither changes unless screen.mp4 does.
//!
//! They live in the user's cache, `<CacheLocation>/speech-v1/` (`~/.cache/omareel/omareel/...`),
//! never in the bundle, so bundles stay clean. An entry is keyed by the video's identity: its
//! size, its modification time to the nanosecond, and a SHA-256 of its first MiB. Any change to
//! the file gives another key, so a stale entry is never read; writing an entry removes the
//! older ones for the same path. Two files per recording, because the two measurements finish
//! on different threads: `<key>.speech.json` and `<key>.screen.json`.
//!
//! ```json
//! {"version": 1, "video": "/abs/screen.mp4", "speech": [[0.52, 3.1], [4.0, 9.75]]}
//! {"version": 1, "video": "/abs/screen.mp4", "geometry": [3840, 2160, 2],
//!  "windows": [[3.1, 4.0]], "still": [], "moving": [[3.1, 4.0]]}
//! ```
//! A screen entry only counts for the same windows (the silences of the same speech) and the
//! same geometry; the version changes whenever the measurements would come out differently.

use super::stillness::ScreenCheck;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::{qdir, qsavefile};
use std::io::Read;
use std::time::UNIX_EPOCH;

/// Bumped when the speech finder or the screen check changes what they find.
pub const VERSION: i32 = 1;
/// The directory under the cache location.
pub const DIRECTORY: &str = "speech-v1";
/// How much of the video's start is hashed.
const HEAD: u64 = 1 << 20;

/// The cache key of `video` as it is now; `None` when it cannot be read.
pub fn identity(video: &str) -> Option<String> {
    let metadata = std::fs::metadata(video).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let mut head = Vec::new();
    std::fs::File::open(video).ok()?.take(HEAD).read_to_end(&mut head).ok()?;
    let key = format!("{}|{}|{}", metadata.len(), modified, qdir::sha256_hex(&head));
    Some(qdir::sha256_hex(key.as_bytes())[..32].to_owned())
}

/// The cache's directory under `cache_location` (`QStandardPaths::CacheLocation`).
pub fn directory(cache_location: &str) -> String {
    qdir::file_path(cache_location, DIRECTORY)
}

fn ranges_to_json(ranges: &[(f64, f64)]) -> Value {
    Value::array(ranges.iter().map(|&(a, b)| Value::array(vec![Value::from_f64(a), Value::from_f64(b)])).collect())
}

fn ranges_from_json(value: &Value) -> Option<Vec<(f64, f64)>> {
    let Value::Array(items) = value else { return None };
    let mut ranges = Vec::with_capacity(items.len());
    for item in items {
        let pair = item.to_array();
        let (a, b) = (pair.first()?.to_double(f64::NAN), pair.get(1)?.to_double(f64::NAN));
        if pair.len() != 2 || !a.is_finite() || !b.is_finite() {
            return None;
        }
        ranges.push((a, b));
    }
    Some(ranges)
}

fn same_ranges(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x.0 - y.0).abs() < 1e-6 && (x.1 - y.1).abs() < 1e-6)
}

/// A recording's entries in a cache directory.
pub struct Entry {
    directory: String,
    key: String,
    video: String,
}

impl Entry {
    /// The entry for `video` as it is now, in the cache under `cache_location`; `None` when the
    /// video cannot be read.
    pub fn new(cache_location: &str, video: &str) -> Option<Entry> {
        let key = identity(video)?;
        Some(Entry { directory: directory(cache_location), key, video: qdir::absolute_path(video) })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    fn path(&self, kind: &str) -> String {
        qdir::file_path(&self.directory, &format!("{}.{kind}.json", self.key))
    }

    fn read(&self, kind: &str) -> Option<Object> {
        let bytes = std::fs::read(self.path(kind)).ok()?;
        let object = qjson::object_from_json(&bytes)?;
        (object.value("version").to_int(0) == VERSION).then_some(object)
    }

    fn write(&self, kind: &str, members: Vec<(&str, Value)>) -> Result<(), String> {
        if !qdir::mkpath(&self.directory) {
            return Err(format!("could not make {}", self.directory));
        }
        let mut object = Object::from_iter([("version", Value::from(VERSION)), ("video", Value::from(self.video.as_str()))]);
        for (key, value) in members {
            object.insert(key, value);
        }
        qsavefile::save(&self.path(kind), object.to_json(Format::Compact).as_bytes())?;
        self.remove_older(kind);
        Ok(())
    }

    /// Removes this kind's entries for the same video under other keys: the video changed.
    fn remove_older(&self, kind: &str) {
        let Ok(entries) = std::fs::read_dir(&self.directory) else { return };
        let suffix = format!(".{kind}.json");
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(&suffix) || name.starts_with(&self.key) {
                continue;
            }
            let same_video = std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| qjson::object_from_json(&bytes))
                .is_some_and(|object| object.value("video").to_string_or("") == self.video);
            if same_video {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// The speech measured before, if any.
    pub fn speech(&self) -> Option<Vec<(f64, f64)>> {
        ranges_from_json(self.read("speech")?.value("speech"))
    }

    pub fn store_speech(&self, speech: &[(f64, f64)]) -> Result<(), String> {
        self.write("speech", vec![("speech", ranges_to_json(speech))])
    }

    /// The screen check of exactly `windows` at `geometry` (width, height, scale), if it was made.
    pub fn screen(&self, windows: &[(f64, f64)], geometry: (i32, i32, f64)) -> Option<ScreenCheck> {
        let object = self.read("screen")?;
        let stored = object.value("geometry").to_array();
        let same_geometry = stored.len() == 3
            && stored[0].to_int(-1) == geometry.0
            && stored[1].to_int(-1) == geometry.1
            && (stored[2].to_double(f64::NAN) - geometry.2).abs() < 1e-9;
        if !same_geometry || !same_ranges(&ranges_from_json(object.value("windows"))?, windows) {
            return None;
        }
        Some(ScreenCheck { still: ranges_from_json(object.value("still"))?, moving: ranges_from_json(object.value("moving"))? })
    }

    pub fn store_screen(&self, windows: &[(f64, f64)], geometry: (i32, i32, f64), check: &ScreenCheck) -> Result<(), String> {
        let geometry = Value::array(vec![geometry.0.into(), geometry.1.into(), Value::from_f64(geometry.2)]);
        self.write(
            "screen",
            vec![
                ("geometry", geometry),
                ("windows", ranges_to_json(windows)),
                ("still", ranges_to_json(&check.still)),
                ("moving", ranges_to_json(&check.moving)),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("CARGO_TARGET_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
        let dir = base.join(format!("omareel-speech-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn hits_misses_and_invalidates() {
        let dir = scratch("hits");
        let cache = dir.join("cache").to_string_lossy().into_owned();
        let video = dir.join("screen.mp4");
        std::fs::write(&video, vec![7u8; 3 << 20]).unwrap();
        let video = video.to_string_lossy().into_owned();

        // Miss: nothing stored yet.
        let entry = Entry::new(&cache, &video).unwrap();
        assert_eq!(entry.speech(), None);
        let windows = [(0.0, 1.25), (3.5, 9.0)];
        let geometry = (3840, 2160, 2.0);
        assert_eq!(entry.screen(&windows, geometry), None);

        // Hit, with every digit kept.
        let speech = vec![(1.25, 3.5), (9.0, 12.000000001)];
        entry.store_speech(&speech).unwrap();
        let check = ScreenCheck { still: vec![(0.0, 1.25)], moving: vec![(3.5, 9.0)] };
        entry.store_screen(&windows, geometry, &check).unwrap();
        let again = Entry::new(&cache, &video).unwrap();
        assert_eq!(again.key(), entry.key());
        assert_eq!(again.speech(), Some(speech.clone()));
        assert_eq!(again.screen(&windows, geometry), Some(check.clone()));
        // Other silences or another geometry are a miss for the screen.
        assert_eq!(again.screen(&windows[..1], geometry), None);
        assert_eq!(again.screen(&windows, (1920, 1080, 1.0)), None);

        // Any change to the video is a miss, and storing again removes the stale entry.
        let mut bytes = std::fs::read(&video).unwrap();
        bytes[100] = 8;
        std::fs::write(&video, &bytes).unwrap();
        let changed = Entry::new(&cache, &video).unwrap();
        assert_ne!(changed.key(), entry.key());
        assert_eq!(changed.speech(), None);
        changed.store_speech(&[(2.0, 3.0)]).unwrap();
        let names: Vec<String> = std::fs::read_dir(directory(&cache))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&format!("{}.speech.json", changed.key())), "{names:?}");
        assert!(!names.contains(&format!("{}.speech.json", entry.key())), "{names:?}");
        // The old screen entry stays until a new one replaces it, but nothing reads it.
        assert_eq!(changed.screen(&windows, geometry), None);

        // A change past the first MiB keeps the size, but not the modification time.
        std::thread::sleep(std::time::Duration::from_millis(20));
        bytes[(2 << 20) + 5] = 9;
        std::fs::write(&video, &bytes).unwrap();
        assert_ne!(Entry::new(&cache, &video).unwrap().key(), changed.key());

        // A file from another version, or a broken one, is a miss.
        let current = Entry::new(&cache, &video).unwrap();
        std::fs::write(current.path("speech"), br#"{"version":0,"speech":[[1,2]]}"#).unwrap();
        assert_eq!(current.speech(), None);
        std::fs::write(current.path("speech"), b"{").unwrap();
        assert_eq!(current.speech(), None);
        assert!(Entry::new(&cache, &dir.join("missing.mp4").to_string_lossy()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
