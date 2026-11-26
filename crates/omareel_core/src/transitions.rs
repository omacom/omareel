//! Transitions at the cuts between recording clips: where the recording jumps from one moment to
//! another, a short effect can hide the jump. The project has one style for every cut
//! (`project.transition`) and a clip can override the one at its start (`clip.transition`).
//!
//! Styles:
//! - `none`: a plain jump cut, as before transitions existed.
//! - `zoom-punch`: the screen grows by [`PUNCH_SCALE`] over the last [`PUNCH_IN`] seconds before
//!   the cut and settles back over the first [`PUNCH_OUT`] seconds after it, so the eye follows
//!   the movement instead of the jump. The sound fades out and in over [`DECLICK`] seconds at the
//!   cut, so the join does not click.
//!
//! A crossfade is not offered: it needs the frames of both clips at once, which the preview's
//! single screen player and the export's one-clip-at-a-time decoder do not have (see
//! docs/USAGE.md, "Transitions").
//!
//! Everything here is in OUTPUT seconds and only reads the clip list, so the preview
//! (`editor::Document::preview_transition`) and the export (`export::composition`) sample the
//! same function and render the same frames.

use crate::project::Clip;

/// The styles a project or a clip can name.
pub const STYLES: [&str; 2] = ["none", "zoom-punch"];

/// How much bigger the screen is at the cut.
pub const PUNCH_SCALE: f64 = 0.06;
/// Seconds the punch grows before the cut.
pub const PUNCH_IN: f64 = 0.08;
/// Seconds it settles after the cut.
pub const PUNCH_OUT: f64 = 0.32;
/// Seconds of sound faded out before and in after a cut with a transition.
pub const DECLICK: f64 = 0.02;
/// Recording clips whose source times are closer than this play on without a jump (a split, or
/// a speed change), so they meet without a cut.
pub const CONTINUOUS: f64 = 1.0 / 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    None,
    ZoomPunch,
}

impl Style {
    pub fn parse(name: &str) -> Option<Style> {
        match name {
            "none" => Some(Style::None),
            "zoom-punch" => Some(Style::ZoomPunch),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Style::None => "none",
            Style::ZoomPunch => "zoom-punch",
        }
    }
}

/// A cut between two recording clips, with the transition it has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// Index of the clip after the cut.
    pub clip: usize,
    /// Output time of the cut.
    pub time: f64,
    pub style: Style,
}

/// What the composition draws for the transitions at one output time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransitionFrame {
    /// Scale of the screen (and the cursor on it) about the canvas centre; 1 for none.
    pub scale: f64,
}

impl Default for TransitionFrame {
    fn default() -> TransitionFrame {
        TransitionFrame { scale: 1.0 }
    }
}

fn length(clip: &Clip) -> f64 {
    (clip.out - clip.in_) / clip.speed.max(0.000001)
}

/// Whether the recording jumps between two neighbouring clips: both are recording clips and the
/// second does not go on where the first stopped.
pub fn is_cut(before: &Clip, after: &Clip) -> bool {
    !before.is_media() && !after.is_media() && (after.in_ - before.out).abs() > CONTINUOUS
}

/// The style of the cut at the start of `clip`: its own, or the project's (`default`); `None`
/// for unknown names.
pub fn style_of(clip: &Clip, default: &str) -> Style {
    Style::parse(clip.transition.as_deref().unwrap_or(default)).unwrap_or(Style::None)
}

/// Every cut in the clip list, transitions of `none` included.
pub fn cuts(clips: &[Clip], default: &str) -> Vec<Cut> {
    let mut cuts = Vec::new();
    let mut time = 0.0;
    for (i, clip) in clips.iter().enumerate() {
        if i > 0 && is_cut(&clips[i - 1], clip) {
            cuts.push(Cut { clip: i, time, style: style_of(clip, default) });
        }
        time += length(clip).max(0.0);
    }
    cuts
}

/// Smoothstep from 0 to 1.
fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The zoom punch's extra scale `offset` seconds from its cut (negative before it).
pub fn punch(offset: f64) -> f64 {
    if offset < -PUNCH_IN || offset >= PUNCH_OUT {
        0.0
    } else if offset < 0.0 {
        PUNCH_SCALE * ease((offset + PUNCH_IN) / PUNCH_IN)
    } else {
        // Settles quickly, then slowly: an ease-out over the settle time.
        let t = offset / PUNCH_OUT;
        PUNCH_SCALE * (1.0 - t).powi(3)
    }
}

/// The transitions at `output_time`, given the timeline's [`cuts`].
pub fn sample(cuts: &[Cut], output_time: f64) -> TransitionFrame {
    // Cuts close together (filler words) would add their punches up and shake the frame; the
    // strongest one at this moment counts.
    let mut extra: f64 = 0.0;
    for cut in cuts {
        if cut.style == Style::ZoomPunch {
            extra = extra.max(punch(output_time - cut.time));
        }
    }
    TransitionFrame { scale: 1.0 + extra }
}

/// For each clip, whether its sound fades in at its start and out at its end, for the cuts
/// with a transition (de-clicking the join).
pub fn audio_fades(clips: &[Clip], default: &str) -> Vec<(bool, bool)> {
    let mut fades = vec![(false, false); clips.len()];
    for cut in cuts(clips, default) {
        if cut.style != Style::None {
            fades[cut.clip].0 = true;
            fades[cut.clip - 1].1 = true;
        }
    }
    fades
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: &str, in_: f64, out: f64) -> Clip {
        Clip { id: id.into(), in_, out, ..Clip::default() }
    }

    #[test]
    fn punches_at_close_cuts_do_not_add_up() {
        let close = [
            Cut { clip: 1, time: 1.0, style: Style::ZoomPunch },
            Cut { clip: 2, time: 1.06, style: Style::ZoomPunch },
        ];
        let peak = (0..300).map(|i| sample(&close, 0.5 + f64::from(i) * 0.005).scale).fold(0.0, f64::max);
        assert!(peak <= 1.0 + PUNCH_SCALE + 1e-9, "{peak}");
        assert!(peak > 1.0 + PUNCH_SCALE * 0.9);
    }

    #[test]
    fn only_jumps_between_recording_clips_are_cuts() {
        let mut clips = vec![
            clip("a", 0.0, 2.0),
            // A split: goes on where "a" stopped.
            clip("b", 2.0, 3.0),
            // A jump forward.
            clip("c", 5.0, 6.0),
            Clip { media: "media/x.mp4".into(), ..clip("m", 0.0, 1.0) },
            clip("d", 6.0, 7.0),
            // Played at double speed: 1 s of output.
            Clip { speed: 2.0, ..clip("e", 1.0, 3.0) },
        ];
        let cuts = cuts(&clips, "zoom-punch");
        assert_eq!(cuts.len(), 2, "{cuts:?}");
        assert_eq!((cuts[0].clip, cuts[0].time, cuts[0].style), (2, 3.0, Style::ZoomPunch));
        assert_eq!((cuts[1].clip, cuts[1].time), (5, 6.0));
        // A clip's own style wins; an unknown one is none.
        clips[2].transition = Some("none".into());
        clips[5].transition = Some("wipe".into());
        assert!(super::cuts(&clips, "zoom-punch").iter().all(|cut| cut.style == Style::None));
        clips[2].transition = Some("zoom-punch".into());
        let styles: Vec<Style> = super::cuts(&clips, "none").iter().map(|cut| cut.style).collect();
        assert_eq!(styles, vec![Style::ZoomPunch, Style::None]);
    }

    #[test]
    fn the_punch_peaks_at_the_cut_and_settles() {
        let clips = vec![clip("a", 0.0, 2.0), clip("b", 4.0, 6.0)];
        let cuts = cuts(&clips, "zoom-punch");
        let at = |t: f64| sample(&cuts, t).scale;
        assert_eq!(at(0.0), 1.0);
        assert_eq!(at(2.0 - PUNCH_IN - 0.001), 1.0);
        assert!(at(1.96) > 1.0 && at(1.96) < 1.0 + PUNCH_SCALE);
        assert!((at(2.0) - (1.0 + PUNCH_SCALE)).abs() < 1e-12);
        assert!(at(2.05) > at(2.15) && at(2.15) > at(2.3) && at(2.3) > 1.0);
        assert_eq!(at(2.0 + PUNCH_OUT), 1.0);
        assert_eq!(at(3.5), 1.0);
        // Rising before the cut, falling after it.
        let mut previous = 1.0;
        for i in 0..=8 {
            let scale = at(2.0 - PUNCH_IN + f64::from(i) * PUNCH_IN / 8.0);
            assert!(scale >= previous);
            previous = scale;
        }
        // Without a transition nothing moves.
        let none = super::cuts(&clips, "none");
        for t in [1.95, 2.0, 2.1] {
            assert_eq!(sample(&none, t), TransitionFrame::default());
        }
    }

    #[test]
    fn audio_fades_at_cuts_with_a_transition() {
        let mut clips = vec![clip("a", 0.0, 2.0), clip("b", 2.0, 3.0), clip("c", 5.0, 6.0), clip("d", 8.0, 9.0)];
        clips[3].transition = Some("none".into());
        assert_eq!(audio_fades(&clips, "zoom-punch"), vec![(false, false), (false, true), (true, false), (false, false)]);
        assert_eq!(audio_fades(&clips, "none"), vec![(false, false); 4]);
    }
}
