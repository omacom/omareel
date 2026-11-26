//! Captions from the transcript, on the OUTPUT timeline: each word is mapped through the clips
//! (a word in a cut part is gone, a sped-up clip shortens it), then the words that survive are
//! grouped into cues. The editor's preview and the export sample the same cues, and subtitle
//! files (SRT, WebVTT, and ASS with the spoken word lit) are written from them.
//!
//! Three styles ([`CaptionStyle`]): a phrase of up to two lines at a time; the same with the word
//! being spoken lit; or a few large words at a time, for short vertical videos. The last two
//! follow each word, so they need measured word times; with estimated ones (text-only engines)
//! captions fall back to phrases.

use crate::compat::cpp;
use crate::project::Clip;
use crate::transcript::{Transcript, WordTiming};

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
/// Characters per line in the short style, at most (a longer word gets a line of its own).
pub const SHORT_CHARS: usize = 18;
/// Words per cue in the short style, at most.
const SHORT_WORDS: usize = 3;
/// A silence this long between words starts a new cue in the short style.
const SHORT_GAP: f64 = 0.35;
/// How long a short-style cue stays after its last word, unless the next one comes first.
const SHORT_LINGER: f64 = 0.3;

/// How captions show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptionStyle {
    /// A phrase of up to two lines at a time.
    #[default]
    Phrase,
    /// The phrase, with the word being spoken in the accent colour.
    Highlight,
    /// One to three large words at a time, the spoken one lit.
    Short,
}

impl CaptionStyle {
    pub const ALL: [CaptionStyle; 3] = [CaptionStyle::Phrase, CaptionStyle::Highlight, CaptionStyle::Short];

    /// The name in project.json.
    pub fn name(self) -> &'static str {
        match self {
            CaptionStyle::Phrase => "phrase",
            CaptionStyle::Highlight => "highlight",
            CaptionStyle::Short => "short",
        }
    }

    pub fn from_name(name: &str) -> Option<CaptionStyle> {
        CaptionStyle::ALL.into_iter().find(|style| style.name() == name)
    }

    /// Whether the style follows each word, and so needs measured word times.
    pub fn follows_words(self) -> bool {
        self != CaptionStyle::Phrase
    }
}

/// The style captions really get: one that follows words falls back to phrases when the word
/// times are estimated, since a lit word off by half a second reads worse than none.
pub fn effective_style(style: CaptionStyle, transcript: &Transcript) -> CaptionStyle {
    if style.follows_words() && transcript.word_timing != WordTiming::Measured {
        CaptionStyle::Phrase
    } else {
        style
    }
}

/// One caption: its output time span, its lines, and the output times of its words, in the
/// order they appear in the lines (each line's words are separated by single spaces).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub lines: Vec<String>,
    pub words: Vec<(f64, f64)>,
    pub style: CaptionStyle,
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

/// The output spans of the inserted videos.
fn insert_spans(clips: &[Clip]) -> Vec<(f64, f64)> {
    let mut cursor = 0.0;
    let mut spans = Vec::new();
    for clip in clips {
        let length = (clip.out - clip.in_) / cpp::max(0.000001, clip.speed);
        if clip.is_media() {
            spans.push((cursor, cursor + length));
        }
        cursor += length;
    }
    spans
}

/// Words wrapped into lines of at most `width` characters (a longer word gets a line of its
/// own), filled greedily.
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

/// [`wrap`], but two lines are evened out: "Umm, the second sentence / comes after a pause."
/// rather than a lone "pause." under a full line.
pub fn wrap_balanced(words: &[&str], width: usize) -> Vec<String> {
    let lines = wrap(words, width);
    if lines.len() != 2 {
        return lines;
    }
    let join = |part: &[&str]| part.join(" ");
    (1..words.len())
        .map(|split| (join(&words[..split]), join(&words[split..])))
        .filter(|(a, b)| a.chars().count() <= width && b.chars().count() <= width)
        // The shorter longest line; on a tie, the longer first line.
        .min_by_key(|(a, b)| (a.chars().count().max(b.chars().count()), b.chars().count()))
        .map_or(lines, |(a, b)| vec![a, b])
}

fn ends_sentence(word: &str) -> bool {
    word.ends_with(['.', '?', '!'])
}

fn ends_clause(word: &str) -> bool {
    word.ends_with(['.', '?', '!', ',', ';', ':'])
}

/// The captions for a transcript under these clips, in `style` (see [`effective_style`]).
pub fn build_cues(transcript: &Transcript, clips: &[Clip], style: CaptionStyle) -> Vec<Cue> {
    let style = effective_style(style, transcript);
    let words = output_words(transcript, clips);
    // Where inserted videos play: a caption never runs across one (subtitle files would show it
    // over the insert, and the overlay hides captions there).
    let inserts = insert_spans(clips);
    let across_insert = |from: f64, to: f64| inserts.iter().any(|&(start, _)| start >= from - 1e-9 && start < to);
    let total: f64 = clips.iter().map(|clip| (clip.out - clip.in_) / cpp::max(0.000001, clip.speed)).sum();
    if style == CaptionStyle::Short {
        return short_cues(&words, total, &inserts);
    }
    let mut groups: Vec<Vec<&OutputWord>> = Vec::new();
    for word in &words {
        let starts_new = match groups.last() {
            None => true,
            Some(group) => {
                let last = group[group.len() - 1];
                let texts: Vec<&str> = group.iter().map(|w| w.text.as_str()).chain([word.text.as_str()]).collect();
                let chars: usize = group.iter().map(|w| w.text.chars().count() + 1).sum();
                // A word that starts with the cue never starts the next: two cues at one time would
                // leave the first no time to show, and its words would be lost.
                word.start > group[0].start
                    && (word.start - last.end > BREAK_GAP
                    || across_insert(last.end, word.start)
                    || wrap(&texts, LINE_CHARS).len() > MAX_LINES
                    // A new phrase, or a new sentence once there is a line's worth, starts fresh.
                    || (word.segment != last.segment && chars >= 12)
                    || (ends_sentence(&last.text) && chars >= LINE_CHARS / 2))
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
            lines: wrap_balanced(&group.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), LINE_CHARS),
            words: group.iter().map(|w| (w.start, w.end)).collect(),
            style,
        })
        .collect();
    // Linger after the last word, and show long enough to read, without running into the next
    // or past the end.
    for i in 0..cues.len() {
        let next = cues.get(i + 1).map_or(total, |cue| cue.start);
        // Nor does it linger into an insert.
        let next = inserts.iter().map(|&(start, _)| start).filter(|&start| start >= cues[i].end - 1e-9).fold(next, f64::min);
        let cue = &mut cues[i];
        let wanted = (cue.end + LINGER).max(cue.start + MIN_SHOW);
        cue.end = wanted.min(next).max(cue.end.min(next));
    }
    cues.retain(|cue| cue.end > cue.start);
    cues
}

/// The short style's cues: up to three words and [`SHORT_CHARS`] a line, a new cue at each
/// pause, clause end or phrase. Each shows until the next starts, or briefly after its last word
/// before a pause, so the words keep pace with the voice.
fn short_cues(words: &[OutputWord], total: f64, inserts: &[(f64, f64)]) -> Vec<Cue> {
    let mut groups: Vec<Vec<&OutputWord>> = Vec::new();
    for word in words {
        let starts_new = match groups.last() {
            None => true,
            Some(group) => {
                let last = group[group.len() - 1];
                let chars: usize = group.iter().map(|w| w.text.chars().count() + 1).sum::<usize>() + word.text.chars().count();
                word.start > group[0].start
                    && (group.len() >= SHORT_WORDS
                        || inserts.iter().any(|&(start, _)| start >= last.end - 1e-9 && start < word.start)
                        || chars > SHORT_CHARS
                        || word.start - last.end > SHORT_GAP
                        || word.segment != last.segment
                        || ends_clause(&last.text))
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
            lines: wrap(&group.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), SHORT_CHARS),
            words: group.iter().map(|w| (w.start, w.end)).collect(),
            style: CaptionStyle::Short,
        })
        .collect();
    for i in 0..cues.len() {
        let next = cues.get(i + 1).map_or(total, |cue| cue.start);
        let next = inserts.iter().map(|&(start, _)| start).filter(|&start| start >= cues[i].end - 1e-9).fold(next, f64::min);
        let cue = &mut cues[i];
        // Back to back while speech runs on; a short hold before a pause.
        let wanted = if next - cue.end <= SHORT_GAP + SHORT_LINGER { next } else { cue.end + SHORT_LINGER };
        cue.end = wanted.min(next).max(cue.end.min(next));
    }
    cues.retain(|cue| cue.end > cue.start);
    cues
}

/// What shows at an output time. `active` is the index, among the cue's words, of the one being
/// spoken (the last one started), when the style lights it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptionFrame {
    pub lines: Vec<String>,
    pub opacity: f64,
    pub active: Option<usize>,
    pub style: CaptionStyle,
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
    let active = if cue.style.follows_words() { active_word(&cue.words, output_time) } else { None };
    CaptionFrame { lines: cue.lines.clone(), opacity, active, style: cue.style }
}

/// The word being spoken at `time`: the last one started, until a short while after the cue's
/// last word ends.
fn active_word(words: &[(f64, f64)], time: f64) -> Option<usize> {
    let index = words.partition_point(|word| word.0 <= time).checked_sub(1)?;
    (index + 1 < words.len() || time < words[index].1 + SHORT_LINGER).then_some(index)
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
        // Cue text is markup: escape it ("R&D", "<div>"), which also keeps out "-->".
        let text = cue.lines.join("\n").replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        out.push_str(&format!("{} --> {}\n{}\n\n", timestamp(cue.start, '.'), timestamp(cue.end, '.'), text));
    }
    out
}

/// Text size in pixels at 1080 high, before the project's `size`, in a frame `aspect` wide
/// over high: what CaptionOverlay.qml draws with. Short captions shrink in a portrait frame, to
/// three quarters at most, so a few words still fit its width.
pub fn font_px(style: CaptionStyle, aspect: f64) -> f64 {
    if style == CaptionStyle::Short { 68.0 * aspect.clamp(0.75, 1.0) } else { 36.0 }
}

/// How much of the foreground's opacity the other words keep while one is lit.
pub const UNLIT_OPACITY: f64 = 0.62;

/// How an ASS file draws its captions: the frame's shape and the overlay's settings and colours.
#[derive(Clone, Debug, PartialEq)]
pub struct AssLook {
    /// Output width over height.
    pub aspect: f64,
    /// The project's caption size, 0.5 to 2.
    pub size: f64,
    pub top: bool,
    pub foreground: crate::compat::qcolor::QColor,
    pub background: crate::compat::qcolor::QColor,
    pub accent: crate::compat::qcolor::QColor,
}

impl AssLook {
    /// The look of the project's captions in the current Omarchy theme.
    pub fn themed(aspect: f64, settings: &crate::project::Captions) -> AssLook {
        let (background, foreground) = crate::theme::render_colors();
        AssLook {
            aspect,
            size: settings.size,
            top: settings.position == "top",
            foreground,
            background,
            accent: crate::theme::render_accent(),
        }
    }
}

/// The subtitle file for `extension` ("srt", "vtt" or "ass", any case), or `None` for another.
pub fn subtitle_file(extension: &str, cues: &[Cue], look: &AssLook) -> Option<String> {
    match extension.to_lowercase().as_str() {
        "srt" => Some(to_srt(cues)),
        "vtt" => Some(to_vtt(cues)),
        "ass" => Some(to_ass(cues, look)),
        _ => None,
    }
}

/// `&HAABBGGRR`, ASS's colour, with `opacity` 0 to 1.
fn ass_colour(colour: &crate::compat::qcolor::QColor, opacity: f64) -> String {
    let alpha = (255.0 * (1.0 - opacity.clamp(0.0, 1.0))).round() as i32;
    format!("&H{alpha:02X}{:02X}{:02X}{:02X}", colour.blue(), colour.green(), colour.red())
}

fn ass_time(seconds: f64) -> String {
    let total = (seconds.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", total / 360_000, total / 6000 % 60, total / 100 % 60, total % 100)
}

/// Text for an ASS event: its override braces and backslashes taken out.
fn ass_text(text: &str) -> String {
    text.replace('\\', "/").replace('{', "(").replace('}', ")")
}

/// Advanced SubStation Alpha (`.ass`): the captions as the export draws them, in a box of the
/// theme's background, with the spoken word in the accent colour for the styles that light it.
/// mpv, VLC, ffmpeg's `subtitles` filter and most video editors read it.
pub fn to_ass(cues: &[Cue], look: &AssLook) -> String {
    let height = 1080.0;
    let width = (height * look.aspect.max(0.1)).round();
    let style = cues.first().map_or(CaptionStyle::Phrase, |cue| cue.style);
    let size = look.size.clamp(0.5, 2.0);
    let font = (font_px(style, look.aspect) * size).round();
    let (alignment, margin) = match (style, look.top) {
        (CaptionStyle::Short, false) => (2, height * 0.22),
        (CaptionStyle::Short, true) => (8, height * 0.18),
        (_, false) => (2, 110.0 + 10.0 * size),
        (_, true) => (8, 40.0 + 10.0 * size),
    };
    let box_colour = ass_colour(&look.background, 0.85);
    let mut out = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {width}\nPlayResY: {height}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, \
         Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,Sans,{font},{fg},{fg},{box_colour},{box_colour},-1,0,0,0,100,100,0,0,3,{pad},0,{alignment},60,60,{margin},1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        fg = ass_colour(&look.foreground, 1.0),
        pad = (12.0 * size).round(),
        margin = margin.round(),
    );
    let accent = ass_colour(&look.accent, 1.0);
    let foreground = ass_colour(&look.foreground, 1.0);
    // While a word is lit the others step back, as in the export.
    let unlit_alpha = format!("&H{:02X}&", (255.0 * (1.0 - UNLIT_OPACITY)).round() as i32);
    let mut event = |start: f64, end: f64, text: String| {
        if end - start >= 0.005 {
            out.push_str(&format!("Dialogue: 0,{},{},Default,,0,0,0,,{text}\n", ass_time(start), ass_time(end)));
        }
    };
    for cue in cues {
        let lines: Vec<Vec<String>> = cue.lines.iter().map(|line| line.split(' ').map(ass_text).collect()).collect();
        let text = |active: Option<usize>| {
            let mut index = 0;
            let dim = if active.is_some() { format!("{{\\1a{unlit_alpha}}}") } else { String::new() };
            dim + &lines
                .iter()
                .map(|words| {
                    words
                        .iter()
                        .map(|word| {
                            let lit = active == Some(index);
                            index += 1;
                            if lit {
                                format!("{{\\1c{accent}\\1a&H00&}}{word}{{\\1c{foreground}\\1a{unlit_alpha}}}")
                            } else {
                                word.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("\\N")
        };
        if !cue.style.follows_words() || cue.words.is_empty() {
            event(cue.start, cue.end, text(None));
            continue;
        }
        // One event per lit word, from its start to the next word's start.
        event(cue.start, cue.words[0].0.max(cue.start), text(None));
        for (i, word) in cue.words.iter().enumerate() {
            let from = word.0.max(cue.start);
            let to = cue.words.get(i + 1).map_or(cue.end, |next| next.0).min(cue.end);
            if i + 1 == cue.words.len() {
                let lit_until = (word.1 + SHORT_LINGER).min(cue.end);
                event(from, lit_until, text(Some(i)));
                event(lit_until, cue.end, text(None));
            } else {
                event(from, to, text(Some(i)));
            }
        }
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
        let cues = build_cues(&t, &[clip("a", 0.0, 20.0, 1.0)], CaptionStyle::Phrase);
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
        let cues = build_cues(&t, &[clip("a", 0.0, 10.0, 1.0)], CaptionStyle::Phrase);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].lines, vec!["Hi there."]);
        assert!((cues[0].end - 1.2).abs() < 1e-9, "{cues:?}");
        assert!((cues[1].end - 4.0).abs() < 1e-9, "{cues:?}");
        // Cut out the pause and speed up: "Again" now follows at once, in the same caption.
        let cues = build_cues(&t, &[clip("a", 0.0, 0.8, 1.0), clip("b", 2.9, 10.0, 2.0)], CaptionStyle::Phrase);
        assert_eq!(cues.len(), 1, "{cues:?}");
        assert_eq!(cues[0].lines, vec!["Hi there. Again"]);
        assert!((cues[0].end - (0.8 + 0.25 + 0.6)).abs() < 1e-9, "{cues:?}");
    }

    #[test]
    fn sampling_fades() {
        let cues = vec![
            Cue { start: 1.0, end: 2.0, lines: vec!["a".into()], ..Cue::default() },
            Cue { start: 2.0, end: 3.0, lines: vec!["b".into()], ..Cue::default() },
        ];
        assert_eq!(sample(&cues, 0.5), CaptionFrame::default());
        assert_eq!(sample(&cues, 1.0).opacity, 0.0);
        assert_eq!(sample(&cues, 1.06), CaptionFrame { lines: vec!["a".into()], opacity: 0.5000000000000004, ..CaptionFrame::default() });
        assert_eq!(sample(&cues, 1.99).opacity, 1.0);
        assert_eq!(sample(&cues, 2.0), CaptionFrame { lines: vec!["b".into()], opacity: 1.0, ..CaptionFrame::default() });
        assert!((sample(&cues, 2.94).opacity - 0.5).abs() < 1e-6);
        assert_eq!(sample(&cues, 3.0), CaptionFrame::default());
    }

    #[test]
    fn subtitle_formats() {
        let cues = vec![
            Cue { start: 0.5, end: 2.25, lines: vec!["Hello".into(), "there".into()], ..Cue::default() },
            Cue { start: 3661.001, end: 3662.0, lines: vec!["a --> b".into()], ..Cue::default() },
            Cue { start: 4000.0, end: 4001.0, lines: vec!["R&D <b>is</b> fun".into()], ..Cue::default() },
        ];
        assert_eq!(
            to_srt(&cues),
            "1\n00:00:00,500 --> 00:00:02,250\nHello\nthere\n\n2\n01:01:01,001 --> 01:01:02,000\na --> b\n\n\
             3\n01:06:40,000 --> 01:06:41,000\nR&D <b>is</b> fun\n\n"
        );
        assert_eq!(
            to_vtt(&cues),
            "WEBVTT\n\n00:00:00.500 --> 00:00:02.250\nHello\nthere\n\n01:01:01.001 --> 01:01:02.000\na --&gt; b\n\n\
             01:06:40.000 --> 01:06:41.000\nR&amp;D &lt;b&gt;is&lt;/b&gt; fun\n\n"
        );
    }

    fn measured(words: &[(f64, f64, &str)]) -> Transcript {
        Transcript { word_timing: WordTiming::Measured, ..transcript(words) }
    }

    #[test]
    fn style_names_and_fallback() {
        for style in CaptionStyle::ALL {
            assert_eq!(CaptionStyle::from_name(style.name()), Some(style));
        }
        assert_eq!(CaptionStyle::from_name("karaoke"), None);
        let words = [(0.0, 0.3, "Hi"), (0.4, 0.6, "there.")];
        // Estimated word times can't light words: phrases instead.
        assert_eq!(effective_style(CaptionStyle::Short, &transcript(&words)), CaptionStyle::Phrase);
        assert_eq!(effective_style(CaptionStyle::Highlight, &measured(&words)), CaptionStyle::Highlight);
        let cues = build_cues(&transcript(&words), &[clip("a", 0.0, 10.0, 1.0)], CaptionStyle::Short);
        assert_eq!(cues[0].style, CaptionStyle::Phrase);
        assert_eq!(sample(&cues, 0.45).active, None);
    }

    #[test]
    fn two_lines_are_balanced() {
        let words: Vec<&str> = "Umm, the second sentence comes after a long pause in the talk.".split(' ').collect();
        assert_eq!(wrap(&words, 42), vec!["Umm, the second sentence comes after a", "long pause in the talk."]);
        assert_eq!(wrap_balanced(&words, 42), vec!["Umm, the second sentence comes", "after a long pause in the talk."]);
        assert_eq!(wrap_balanced(&["one", "two"], 42), vec!["one two"]);
    }

    #[test]
    fn highlight_lights_the_spoken_word() {
        let t = measured(&[(1.0, 1.3, "Hello"), (1.4, 1.8, "there,"), (2.5, 2.9, "friend.")]);
        let cues = build_cues(&t, &[clip("a", 0.0, 10.0, 1.0)], CaptionStyle::Highlight);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].lines, vec!["Hello there, friend."]);
        assert_eq!(cues[0].words.len(), 3);
        let active = |time: f64| sample(&cues, time).active;
        assert_eq!(active(1.1), Some(0));
        assert_eq!(active(1.5), Some(1));
        // Between words the last one spoken stays lit.
        assert_eq!(active(2.2), Some(1));
        assert_eq!(active(2.6), Some(2));
        // After the last word, briefly, then the caption lingers unlit.
        assert_eq!(active(3.1), Some(2));
        assert_eq!(active(3.3), None);
        assert_eq!(sample(&cues, 2.6).style, CaptionStyle::Highlight);
        // Phrase captions light nothing.
        let phrase = build_cues(&t, &[clip("a", 0.0, 10.0, 1.0)], CaptionStyle::Phrase);
        assert_eq!(sample(&phrase, 1.5).active, None);
    }

    #[test]
    fn short_cues_keep_pace() {
        let t = measured(&[
            (0.0, 0.2, "So"),
            (0.25, 0.5, "today"),
            (0.55, 0.7, "we"),
            (0.75, 1.0, "export,"),
            (1.05, 1.5, "captions"),
            (1.55, 1.7, "and"),
            (3.0, 3.6, "extraordinarily"),
            (3.65, 4.2, "long-winded"),
            (4.25, 4.5, "words."),
        ]);
        let cues = build_cues(&t, &[clip("a", 0.0, 10.0, 1.0)], CaptionStyle::Short);
        let lines: Vec<String> = cues.iter().map(|c| c.lines.join(" / ")).collect();
        // Three words at most, a break after a comma and at the pause, long words alone.
        assert_eq!(lines, vec!["So today we", "export,", "captions and", "extraordinarily", "long-winded words."]);
        assert!(cues.iter().all(|c| c.style == CaptionStyle::Short && c.lines.len() == 1));
        // Back to back while speech runs on; a short hold before the pause.
        assert_eq!(cues[0].end, cues[1].start);
        assert!((cues[2].end - 2.0).abs() < 1e-9, "{:?}", cues[2]);
        assert_eq!(sample(&cues, 0.6).active, Some(2));
        // The cue after the pause doesn't fade in from nothing within a back-to-back run.
        assert_eq!(sample(&cues, cues[4].start).opacity, 1.0);
    }

    #[test]
    fn ass_lights_words_in_turn() {
        let t = measured(&[(1.0, 1.3, "Hello"), (1.4, 1.8, "{there}")]);
        let cues = build_cues(&t, &[clip("a", 0.0, 3.0, 1.0)], CaptionStyle::Highlight);
        let look = AssLook {
            aspect: 9.0 / 16.0,
            size: 1.0,
            top: false,
            foreground: crate::compat::qcolor::QColor::parse("#ffffff"),
            background: crate::compat::qcolor::QColor::parse("#000000"),
            accent: crate::compat::qcolor::QColor::parse("#7aa2f7"),
        };
        let ass = to_ass(&cues, &look);
        assert!(ass.contains("PlayResX: 608\nPlayResY: 1080\n"), "{ass}");
        assert!(ass.contains("Style: Default,Sans,36,&H00FFFFFF,&H00FFFFFF,&H26000000,&H26000000,"), "{ass}");
        let events: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(
            events,
            vec![
                "Dialogue: 0,0:00:01.00,0:00:01.40,Default,,0,0,0,,{\\1a&H61&}{\\1c&H00F7A27A\\1a&H00&}Hello{\\1c&H00FFFFFF\\1a&H61&} (there)",
                "Dialogue: 0,0:00:01.40,0:00:02.10,Default,,0,0,0,,{\\1a&H61&}Hello {\\1c&H00F7A27A\\1a&H00&}(there){\\1c&H00FFFFFF\\1a&H61&}",
                "Dialogue: 0,0:00:02.10,0:00:02.40,Default,,0,0,0,,Hello (there)",
            ]
        );
        let phrase = to_ass(&build_cues(&t, &[clip("a", 0.0, 3.0, 1.0)], CaptionStyle::Phrase), &look);
        assert_eq!(phrase.lines().filter(|l| l.starts_with("Dialogue:")).count(), 1);
    }


    /// Words that share a start time (an engine's rounding) stay in one cue, so none is lost to
    /// a cue with no time to show.
    #[test]
    fn words_at_one_time_are_never_dropped() {
        let t = measured(&[(1.0, 1.0, "one."), (1.0, 1.2, "two,"), (1.0, 1.3, "three"), (1.4, 1.6, "four")]);
        for style in CaptionStyle::ALL {
            let cues = build_cues(&t, &[clip("a", 0.0, 10.0, 1.0)], style);
            let shown: usize = cues.iter().map(|c| c.lines.join(" ").split(' ').count()).sum();
            assert_eq!(shown, 4, "{style:?}: {cues:?}");
            assert!(cues.iter().all(|c| c.end > c.start), "{style:?}: {cues:?}");
        }
    }

    /// A caption never runs across an inserted video, nor lingers into it.
    #[test]
    fn captions_stop_at_an_insert() {
        let t = transcript(&[(0.0, 0.3, "Before"), (0.4, 0.8, "the"), (1.0, 1.3, "insert")]);
        let insert = Clip { media: "media/a.mp4".into(), ..clip("m", 0.0, 2.0, 1.0) };
        // Recording 0..0.9, the insert at 0.9..2.9, then recording 0.9..5.
        let clips = [clip("a", 0.0, 0.9, 1.0), insert, clip("b", 0.9, 5.0, 1.0)];
        let cues = build_cues(&t, &clips, CaptionStyle::Phrase);
        assert_eq!(cues.len(), 2, "{cues:?}");
        let short = build_cues(&measured(&[(0.0, 0.3, "Before"), (0.4, 0.8, "the"), (1.0, 1.3, "insert")]), &clips, CaptionStyle::Short);
        assert!(short.iter().all(|cue| cue.end <= 0.9 + 1e-9 || cue.start >= 2.9 - 1e-9), "{short:?}");
        assert_eq!(cues[0].lines, vec!["Before the"]);
        assert!(cues[0].end <= 0.9 + 1e-9, "{cues:?}");
        assert!(cues[1].start >= 2.9 - 1e-9, "{cues:?}");
    }

}
