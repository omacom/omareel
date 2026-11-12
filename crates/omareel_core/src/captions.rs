//! Captions from the transcript, on the OUTPUT timeline: each word is mapped through the clips
//! (a word in a cut part is gone, a sped-up clip shortens it), then the words that survive are
//! grouped into short cues of at most two lines. The editor's preview and the export sample the
//! same cues, and subtitle files (SRT, WebVTT) are written from them.

use crate::compat::cpp;
use crate::project::Clip;
use crate::transcript::Transcript;

/// Characters per caption line, at most.
pub const LINE_CHARS: usize = 42;
/// Lines per caption, at most.
pub const MAX_LINES: usize = 2;
/// A silence this long (output seconds) between words starts a new caption.
const BREAK_GAP: f64 = 0.8;
/// How long a caption stays after its last word, unless the next one comes first.
const LINGER: f64 = 0.6;
/// The shortest time a caption shows when there is room.
const MIN_SHOW: f64 = 0.9;
/// Fade in and out.
pub const FADE: f64 = 0.12;

/// One caption: its output time span and its lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub lines: Vec<String>,
}

/// A transcript word placed on the output timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputWord {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// Index of the transcript segment it came from.
    pub segment: usize,
}

/// The words still in the edit, in output order: a word stays when the middle of it is in a
/// recording clip, and is clamped to that clip.
pub fn output_words(transcript: &Transcript, clips: &[Clip]) -> Vec<OutputWord> {
    // Output start of each clip.
    let mut starts = Vec::with_capacity(clips.len());
    let mut cursor = 0.0;
    for clip in clips {
        starts.push(cursor);
        cursor += (clip.out - clip.in_) / cpp::max(0.000001, clip.speed);
    }
    let mut words = Vec::new();
    for (segment_index, segment) in transcript.segments.iter().enumerate() {
        for word in &segment.words {
            let middle = (word.start + word.end) / 2.0;
            for (clip, start) in clips.iter().zip(&starts) {
                if clip.is_media() || middle < clip.in_ || middle >= clip.out {
                    continue;
                }
                let speed = cpp::max(0.000001, clip.speed);
                let map = |t: f64| start + (cpp::clamp(t, clip.in_, clip.out) - clip.in_) / speed;
                words.push(OutputWord {
                    start: map(word.start),
                    end: map(word.end),
                    text: word.text.clone(),
                    segment: segment_index,
                });
            }
        }
    }
    words.sort_by(|a, b| a.start.total_cmp(&b.start));
    words
}

/// Words wrapped into lines of at most `width` characters (a longer word gets a line of its
/// own).
pub fn wrap(words: &[&str], width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in words {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push((*word).to_owned()),
        }
    }
    lines
}

fn ends_sentence(word: &str) -> bool {
    word.ends_with(['.', '?', '!'])
}

/// The captions for a transcript under these clips.
pub fn build_cues(transcript: &Transcript, clips: &[Clip]) -> Vec<Cue> {
    let words = output_words(transcript, clips);
    let mut groups: Vec<Vec<&OutputWord>> = Vec::new();
    for word in &words {
        let starts_new = match groups.last() {
            None => true,
            Some(group) => {
                let last = group[group.len() - 1];
                let texts: Vec<&str> = group.iter().map(|w| w.text.as_str()).chain([word.text.as_str()]).collect();
                let chars: usize = group.iter().map(|w| w.text.chars().count() + 1).sum();
                word.start - last.end > BREAK_GAP
                    || wrap(&texts, LINE_CHARS).len() > MAX_LINES
                    // A new phrase, or a new sentence once there is a line's worth, starts fresh.
                    || (word.segment != last.segment && chars >= 12)
                    || (ends_sentence(&last.text) && chars >= LINE_CHARS / 2)
            }
        };
        if starts_new {
            groups.push(vec![word]);
        } else if let Some(group) = groups.last_mut() {
            group.push(word);
        }
    }
    let mut cues: Vec<Cue> = groups
        .iter()
        .map(|group| Cue {
            start: group[0].start,
            end: group.iter().map(|w| w.end).fold(group[0].end, f64::max),
            lines: wrap(&group.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), LINE_CHARS),
        })
        .collect();
    // Linger after the last word, and show long enough to read, without running into the next
    // or past the end.
    let total: f64 = clips.iter().map(|clip| (clip.out - clip.in_) / cpp::max(0.000001, clip.speed)).sum();
    for i in 0..cues.len() {
        let next = cues.get(i + 1).map_or(total, |cue| cue.start);
        let cue = &mut cues[i];
        let wanted = (cue.end + LINGER).max(cue.start + MIN_SHOW);
        cue.end = wanted.min(next).max(cue.end.min(next));
    }
    cues.retain(|cue| cue.end > cue.start);
    cues
}

/// What shows at an output time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptionFrame {
    pub lines: Vec<String>,
    pub opacity: f64,
}

/// The caption at `output_time`, fading in and out; no lines between captions.
pub fn sample(cues: &[Cue], output_time: f64) -> CaptionFrame {
    // Cues are sorted and apart: the last one starting at or before the time.
    let index = cues.partition_point(|cue| cue.start <= output_time);
    let Some(cue) = index.checked_sub(1).map(|i| &cues[i]) else { return CaptionFrame::default() };
    if output_time >= cue.end {
        return CaptionFrame::default();
    }
    let fade_in = cpp::clamp((output_time - cue.start) / FADE, 0.0, 1.0);
    let fade_out = cpp::clamp((cue.end - output_time) / FADE, 0.0, 1.0);
    // Back-to-back cues swap without fading through nothing.
    let joined_before = index >= 2 && (cues[index - 2].end - cue.start).abs() < 1e-6;
    let joined_after = cues.get(index).is_some_and(|next| (next.start - cue.end).abs() < 1e-6);
    let opacity = (if joined_before { 1.0 } else { fade_in }).min(if joined_after { 1.0 } else { fade_out });
    CaptionFrame { lines: cue.lines.clone(), opacity }
}

fn timestamp(seconds: f64, separator: char) -> String {
    let total = (seconds.max(0.0) * 1000.0).round() as u64;
    let (hours, minutes, secs, millis) = (total / 3_600_000, total / 60_000 % 60, total / 1000 % 60, total % 1000);
    format!("{hours:02}:{minutes:02}:{secs:02}{separator}{millis:03}")
}

/// SubRip (`.srt`).
pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            timestamp(cue.start, ','),
            timestamp(cue.end, ','),
            cue.lines.join("\n")
        ));
    }
    out
}

/// WebVTT (`.vtt`).
pub fn to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        // "-->" may not appear in a cue's text.
        let text = cue.lines.join("\n").replace("-->", "->");
        out.push_str(&format!("{} --> {}\n{}\n\n", timestamp(cue.start, '.'), timestamp(cue.end, '.'), text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Segment, Word};

    fn transcript(words: &[(f64, f64, &str)]) -> Transcript {
        Transcript {
            segments: vec![Segment {
                start: words[0].0,
                end: words[words.len() - 1].1,
                text: String::new(),
                words: words.iter().map(|&(start, end, text)| Word { start, end, text: text.into() }).collect(),
            }],
            ..Transcript::default()
        }
    }

    fn clip(id: &str, in_: f64, out: f64, speed: f64) -> Clip {
        Clip { id: id.into(), in_, out, speed, ..Clip::default() }
    }

    #[test]
    fn words_map_through_cuts_and_speed() {
        let t = transcript(&[(1.0, 1.5, "one"), (3.0, 3.5, "two"), (6.0, 7.0, "three"), (9.0, 9.4, "four")]);
        // 0..2 at 1x, 2..5 cut, 5..10 at 2x; an inserted video of 1 s between.
        let insert = Clip { media: "media/a.mp4".into(), ..clip("m", 0.0, 1.0, 1.0) };
        let clips = [clip("a", 0.0, 2.0, 1.0), insert, clip("b", 5.0, 10.0, 2.0)];
        let words = output_words(&t, &clips);
        let got: Vec<(f64, f64, &str)> = words.iter().map(|w| (w.start, w.end, w.text.as_str())).collect();
        // "two" is cut; "three" starts at 2 + 1 + (6 - 5) / 2 = 3.5.
        assert_eq!(got, vec![(1.0, 1.5, "one"), (3.5, 4.0, "three"), (5.0, 5.2, "four")]);
    }

    #[test]
    fn wrapping_and_cue_limits() {
        assert_eq!(wrap(&["aaa", "bbb", "ccc"], 7), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap(&["averyveryverylongword", "b"], 7), vec!["averyveryverylongword", "b"]);
        let many: Vec<(f64, f64, String)> = (0..40).map(|i| (i as f64 * 0.3, i as f64 * 0.3 + 0.25, format!("word{i}"))).collect();
        let t = transcript(&many.iter().map(|(a, b, s)| (*a, *b, s.as_str())).collect::<Vec<_>>());
        let cues = build_cues(&t, &[clip("a", 0.0, 20.0, 1.0)]);
        assert!(cues.len() > 3);
        for cue in &cues {
            assert!(cue.lines.len() <= MAX_LINES && cue.lines.iter().all(|l| l.chars().count() <= LINE_CHARS), "{cue:?}");
        }
        for pair in cues.windows(2) {
            assert!(pair[0].end <= pair[1].start + 1e-9);
        }
        // Every word shows exactly once.
        let shown: usize = cues.iter().map(|c| c.lines.join(" ").split(' ').count()).sum();
        assert_eq!(shown, 40);
    }

    #[test]
    fn pauses_break_cues_and_cues_linger() {
        let t = transcript(&[(0.0, 0.3, "Hi"), (0.4, 0.6, "there."), (3.0, 3.4, "Again")]);
        let cues = build_cues(&t, &[clip("a", 0.0, 10.0, 1.0)]);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].lines, vec!["Hi there."]);
        assert!((cues[0].end - 1.2).abs() < 1e-9, "{cues:?}");
        assert!((cues[1].end - 4.0).abs() < 1e-9, "{cues:?}");
        // Cut out the pause and speed up: "Again" now follows at once, in the same caption.
        let cues = build_cues(&t, &[clip("a", 0.0, 0.8, 1.0), clip("b", 2.9, 10.0, 2.0)]);
        assert_eq!(cues.len(), 1, "{cues:?}");
        assert_eq!(cues[0].lines, vec!["Hi there. Again"]);
        assert!((cues[0].end - (0.8 + 0.25 + 0.6)).abs() < 1e-9, "{cues:?}");
    }

    #[test]
    fn sampling_fades() {
        let cues = vec![
            Cue { start: 1.0, end: 2.0, lines: vec!["a".into()] },
            Cue { start: 2.0, end: 3.0, lines: vec!["b".into()] },
        ];
        assert_eq!(sample(&cues, 0.5), CaptionFrame::default());
        assert_eq!(sample(&cues, 1.0).opacity, 0.0);
        assert_eq!(sample(&cues, 1.06), CaptionFrame { lines: vec!["a".into()], opacity: 0.5000000000000004 });
        assert_eq!(sample(&cues, 1.99).opacity, 1.0);
        assert_eq!(sample(&cues, 2.0), CaptionFrame { lines: vec!["b".into()], opacity: 1.0 });
        assert!((sample(&cues, 2.94).opacity - 0.5).abs() < 1e-6);
        assert_eq!(sample(&cues, 3.0), CaptionFrame::default());
    }

    #[test]
    fn subtitle_formats() {
        let cues = vec![
            Cue { start: 0.5, end: 2.25, lines: vec!["Hello".into(), "there".into()] },
            Cue { start: 3661.001, end: 3662.0, lines: vec!["a --> b".into()] },
        ];
        assert_eq!(to_srt(&cues), "1\n00:00:00,500 --> 00:00:02,250\nHello\nthere\n\n2\n01:01:01,001 --> 01:01:02,000\na --> b\n\n");
        assert_eq!(
            to_vtt(&cues),
            "WEBVTT\n\n00:00:00.500 --> 00:00:02.250\nHello\nthere\n\n01:01:01.001 --> 01:01:02.000\na -> b\n\n"
        );
    }
}
