//! Source ranges to cut for "Trim pauses" and "Remove filler words". They only say what to
//! remove; `editor::Document::remove_source_ranges` removes them as one undo step.

use super::Transcript;

/// How pauses are found and shortened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PauseSettings {
    /// Silences at least this long are pauses (seconds).
    pub min_pause: f64,
    /// How much of each pause stays, split around the cut (seconds).
    pub keep: f64,
}

impl Default for PauseSettings {
    fn default() -> PauseSettings {
        PauseSettings { min_pause: 1.0, keep: 0.4 }
    }
}

/// Sorted ranges with overlapping or touching ones joined; empty and inverted ones dropped.
pub fn merge_ranges(ranges: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut sorted: Vec<(f64, f64)> =
        ranges.iter().copied().filter(|(a, b)| a.is_finite() && b.is_finite() && b > a).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for range in sorted {
        match merged.last_mut() {
            Some(last) if range.0 <= last.1 + 1e-6 => last.1 = last.1.max(range.1),
            _ => merged.push(range),
        }
    }
    merged
}

/// The pauses to cut from a recording of `duration` seconds, given its speech (sorted spans,
/// see `vad::speech_regions`): each silence of at least `min_pause` loses all but `keep`
/// seconds, half kept on each side. The silence before the first and after the last speech
/// counts too, keeping half of `keep` next to the speech.
///
/// A silence with input in it (`activity`: times of key presses, clicks, scrolls and cursor
/// travel) is left alone: on a screen recording that is someone showing something, not dead
/// air. Without any speech there is nothing to measure pauses against, so nothing is cut.
pub fn pause_ranges(speech: &[(f64, f64)], duration: f64, activity: &[f64], settings: &PauseSettings) -> Vec<(f64, f64)> {
    if speech.is_empty() {
        return Vec::new();
    }
    let half = settings.keep / 2.0;
    let mut gaps: Vec<(f64, f64, bool, bool)> = Vec::new(); // start, end, leading, trailing
    gaps.push((0.0, speech[0].0, true, false));
    for pair in speech.windows(2) {
        gaps.push((pair[0].1, pair[1].0, false, false));
    }
    gaps.push((speech[speech.len() - 1].1, duration, false, true));
    let mut ranges = Vec::new();
    for (start, end, leading, trailing) in gaps {
        if end - start < settings.min_pause {
            continue;
        }
        if activity.iter().any(|&t| t >= start && t <= end) {
            continue;
        }
        let from = if leading { 0.0 } else { start + half };
        let to = if trailing { duration } else { end - half };
        if to - from > 0.05 {
            ranges.push((from, to));
        }
    }
    merge_ranges(&ranges)
}

/// Hesitation sounds, as engines spell them (compared lower case, letters only).
pub const FILLER_WORDS: &[&str] =
    &["um", "umm", "ummm", "uh", "uhh", "uhm", "erm", "er", "err", "ah", "ahh", "hm", "hmm", "hmmm", "mm", "mmm", "eh"];

/// Whether a transcript word is a filler, ignoring case and punctuation.
pub fn is_filler(word: &str) -> bool {
    let letters: String = word.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
    !letters.is_empty() && FILLER_WORDS.contains(&letters.as_str())
}

/// The filler words to cut: each filler with the hesitation around it, up to a little before
/// the next word and after the previous one, so the words either side close up naturally.
pub fn filler_ranges(transcript: &Transcript) -> Vec<(f64, f64)> {
    /// The gap kept between the words either side of a removed filler, on each side.
    const BREATH: f64 = 0.06;
    /// Silence longer than this around a filler is a pause, left to pause trimming.
    const HESITATION: f64 = 0.6;
    let words: Vec<_> = transcript.words().collect();
    let mut ranges = Vec::new();
    for (i, word) in words.iter().enumerate() {
        if !is_filler(&word.text) {
            continue;
        }
        let start = match i.checked_sub(1).map(|p| words[p]) {
            Some(previous) if word.start - previous.end < HESITATION => (previous.end + BREATH).min(word.start),
            _ => word.start - 0.03,
        };
        let end = match words.get(i + 1) {
            Some(next) if next.start - word.end < HESITATION => (next.start - BREATH).max(word.end),
            _ => word.end + 0.03,
        };
        ranges.push((start.max(0.0), end));
    }
    merge_ranges(&ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Segment, Word, WordTiming};

    #[test]
    fn merge_sorts_and_joins() {
        assert_eq!(merge_ranges(&[(3.0, 4.0), (1.0, 2.0), (1.5, 2.5), (5.0, 5.0), (7.0, 6.0)]), vec![(1.0, 2.5), (3.0, 4.0)]);
    }

    #[test]
    fn pauses_keep_some_silence_and_respect_activity() {
        let speech = [(1.5, 3.0), (3.4, 5.0), (7.0, 8.0), (12.0, 13.0)];
        let settings = PauseSettings { min_pause: 1.0, keep: 0.4 };
        let ranges = pause_ranges(&speech, 15.0, &[], &settings);
        // Leading silence keeps 0.2 s before speech, the 0.4 s gap stays, 5..7 and 8..12 shrink,
        // the tail after 13 goes.
        assert_eq!(ranges, vec![(0.0, 1.5 - 0.2), (5.2, 6.8), (8.2, 11.8), (13.2, 15.0)]);
        let ranges = pause_ranges(&speech, 15.0, &[9.5], &settings);
        assert_eq!(ranges, vec![(0.0, 1.3), (5.2, 6.8), (13.2, 15.0)]);
        assert!(pause_ranges(&[], 15.0, &[], &settings).is_empty());
    }

    #[test]
    fn fillers_match_spellings() {
        for word in ["um", "Um,", "UHH...", "erm", "hmm."] {
            assert!(is_filler(word), "{word}");
        }
        for word in ["umbrella", "a", "", "...", "her", "I"] {
            assert!(!is_filler(word), "{word}");
        }
    }

    #[test]
    fn filler_ranges_close_up_the_neighbours() {
        let words = vec![
            Word { start: 0.0, end: 0.5, text: "So".into() },
            Word { start: 0.7, end: 0.9, text: "um,".into() },
            Word { start: 1.1, end: 1.5, text: "this".into() },
            Word { start: 3.0, end: 3.2, text: "uh".into() },
            Word { start: 4.0, end: 4.3, text: "works.".into() },
        ];
        let transcript = Transcript {
            word_timing: WordTiming::Measured,
            segments: vec![Segment { start: 0.0, end: 4.3, text: String::new(), words }],
            ..Transcript::default()
        };
        let ranges = filler_ranges(&transcript);
        assert_eq!(ranges.len(), 2);
        assert!((ranges[0].0 - 0.56).abs() < 1e-9 && (ranges[0].1 - 1.04).abs() < 1e-9, "{ranges:?}");
        assert!((ranges[1].0 - 2.97).abs() < 1e-9 && (ranges[1].1 - 3.23).abs() < 1e-9, "{ranges:?}");
    }
}
