//! Audio cleanup in the editor: the `audio.cleanup` edit, and the cleaned sound the preview
//! plays so it can be heard before exporting.
//!
//! When cleanup is on, the document asks for the cleaned sound ([`CleanAudioJob`], cache files
//! under `<cache>/audio-cleanup/`) with [`Effect::StartCleanAudio`]: the recording's sound,
//! denoised and normalised, and, when normalising, each inserted video's sound with the same
//! gain. The gain comes from measuring the edited program as export does, so what the preview
//! plays is what the export will sound like; the Volume slider then sets the players' volume,
//! which is where export applies it too (after normalising).
//!
//! Once the files are there it hands them to the players with [`Effect::SetScreenAudioSource`]
//! and [`Effect::InsertAudioSourcesChanged`]: each player keeps showing its picture and plays
//! that file's sound in its place, on the same timeline. Until then (or after a change, until
//! the new files are ready) the preview plays what it played before.
//! [`Document::set_clean_audio_bypass`] switches back to the files' own sound for comparing,
//! without editing the project.
//!
//! Every job has an id of its own. A job for settings nobody wants any more is cancelled; one
//! for an older edit of the same settings runs to its end (its work is mostly reusable) and
//! the next one starts after it. Results are matched to jobs by id, so a cancelled job that
//! finishes late never touches the one that replaced it.

use super::thumbnails::video_fingerprint;
use super::{Document, Effect};
use crate::audio_cleanup::{
    cache_directory, CleanAudioJob, CleanAudioResult, CleanSettings, InsertSound, LoudnessMeasurement, ProgramSound,
};
use crate::compat::qdir;
use crate::project::{AudioCleanup, Project};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Where the preview's cleaned sound stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanAudioStatus {
    /// Cleanup is off (or the recording has no sound): the preview plays the recording.
    Off,
    /// The cleaned sound for the current settings is being made.
    Preparing,
    /// The preview can play the cleaned sound (unless bypassed).
    Ready,
    /// It could not be made; [`Document::clean_audio_error`] says why.
    Failed,
}

impl CleanAudioStatus {
    /// "off", "preparing", "ready" or "failed", as QML reads it.
    pub fn name(self) -> &'static str {
        match self {
            CleanAudioStatus::Off => "off",
            CleanAudioStatus::Preparing => "preparing",
            CleanAudioStatus::Ready => "ready",
            CleanAudioStatus::Failed => "failed",
        }
    }
}

/// What the settings and the edit ask the cleaned sound to be.
#[derive(Clone, Debug, PartialEq)]
struct Request {
    /// Identifies the request: the settings and, when normalising, the program and its files.
    key: String,
    settings: CleanSettings,
    program: ProgramSound,
    inserts: Vec<InsertSound>,
}

/// A job started and not finished yet.
#[derive(Debug)]
struct Running {
    id: u64,
    key: String,
    settings: CleanSettings,
    cancel: Arc<AtomicBool>,
}

/// The document's side of the cleaned preview sound.
#[derive(Debug, Default)]
pub struct CleanAudio {
    fingerprint: Option<String>,
    /// The inserted videos' fingerprints, by bundle-relative path.
    insert_fingerprints: HashMap<String, String>,
    wanted: Option<Request>,
    /// The request the latest finished job made its files for, its settings, and what it made.
    done: Option<(String, CleanSettings, CleanAudioResult)>,
    /// The file the screen player's sound comes from; empty for the recording's own.
    playing: String,
    /// The files the media player's sound comes from, by inserted video.
    playing_inserts: HashMap<String, String>,
    /// What the files playing now were normalised with, and for which settings.
    applied: Option<(CleanSettings, LoudnessMeasurement)>,
    running: Option<Running>,
    /// Jobs cancelled but not finished yet: cancelled again when the editor closes.
    cancelled: Vec<(u64, Arc<AtomicBool>)>,
    next_id: u64,
    /// A request that could not be made, and why.
    failed: String,
    error: String,
    bypass: bool,
}

impl Document {
    /// Sets `audio.cleanup` as one undo step: what the Audio panel's controls set, and what an
    /// agent's edit calls. Values are read as project.json reads them (strength clamped to 0.1-1,
    /// an unknown loudness is "off").
    pub fn set_audio_cleanup(&mut self, cleanup: AudioCleanup) {
        let old = self.project.to_json();
        let mut next = self.project.clone();
        next.audio.cleanup = cleanup;
        let parsed = Project::from_json(&next.to_json());
        if parsed.to_json() == old {
            return;
        }
        self.snapshot("");
        self.project = parsed;
        self.changed(false);
    }

    /// Whether noise suppression can apply: the recording has microphone sound.
    pub fn denoise_available(&self) -> bool {
        self.recording.has_audio && self.recording.has_microphone_audio
    }

    /// What the preview's cleaned sound is made with; `None` when cleanup does nothing for this
    /// recording (off, no sound, or only denoising and no microphone).
    pub fn clean_settings(&self) -> Option<CleanSettings> {
        if !self.recording.has_audio {
            return None;
        }
        let cleanup = &self.project.audio.cleanup;
        let settings = CleanSettings {
            denoise: (cleanup.denoise && self.denoise_available()).then_some(cleanup.strength),
            loudness: cleanup.loudness_lufs(),
        };
        (settings.denoise.is_some() || settings.loudness.is_some()).then_some(settings)
    }

    pub fn clean_audio_status(&self) -> CleanAudioStatus {
        let clean = &self.clean_audio;
        let Some(wanted) = &clean.wanted else { return CleanAudioStatus::Off };
        if clean.failed == wanted.key {
            CleanAudioStatus::Failed
        } else if clean.done.as_ref().is_some_and(|(key, _, _)| *key == wanted.key) {
            CleanAudioStatus::Ready
        } else {
            CleanAudioStatus::Preparing
        }
    }

    /// Why the cleaned sound could not be made, when [`CleanAudioStatus::Failed`].
    pub fn clean_audio_error(&self) -> &str {
        &self.clean_audio.error
    }

    /// The file the preview's screen sound comes from; empty for the recording's own.
    pub fn screen_audio_source(&self) -> &str {
        &self.clean_audio.playing
    }

    /// The file the preview's sound of the inserted video `media` (bundle-relative) comes
    /// from; empty for the video's own.
    pub fn insert_audio_source(&self, media: &str) -> &str {
        self.clean_audio.playing_inserts.get(media).map_or("", String::as_str)
    }

    pub fn clean_audio_bypass(&self) -> bool {
        self.clean_audio.bypass
    }

    /// Plays the recording's own sound (`true`) or the cleaned one again, for comparing. Not
    /// an edit: nothing is saved.
    pub fn set_clean_audio_bypass(&mut self, bypass: bool) {
        if self.clean_audio.bypass != bypass {
            self.clean_audio.bypass = bypass;
            self.emit(Effect::CleanAudioChanged);
            self.refresh_clean_audio();
        }
    }

    /// What the current settings and edit ask for; `None` when cleanup does nothing.
    fn clean_audio_request(&mut self) -> Option<Request> {
        let settings = self.clean_settings()?;
        let fingerprint =
            self.clean_audio.fingerprint.get_or_insert_with(|| video_fingerprint(&self.video_path)).clone();
        let mut program = ProgramSound {
            clips: Vec::new(),
            insert_audio: Vec::new(),
            use_source_audio: self.project.audio.desktop || self.project.audio.mic,
        };
        let mut inserts = Vec::new();
        let mut key = format!("{fingerprint}|{}", settings.key());
        // The program only matters to the loudness.
        if let Some(target) = settings.loudness {
            program.clips = self.project.clips.clone();
            let clips = program.clips.clone();
            for clip in &clips {
                if !clip.is_media() || program.insert_audio.contains(&clip.media) || !self.insert_info(&clip.media).audio {
                    continue;
                }
                let path = qdir::file_path(&self.recording.bundle_path, &clip.media);
                let fingerprint = self
                    .clean_audio
                    .insert_fingerprints
                    .entry(clip.media.clone())
                    .or_insert_with(|| video_fingerprint(&path))
                    .clone();
                program.insert_audio.push(clip.media.clone());
                inserts.push(InsertSound { media: clip.media.clone(), fingerprint });
            }
            let denoise = settings.denoise.map(|_| "denoise");
            if let Some((inputs, filter)) = program.measure_graph(&self.recording.bundle_path, &self.video_path, denoise, target) {
                key += &format!("|{}|{filter}", inputs.join(" "));
            }
            for insert in &inserts {
                key += &format!("|{}", insert.fingerprint);
            }
        }
        Some(Request { key: qdir::sha256_hex(key.as_bytes()), settings, program, inserts })
    }

    /// Brings the preview's sound in line with the cleanup settings and the edit: asks for
    /// the files they need ([`Effect::StartCleanAudio`]) and switches the players to them, or
    /// back to their own sound, with [`Effect::SetScreenAudioSource`] and
    /// [`Effect::InsertAudioSourcesChanged`]. Called after every change, and by the Qt side
    /// once the screen player is loaded.
    pub fn refresh_clean_audio(&mut self) {
        let before = self.clean_audio_status();
        let wanted = self.clean_audio_request();
        let changed = wanted.as_ref().map(|request| &request.key) != self.clean_audio.wanted.as_ref().map(|request| &request.key);
        if changed {
            // Another request, another try.
            self.clean_audio.failed.clear();
            self.clean_audio.error.clear();
        }
        self.clean_audio.wanted = wanted.clone();
        let done = match (&wanted, &self.clean_audio.done) {
            (Some(request), Some((key, _, result))) if *key == request.key => Some(result.clone()),
            _ => None,
        };
        // The same settings for an older edit: within a little of the gain the new one will
        // have, so it plays while that is made.
        let close = match (&wanted, &self.clean_audio.done) {
            (Some(request), Some((_, settings, result))) if *settings == request.settings => Some(result),
            _ => None,
        };
        // While new files are made, the preview keeps what it played.
        let (playing, playing_inserts) = if wanted.is_none() || self.clean_audio.bypass {
            (String::new(), HashMap::new())
        } else if let Some(result) = done.as_ref().or(close) {
            (result.recording.clone(), result.inserts.iter().cloned().collect())
        } else {
            (self.clean_audio.playing.clone(), self.clean_audio.playing_inserts.clone())
        };
        if playing != self.clean_audio.playing {
            self.clean_audio.playing = playing.clone();
            self.emit(Effect::SetScreenAudioSource(playing));
        }
        if playing_inserts != self.clean_audio.playing_inserts {
            self.clean_audio.playing_inserts = playing_inserts;
            self.emit(Effect::InsertAudioSourcesChanged);
        }
        if let Some(request) = &wanted {
            let waiting = self.clean_audio.running.as_ref().is_some_and(|running| running.settings == request.settings);
            if done.is_none() && self.clean_audio.failed != request.key && !waiting {
                self.emit(Effect::StartCleanAudio);
            }
        }
        if changed || self.clean_audio_status() != before {
            self.emit(Effect::CleanAudioChanged);
        }
    }

    /// The job [`Effect::StartCleanAudio`] asks for, to run off the GUI thread and hand to
    /// [`Document::finish_clean_audio`] with its id; `None` when nothing needs making, or while
    /// a job for the same settings runs (the next one starts when it ends). A job for other
    /// settings is cancelled.
    pub fn clean_audio_job(&mut self) -> Option<CleanAudioJob> {
        let request = self.clean_audio.wanted.clone()?;
        if self.clean_audio.done.as_ref().is_some_and(|(key, _, _)| *key == request.key) || self.clean_audio.failed == request.key {
            return None;
        }
        if let Some(running) = &self.clean_audio.running {
            if running.settings == request.settings {
                return None;
            }
            self.cancel_running_clean_audio();
        }
        self.clean_audio.next_id += 1;
        let id = self.clean_audio.next_id;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.clean_audio.running =
            Some(Running { id, key: request.key.clone(), settings: request.settings, cancel: Arc::clone(&cancelled) });
        let previous = self
            .clean_audio
            .applied
            .filter(|(settings, _)| *settings == request.settings)
            .map(|(_, measured)| measured);
        Some(CleanAudioJob {
            id,
            bundle: self.recording.bundle_path.clone(),
            video_path: self.video_path.clone(),
            fingerprint: self.clean_audio.fingerprint.clone().unwrap_or_default(),
            cache_directory: cache_directory(&self.cache_location()),
            settings: request.settings,
            program: request.program,
            inserts: request.inserts,
            previous,
            cancelled,
        })
    }

    /// The [`CleanAudioJob`] `id` finished with `result`. A job cancelled since is only
    /// forgotten: what it says is about settings nobody wants.
    pub fn finish_clean_audio(&mut self, id: u64, result: Result<CleanAudioResult, String>) {
        self.clean_audio.cancelled.retain(|(cancelled, _)| *cancelled != id);
        let Some(running) = self.clean_audio.running.take_if(|running| running.id == id) else { return };
        match result {
            Ok(result) => {
                if let Some(measured) = result.applied {
                    self.clean_audio.applied = Some((running.settings, measured));
                }
                self.clean_audio.done = Some((running.key, running.settings, result));
            }
            Err(error) => {
                if error != "cancelled" && self.clean_audio.wanted.as_ref().is_some_and(|wanted| wanted.key == running.key) {
                    self.clean_audio.failed = running.key;
                    self.clean_audio.error = error;
                    self.emit(Effect::CleanAudioChanged);
                }
            }
        }
        // Switches to the files, or asks for the ones the edit wants now.
        self.refresh_clean_audio();
    }

    fn cancel_running_clean_audio(&mut self) {
        if let Some(running) = self.clean_audio.running.take() {
            running.cancel.store(true, Ordering::SeqCst);
            self.clean_audio.cancelled.push((running.id, running.cancel));
        }
    }

    /// Stops every job making cleaned sound (when the editor closes).
    pub fn cancel_clean_audio(&mut self) {
        self.cancel_running_clean_audio();
        for (_, cancel) in &self.clean_audio.cancelled {
            cancel.store(true, Ordering::SeqCst);
        }
    }
}
