//! Editing polish on the timeline that needs no Qt: "Speed up quiet parts" (stretches with no
//! speech but a changing screen play faster), transitions at the cuts, and the measurement
//! cache the speech tools read from. Each edit is one undo step, so `omareel apply` can call
//! them as the panels do.

use super::{Document, Effect};
use crate::compat::cpp;
use crate::project::allowed_clip_speeds;
use crate::transcript::cache;
use crate::transcript::speedup::{self, QuietPart, SpeedUpSettings};
use crate::transcript::stillness::ScreenCheck;
use crate::transitions::{self, Cut, TransitionFrame};

impl Document {
    /// What the screen check found in the silences: [`Document::set_still_windows`] and the
    /// windows the screen moves in, at once.
    pub fn set_screen_check(&mut self, check: ScreenCheck) {
        self.still = Some(check.still);
        self.moving = Some(check.moving);
        self.emit(Effect::TranscriptChanged);
    }

    /// The silences the screen moves in, once checked.
    pub fn moving_windows(&self) -> Option<&[(f64, f64)]> {
        self.moving.as_deref()
    }

    /// Whether "Speed up quiet parts" knows enough: the speech is measured and the screen
    /// checked.
    pub fn quiet_parts_ready(&self) -> bool {
        self.speech.is_some() && self.moving.is_some()
    }

    /// The parts "Speed up quiet parts" would speed up, in source seconds, with the speed each
    /// gets: quiet stretches ([`speedup::quiet_ranges`]) limited to what recording clips at
    /// normal speed still show, at least `settings.min_length` long each. Clips already sped up
    /// or slowed down are left alone. Empty until [`Document::quiet_parts_ready`].
    pub fn quiet_parts(&self, settings: &SpeedUpSettings) -> Vec<QuietPart> {
        let Some(moving) = &self.moving else { return Vec::new() };
        // Recording clips at normal speed, neighbours that continue each other (a split, not a
        // cut) joined into one run: a quiet stretch across a split is one stretch.
        let mut runs: Vec<(f64, f64)> = Vec::new();
        let mut continues = false;
        for clip in &self.project.clips {
            let normal = !clip.is_media() && clip.speed == 1.0;
            match runs.last_mut() {
                Some(run) if normal && continues && (clip.in_ - run.1).abs() < 1e-6 => run.1 = clip.out,
                _ if normal => runs.push((clip.in_, clip.out)),
                _ => {}
            }
            continues = normal;
        }
        let mut parts = Vec::new();
        for (start, end) in speedup::quiet_ranges(moving, self.recording.source_duration, settings) {
            for &(run_in, run_out) in &runs {
                let (from, to) = (cpp::max(start, run_in), cpp::min(end, run_out));
                if to - from >= settings.min_length {
                    parts.push(QuietPart { start: from, end: to, speed: speedup::choose_speed(to - from, settings.target) });
                }
            }
        }
        parts
    }

    /// Splits each part out of the recording clips at normal speed and plays it at its speed, as
    /// ONE undo step; returns how many parts changed something. A speed must be one of the
    /// clip speeds offered (`allowed_clip_speeds`), parts overlapping an earlier one are
    /// skipped, and a piece of a clip that would be left shorter than 0.1 s joins the part next
    /// to it. The first piece of a clip keeps its id (and its transition), the others get new
    /// ones. The playhead stays on the same moment of the recording. [`Effect::Pause`] comes
    /// first.
    pub fn speed_up_source_ranges(&mut self, parts: &[QuietPart]) -> usize {
        let allowed = allowed_clip_speeds();
        let mut parts: Vec<QuietPart> = parts
            .iter()
            .copied()
            .filter(|p| p.start.is_finite() && p.end.is_finite() && p.end > p.start)
            .filter(|p| p.speed != 1.0 && allowed.contains(&p.speed))
            .collect();
        parts.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut kept: Vec<QuietPart> = Vec::new();
        for part in parts {
            if kept.last().is_none_or(|last| part.start >= last.end) {
                kept.push(part);
            }
        }
        let mut used = vec![false; kept.len()];
        let mut clips = Vec::new();
        for clip in self.project.clips.clone() {
            if clip.is_media() || clip.speed != 1.0 {
                clips.push(clip);
                continue;
            }
            let mut pieces: Vec<(f64, f64, f64)> = Vec::new();
            let mut cursor = clip.in_;
            for (k, part) in kept.iter().enumerate() {
                let (mut from, mut to) = (cpp::max(part.start, cursor), cpp::min(part.end, clip.out));
                if to - from < 0.1 {
                    continue;
                }
                if from - cursor < 0.1 {
                    from = cursor;
                }
                if clip.out - to < 0.1 {
                    to = clip.out;
                }
                if from > cursor {
                    pieces.push((cursor, from, 1.0));
                }
                pieces.push((from, to, part.speed));
                used[k] = true;
                cursor = to;
            }
            if pieces.is_empty() {
                clips.push(clip);
                continue;
            }
            if cursor < clip.out {
                pieces.push((cursor, clip.out, 1.0));
            }
            for (i, (in_, out, speed)) in pieces.into_iter().enumerate() {
                let mut piece = clip.clone();
                (piece.in_, piece.out, piece.speed) = (in_, out, speed);
                if i > 0 {
                    piece.id = self.next_id("c");
                    piece.transition = None;
                }
                clips.push(piece);
            }
        }
        let changed = used.iter().filter(|&&used| used).count();
        if changed == 0 {
            return 0;
        }
        self.emit(Effect::Pause);
        let source = self.source_position();
        let in_insert = self.clip_is_media(self.clip_for_output(self.output_position).0);
        self.snapshot("");
        self.project.clips = clips;
        self.changed(false);
        let kept = if in_insert { -1.0 } else { self.source_to_output(source, -1) };
        let at = if kept >= 0.0 { kept } else { cpp::min(self.output_position, self.duration()) };
        self.emit(Effect::Seek(at));
        self.emit(Effect::SelectionChanged);
        changed
    }

    // ---------------------------------------------------------------------------------------
    // Transitions.

    /// The cuts between recording clips, with their transitions (`none` ones too).
    pub fn transition_cuts(&self) -> Vec<Cut> {
        transitions::cuts(&self.project.clips, &self.project.transition)
    }

    /// Whether the clip with this id starts at a cut, so it can have a transition of its own.
    pub fn clip_starts_at_cut(&self, id: &str) -> bool {
        let index = self.clip_index(id);
        index > 0 && transitions::is_cut(&self.project.clips[index as usize - 1], &self.project.clips[index as usize])
    }

    /// The transitions the preview draws at the output position (the composition's
    /// `transition`); the export samples the same function.
    pub fn preview_transition(&self) -> TransitionFrame {
        transitions::sample(&self.transition_cuts(), self.output_position)
    }

    /// The project's transition at every cut ("none" or "zoom-punch"), one undo step; false for
    /// an unknown style or no change.
    pub fn set_transition(&mut self, style: &str) -> bool {
        if !transitions::STYLES.contains(&style) || self.project.transition == style {
            return false;
        }
        self.snapshot("");
        self.project.transition = style.to_owned();
        self.changed(false);
        true
    }

    /// The transition at the cut before the clip `id`, overriding the project's; `None` follows
    /// the project again. One undo step; false for an unknown clip or style, or no change.
    pub fn set_cut_transition(&mut self, id: &str, style: Option<&str>) -> bool {
        let index = self.clip_index(id);
        if index < 0 || style.is_some_and(|style| !transitions::STYLES.contains(&style)) {
            return false;
        }
        let style = style.map(str::to_owned);
        if self.project.clips[index as usize].transition == style {
            return false;
        }
        self.snapshot("");
        self.project.clips[index as usize].transition = style;
        self.changed(false);
        true
    }

    // ---------------------------------------------------------------------------------------
    // Measurement cache.

    /// The cache entry for this recording's speech and screen measurements, keyed by
    /// screen.mp4 as it is now (see `transcript::cache`); `None` when it cannot be read. Reads
    /// the video's first MiB.
    pub fn measurement_cache(&self) -> Option<cache::Entry> {
        cache::Entry::new(&self.cache_location(), &self.video_path)
    }
}
