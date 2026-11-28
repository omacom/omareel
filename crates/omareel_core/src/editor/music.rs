//! Background music: adding it (copied into the bundle's media/ like an inserted video),
//! removing it, and what the preview's music player should do at an output time. Its settings
//! change through [`Document::set_project_value`] (`music.volume`, `music.fadeIn`,
//! `music.fadeOut`, `music.duck`, `music.loop`, `music.start`), which ignores them while there
//! is no music.

use super::bundle::{import_music, ImportedMusic};
use super::{Document, Effect};
use crate::compat::qdir;
use crate::music::{self, AudioLevels};
use crate::project::Music;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// How the document learns the length of a music file's sound (normally
/// [`crate::media_probe::probe_audio`]).
pub type ProbeAudio = Arc<dyn Fn(&str) -> Result<f64, String> + Send + Sync>;

/// What the document keeps about the music beside the project.
pub(crate) struct MusicState {
    probe_audio: ProbeAudio,
    /// The length of each music file's sound, by media path (`None`: it did not probe).
    durations: HashMap<String, Option<f64>>,
    importing: bool,
    /// Sound levels by absolute path: the music's, for the timeline, and the recording's and
    /// inserted videos', which the preview ducks by.
    levels: HashMap<String, Arc<AudioLevels>>,
    /// Paths whose levels were asked for.
    levels_requested: HashSet<String>,
    levels_revision: u64,
}

impl Default for MusicState {
    fn default() -> MusicState {
        MusicState {
            probe_audio: Arc::new(crate::media_probe::probe_audio),
            durations: HashMap::new(),
            importing: false,
            levels: HashMap::new(),
            levels_requested: HashSet::new(),
            levels_revision: 0,
        }
    }
}

/// What the music import runs off the GUI thread: [`import_music`].
#[derive(Clone)]
pub struct MusicImportJob {
    pub source: String,
    pub bundle_path: String,
    pub probe_audio: ProbeAudio,
}

impl MusicImportJob {
    pub fn run(&self) -> ImportedMusic {
        import_music(&self.source, &self.bundle_path, &*self.probe_audio)
    }
}

/// What the preview's music player does at an output time.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicPlayback {
    /// The music file's absolute path.
    pub path: String,
    /// Where in the file, in milliseconds; meaningless when not `audible`.
    pub position_ms: i64,
    /// The player's volume, 0 to 1: the music's volume, fades and ducking.
    pub gain: f64,
    /// The music plays here (it has not run out).
    pub audible: bool,
    /// Which pass over the file this is (see [`music::FilePosition`]): the player restarts
    /// when it changes, however short the file.
    pub pass: i64,
    /// How long until the file's sound ends in this pass, in milliseconds; -1 when unknown.
    pub remaining_ms: i64,
}

impl Document {
    /// The project's music.
    pub fn music(&self) -> Option<&Music> {
        self.project.music.as_ref()
    }

    /// Replaces [`crate::media_probe::probe_audio`] (for tests).
    pub fn set_audio_probe(&mut self, probe: ProbeAudio) {
        self.music.probe_audio = probe;
        self.music.durations.clear();
    }

    /// The length of the music file's sound, once probed; `None` without music or when it did
    /// not probe.
    pub fn music_duration(&self) -> Option<f64> {
        let music = self.project.music.as_ref()?;
        self.music.durations.get(&music.file).copied().flatten()
    }

    /// Probes the music file the project names, once.
    pub(crate) fn probe_music(&mut self) {
        let Some(file) = self.project.music.as_ref().map(|music| music.file.clone()) else { return };
        if self.music.durations.contains_key(&file) {
            return;
        }
        let duration = (self.music.probe_audio)(&qdir::file_path(&self.recording.bundle_path, &file)).ok();
        self.music.durations.insert(file, duration);
    }

    /// Adds music from a file (an audio file, or a video whose sound is used), copying it into
    /// the bundle's media/ directory, as one undoable edit. Music already there is replaced,
    /// keeping its settings but starting the new file from its beginning. Waits for the copy;
    /// the editor uses [`Document::start_music_import`] instead.
    pub fn set_music(&mut self, source: &str) -> Result<(), String> {
        if source.is_empty() {
            return Err("No music file given".into());
        }
        let imported = self.music_import_job(source).run();
        if !imported.error.is_empty() {
            return Err(imported.error);
        }
        self.apply_music(imported);
        Ok(())
    }

    /// Removes the music, as one undoable edit; false when there was none. The file stays in
    /// media/, so undo brings it back.
    pub fn clear_music(&mut self) -> bool {
        if self.project.music.is_none() {
            return false;
        }
        self.snapshot("");
        self.project.music = None;
        self.changed(false);
        true
    }

    pub fn music_importing(&self) -> bool {
        self.music.importing
    }

    /// `addMusic(url)`: starts copying the file into the bundle off the GUI thread
    /// ([`Effect::StartMusicImport`]), unless a music import is running.
    pub fn start_music_import(&mut self, source: &str) {
        if self.music.importing || source.is_empty() {
            return;
        }
        self.music.importing = true;
        self.import_error.clear();
        self.emit(Effect::ImportingChanged);
        self.emit(Effect::StartMusicImport(source.to_owned()));
    }

    /// The work [`Effect::StartMusicImport`] asks for.
    pub fn music_import_job(&self, source: &str) -> MusicImportJob {
        MusicImportJob {
            source: source.to_owned(),
            bundle_path: self.recording.bundle_path.clone(),
            probe_audio: self.music.probe_audio.clone(),
        }
    }

    /// The music import's result: the music is set, or the error shown as an import error.
    pub fn finish_music_import(&mut self, imported: ImportedMusic) {
        self.music.importing = false;
        if !imported.error.is_empty() {
            self.import_error = imported.error;
            self.emit(Effect::ImportingChanged);
            return;
        }
        self.apply_music(imported);
        self.emit(Effect::ImportingChanged);
    }

    fn apply_music(&mut self, imported: ImportedMusic) {
        self.music.durations.insert(imported.path.clone(), Some(imported.duration));
        let music = match &self.project.music {
            Some(current) => Music { file: imported.path, start: 0.0, ..current.clone() },
            None => Music { file: imported.path, ..Music::default() },
        };
        if self.project.music.as_ref() == Some(&music) {
            return;
        }
        self.snapshot("");
        self.project.music = Some(music);
        self.changed(false);
    }

    /// The music file's absolute path.
    fn music_path(&self) -> Option<String> {
        self.project.music.as_ref().map(|music| qdir::file_path(&self.recording.bundle_path, &music.file))
    }

    /// The sound under the music at an output time, for ducking: the file the clip there
    /// plays, its time in that file and the clip's speed; `None` when that clip is silent in
    /// the mix.
    fn program_sound_at(&self, output_time: f64) -> Option<(String, f64, f64)> {
        let (index, clip_start) = self.clip_for_output(output_time);
        let clip = self.clip(index)?;
        let source = clip.in_ + (output_time - clip_start) * clip.speed;
        if clip.is_media() {
            let audible = self.inserts.get(&clip.media).is_some_and(|info| info.audio);
            return audible.then(|| (qdir::file_path(&self.recording.bundle_path, &clip.media), source, clip.speed));
        }
        let audible = self.recording.has_audio && (self.project.audio.desktop || self.project.audio.mic);
        audible.then(|| (self.video_path.clone(), source, clip.speed))
    }

    /// What the preview's music player does at an output time; `None` without music.
    pub fn music_playback(&self, output_time: f64) -> Option<MusicPlayback> {
        let music = self.project.music.as_ref()?;
        let path = self.music_path()?;
        let duration = self.duration();
        let file_duration = self.music_duration();
        let position = music::file_position(music, file_duration, output_time);
        let audible = position.is_some() && output_time <= duration;
        let mut gain = music.volume * music::fade_gain(music, duration, output_time);
        if music.duck {
            if let Some((source, time, speed)) = self.program_sound_at(output_time) {
                if let Some(levels) = self.music.levels.get(&source) {
                    let key_gain = self.project.audio.volume.max(0.0);
                    gain *= music::preview_duck_gain(levels, time, speed, key_gain);
                }
            }
        }
        let time = position.map_or(0.0, |position| position.time);
        Some(MusicPlayback {
            path,
            position_ms: (time * 1000.0).round() as i64,
            gain: gain.clamp(0.0, 1.0),
            audible,
            pass: position.map_or(0, |position| position.pass),
            remaining_ms: file_duration.map_or(-1, |length| ((length - time).max(0.0) * 1000.0).round() as i64),
        })
    }

    /// Files whose sound levels the preview and timeline need and nobody has asked for yet,
    /// now marked as asked for: the music's (for its waveform) and, when it ducks, the
    /// recording's and the inserted videos' with sound. Measure each with
    /// [`crate::music::measure_levels`] and give it to [`Document::set_audio_levels`].
    pub fn take_audio_levels_requests(&mut self) -> Vec<String> {
        let Some(music) = self.project.music.as_ref() else { return Vec::new() };
        let mut wanted: Vec<String> = Vec::new();
        if let Some(path) = self.music_path() {
            wanted.push(path);
        }
        if music.duck {
            if self.recording.has_audio {
                wanted.push(self.video_path.clone());
            }
            for clip in &self.project.clips {
                if clip.is_media() && self.inserts.get(&clip.media).is_some_and(|info| info.audio) {
                    wanted.push(qdir::file_path(&self.recording.bundle_path, &clip.media));
                }
            }
        }
        wanted.retain(|path| self.music.levels_requested.insert(path.clone()));
        wanted
    }

    /// A file's measured sound levels (an empty set when it could not be measured).
    pub fn set_audio_levels(&mut self, path: &str, levels: AudioLevels) {
        self.music.levels.insert(path.to_owned(), Arc::new(levels));
        self.music.levels_revision += 1;
    }

    /// Changes whenever levels arrive, so the timeline redraws the music's waveform.
    pub fn audio_levels_revision(&self) -> u64 {
        self.music.levels_revision
    }

    /// The music's lane on the timeline: how long it plays in the output, and its loudness in
    /// `buckets` equal steps across the output, 0 to 1 of its loudest (empty until measured).
    pub fn music_lane(&self, buckets: usize) -> Option<(f64, Vec<f64>)> {
        let music = self.project.music.as_ref()?;
        let duration = self.duration();
        let file_duration = self.music_duration();
        let extent = music::extent(music, file_duration, duration);
        let Some(levels) = self.music_path().and_then(|path| self.music.levels.get(&path).cloned()) else {
            return Some((extent, Vec::new()));
        };
        let peak = levels.peak();
        if peak <= 0.0 || buckets == 0 || duration <= 0.0 {
            return Some((extent, Vec::new()));
        }
        let step = duration / buckets as f64;
        let waveform = (0..buckets)
            .map(|i| {
                let output_time = (i as f64 + 0.5) * step;
                music::file_time(music, file_duration, output_time).map_or(0.0, |time| {
                    let half = step / 2.0;
                    (levels.max_between(time - half, time + half) / peak).clamp(0.0, 1.0)
                })
            })
            .collect();
        Some((extent, waveform))
    }
}
