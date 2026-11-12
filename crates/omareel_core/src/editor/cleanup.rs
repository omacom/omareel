//! Audio cleanup in the editor: the `audio.cleanup` edit, and the cleaned sound the preview
//! plays so it can be heard before exporting.
//!
//! When cleanup is on, the document asks for a cache file of the recording's sound, cleaned
//! ([`CleanAudioJob`], `<cache>/audio-cleanup/<recording>-<settings>.m4a`), with
//! [`Effect::StartCleanAudio`]. Once the file is there it hands it to the screen player with
//! [`Effect::SetScreenAudioSource`]: the player keeps showing the recording's picture and plays
//! that file's sound in its place, on the same timeline. Until then (or after a change, until
//! the new file is ready) the preview plays what it played before. [`Document::set_clean_audio_bypass`]
//! switches back to the recording's own sound for comparing, without editing the project.

use super::thumbnails::video_fingerprint;
use super::{Document, Effect};
use crate::audio_cleanup::{cache_directory, CleanAudioJob, CleanSettings};
use crate::compat::qdir::{self, FileInfo};
use crate::project::{AudioCleanup, Project};
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

/// The document's side of the cleaned preview sound.
#[derive(Debug, Default)]
pub struct CleanAudio {
    fingerprint: Option<String>,
    /// The cache file the settings ask for; empty for none.
    wanted: String,
    /// The file the screen player's sound comes from; empty for the recording's own.
    playing: String,
    /// The file a job is making.
    running: String,
    /// A file that could not be made, and why.
    failed: String,
    error: String,
    bypass: bool,
    cancel: Option<Arc<AtomicBool>>,
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
        if clean.wanted.is_empty() {
            CleanAudioStatus::Off
        } else if clean.failed == clean.wanted {
            CleanAudioStatus::Failed
        } else if FileInfo::new(&clean.wanted).is_file() {
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

    /// Brings the preview's sound in line with the cleanup settings: asks for the file they
    /// need ([`Effect::StartCleanAudio`]) and switches the screen player to it, or back to the
    /// recording, with [`Effect::SetScreenAudioSource`]. Called after every change, and by the
    /// Qt side once the screen player is loaded.
    pub fn refresh_clean_audio(&mut self) {
        let before = self.clean_audio_status();
        let wanted = match self.clean_settings() {
            Some(settings) => {
                let fingerprint = self
                    .clean_audio
                    .fingerprint
                    .get_or_insert_with(|| video_fingerprint(&self.video_path))
                    .clone();
                qdir::file_path(&cache_directory(&self.cache_location()), &settings.cache_name(&fingerprint))
            }
            None => String::new(),
        };
        let changed = wanted != self.clean_audio.wanted;
        if changed {
            // Other settings, another try.
            self.clean_audio.failed.clear();
            self.clean_audio.error.clear();
        }
        self.clean_audio.wanted = wanted.clone();
        let ready = !wanted.is_empty() && FileInfo::new(&wanted).is_file();
        // While a new file is made, the preview keeps what it played.
        let playing = if wanted.is_empty() || self.clean_audio.bypass {
            String::new()
        } else if ready {
            wanted.clone()
        } else {
            self.clean_audio.playing.clone()
        };
        if playing != self.clean_audio.playing {
            self.clean_audio.playing = playing.clone();
            self.emit(Effect::SetScreenAudioSource(playing));
        }
        if !wanted.is_empty() && !ready && self.clean_audio.running != wanted && self.clean_audio.failed != wanted {
            self.emit(Effect::StartCleanAudio);
        }
        if changed || self.clean_audio_status() != before {
            self.emit(Effect::CleanAudioChanged);
        }
    }

    /// The job [`Effect::StartCleanAudio`] asks for, to run off the GUI thread and hand to
    /// [`Document::finish_clean_audio`]; `None` when nothing needs making. A job still making
    /// a file nobody wants any more is cancelled.
    pub fn clean_audio_job(&mut self) -> Option<CleanAudioJob> {
        let settings = self.clean_settings()?;
        let wanted = self.clean_audio.wanted.clone();
        if wanted.is_empty() || FileInfo::new(&wanted).is_file() || self.clean_audio.running == wanted {
            return None;
        }
        self.cancel_clean_audio();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.clean_audio.cancel = Some(Arc::clone(&cancelled));
        self.clean_audio.running = wanted.clone();
        Some(CleanAudioJob {
            video_path: self.video_path.clone(),
            output_path: wanted,
            settings,
            duration: self.recording.source_duration,
            cancelled,
        })
    }

    /// A [`CleanAudioJob`] for `path` finished with `result`.
    pub fn finish_clean_audio(&mut self, path: &str, result: Result<String, String>) {
        if self.clean_audio.running == path {
            self.clean_audio.running.clear();
            self.clean_audio.cancel = None;
        }
        if let Err(error) = result {
            if error != "cancelled" && self.clean_audio.wanted == path {
                self.clean_audio.failed = path.to_owned();
                self.clean_audio.error = error;
                self.emit(Effect::CleanAudioChanged);
            }
        }
        // Switches to the file, or asks for the one the settings want now.
        self.refresh_clean_audio();
    }

    /// Stops a job making cleaned sound (when the settings moved on, or the editor closes).
    pub fn cancel_clean_audio(&mut self) {
        if let Some(cancel) = self.clean_audio.cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        self.clean_audio.running.clear();
    }
}
