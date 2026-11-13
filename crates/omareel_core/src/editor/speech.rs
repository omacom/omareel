//! What the editor does with the recording's speech: the transcript and the captions drawn
//! from it, and cutting pauses and filler words out of the clips. Everything here works on
//! the document alone, without Qt, so `omareel apply` can call it too.

use super::{Document, Effect};
use crate::captions::{self, CaptionFrame, Cue};
use crate::clip_timeline::ClipTimeline;
use crate::compat::cpp;
use crate::input_log::InputKind;
use crate::project::{Captions, Clip};
use crate::transcript::trim::{self, merge_ranges, PauseSettings};
use crate::transcript::Transcript;

/// Cursor travel (logical pixels) that counts as showing something during a pause.
const CURSOR_TRAVEL: f64 = 40.0;

impl Document {
    /// The bundle's transcript, once there is one.
    pub fn transcript(&self) -> Option<&Transcript> {
        self.transcript.as_ref()
    }

    /// A new transcript (or none): captions follow it at once.
    pub fn set_transcript(&mut self, transcript: Option<Transcript>) {
        self.transcript = transcript;
        self.rebuild_captions();
        self.emit(Effect::TranscriptChanged);
        self.emit(Effect::CompositionChanged);
    }

    /// Where the recording's sound has speech (see `transcript::vad`), once measured; pause
    /// trimming needs it.
    pub fn speech_regions(&self) -> Option<&[(f64, f64)]> {
        self.speech.as_deref()
    }

    pub fn set_speech_regions(&mut self, regions: Vec<(f64, f64)>) {
        self.speech = Some(regions);
        self.emit(Effect::TranscriptChanged);
    }

    /// The silences the screen stays still through (see `transcript::stillness`), once checked;
    /// pause trimming needs them too.
    pub fn still_windows(&self) -> Option<&[(f64, f64)]> {
        self.still.as_deref()
    }

    pub fn set_still_windows(&mut self, windows: Vec<(f64, f64)>) {
        self.still = Some(windows);
        self.emit(Effect::TranscriptChanged);
    }

    /// The silences `transcript::stillness::still_windows` should check, once the speech is
    /// measured.
    pub fn silences_to_check(&self) -> Option<Vec<(f64, f64)>> {
        let speech = self.speech.as_ref()?;
        Some(crate::transcript::stillness::silences_to_check(speech, self.recording.source_duration))
    }

    /// Whether pause trimming knows enough: the speech is measured and the screen checked.
    pub fn pauses_ready(&self) -> bool {
        self.speech.is_some() && self.still.is_some()
    }

    /// The project's caption settings, or their defaults.
    pub fn captions(&self) -> Captions {
        self.project.captions.clone().unwrap_or_default()
    }

    /// The captions on the output timeline, as the clips are now.
    pub fn caption_cues(&self) -> &[Cue] {
        &self.caption_cues
    }

    pub(crate) fn rebuild_captions(&mut self) {
        self.caption_cues = match &self.transcript {
            Some(transcript) => captions::build_cues(transcript, &self.project.clips),
            None => Vec::new(),
        };
    }

    /// The caption the preview shows: none while captions are off or an inserted video plays.
    pub fn preview_caption(&self) -> CaptionFrame {
        if !self.captions().enabled || self.clip_is_media(self.clip_for_output(self.output_position).0) {
            return CaptionFrame::default();
        }
        captions::sample(&self.caption_cues, self.output_position)
    }

    /// The parts of source ranges the clips still show.
    pub fn visible_source_ranges(&self, ranges: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let mut visible = Vec::new();
        for &(start, end) in ranges {
            for clip in self.project.clips.iter().filter(|clip| !clip.is_media()) {
                let (from, to) = (cpp::max(start, clip.in_), cpp::min(end, clip.out));
                if to - from > 0.05 {
                    visible.push((from, to));
                }
            }
        }
        merge_ranges(&visible)
    }

    /// Times something happened on screen by hand: key presses, clicks, scrolls, and cursor
    /// travel of more than a few pixels.
    pub fn input_activity(&self) -> Vec<f64> {
        let mut times = Vec::new();
        let mut anchor: Option<(f64, f64)> = None;
        for event in self.input.events() {
            match event.kind {
                InputKind::KeyDown | InputKind::ButtonDown | InputKind::Scroll => times.push(event.time),
                InputKind::Move => {
                    let (x, y) = (event.position.x, event.position.y);
                    match anchor {
                        None => anchor = Some((x, y)),
                        Some((ax, ay)) if (x - ax).hypot(y - ay) > CURSOR_TRAVEL => {
                            times.push(event.time);
                            anchor = Some((x, y));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        times
    }

    /// The pauses "Trim pauses" would cut, in source seconds, limited to what the clips still
    /// show: silences with no input and a still screen. Empty until the speech is measured and
    /// the screen checked ([`Document::pauses_ready`]).
    pub fn pause_ranges(&self, settings: &PauseSettings) -> Vec<(f64, f64)> {
        let (Some(speech), Some(still)) = (&self.speech, &self.still) else { return Vec::new() };
        let ranges = trim::pause_ranges(speech, self.recording.source_duration, &self.input_activity(), still, settings);
        self.visible_source_ranges(&ranges)
    }

    /// Whether filler words can be removed: the transcript's word times are measured (see
    /// `trim::can_remove_fillers`).
    pub fn can_remove_fillers(&self) -> bool {
        self.transcript.as_ref().is_some_and(trim::can_remove_fillers)
    }

    /// The filler words "Remove filler words" would cut, limited to what the clips still show;
    /// none when the word times are estimated.
    pub fn filler_ranges(&self) -> Vec<(f64, f64)> {
        let Some(transcript) = &self.transcript else { return Vec::new() };
        self.visible_source_ranges(&trim::filler_ranges(transcript))
    }

    /// Cuts source time ranges out of the recording clips as ONE undo step, and returns how
    /// many ranges cut something. Each range is cut out of every recording clip it touches; a
    /// piece of a clip that would be left shorter than 0.1 s goes with it. Ranges that would
    /// leave no recording are skipped. Zooms shrink to what still shows, as with
    /// `delete_output_range`, and the playhead stays on the same moment of the recording when
    /// it still plays. [`Effect::Pause`] comes first.
    pub fn remove_source_ranges(&mut self, ranges: &[(f64, f64)]) -> usize {
        let ranges = merge_ranges(ranges);
        let mut timeline = ClipTimeline::new(self.project.clips.clone());
        let mut removed = 0;
        // Latest first: recording clips keep recording order, so earlier output times hold.
        for &(start, end) in ranges.iter().rev() {
            let mut cuts: Vec<(f64, f64)> = Vec::new();
            for (i, clip) in timeline.clips().iter().enumerate() {
                if clip.is_media() {
                    continue;
                }
                let (mut from, mut to) = (cpp::max(start, clip.in_), cpp::min(end, clip.out));
                if to - from <= 1e-6 {
                    continue;
                }
                if from - clip.in_ < 0.1 {
                    from = clip.in_;
                }
                if clip.out - to < 0.1 {
                    to = clip.out;
                }
                cuts.push((timeline.output_time(i as i32, from), timeline.output_time(i as i32, to)));
            }
            let mut cut_any = false;
            for (from, to) in cuts.into_iter().rev() {
                let mut trial = timeline.clone();
                let remainder_id = self.next_id("c");
                if trial.delete_range(from, to, &remainder_id) && !trial.clips().iter().all(Clip::is_media) {
                    timeline = trial;
                    cut_any = true;
                }
            }
            if cut_any {
                removed += 1;
            }
        }
        if removed == 0 {
            return 0;
        }
        self.emit(Effect::Pause);
        let source = self.source_position();
        let in_insert = self.clip_is_media(self.clip_for_output(self.output_position).0);
        self.snapshot("");
        let clips = timeline.clips().to_vec();
        let zooms_changed = self.fit_zooms_to(&clips);
        self.project.clips = clips;
        if self.clip_index(&self.selected_clip_id) < 0 {
            let at = cpp::min(self.output_position, self.duration());
            self.selected_clip_id = self.project.clips[self.clip_for_output(at).0 as usize].id.clone();
        }
        self.changed(zooms_changed);
        let kept = if in_insert { -1.0 } else { self.source_to_output(source, -1) };
        let at = if kept >= 0.0 { kept } else { cpp::min(self.output_position, self.duration()) };
        self.emit(Effect::Seek(at));
        self.emit(Effect::SelectionChanged);
        removed
    }
}
