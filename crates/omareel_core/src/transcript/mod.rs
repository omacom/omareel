//! Speech to text for recordings: the bundle's `transcript.json`, the speech finder that times
//! it, the speech engines that write it (Omarchy's dictation backends), and the source ranges
//! the editor removes to trim pauses and filler words.
//!
//! Every time here is in SOURCE seconds (the recording's own, as in `screen.mp4`), so edits to
//! the clips never invalidate the transcript; captions and subtitles map them through the clips
//! (`crate::captions`).

pub mod audio;
pub mod backend;
pub mod command;
pub mod cache;
pub mod job;
pub mod speedup;
pub mod stillness;
pub mod trim;
pub mod vad;

use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::{qdir, qfile, qsavefile};
use std::path::Path;

/// The file name inside a bundle.
pub const FILE_NAME: &str = "transcript.json";

/// The bundle's transcript path.
pub fn transcript_path(bundle: &str) -> String {
    qdir::file_path(bundle, FILE_NAME)
}

/// How a transcript's word times came about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WordTiming {
    /// The speech engine timed each word.
    Measured,
    /// The engine gave text only: phrases are timed by the speech finder, and words spread over
    /// the phrase's speech by their length.
    #[default]
    Estimated,
}

impl WordTiming {
    pub fn name(self) -> &'static str {
        match self {
            WordTiming::Measured => "measured",
            WordTiming::Estimated => "estimated",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<Word>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    /// `voxtype`, `hex`, `superwhisper` or `whisper-cpp`.
    pub backend: String,
    /// The engine's model, as far as omareel knows it (empty when it doesn't).
    pub model: String,
    /// A language code, or empty when unknown.
    pub language: String,
    pub word_timing: WordTiming,
    pub segments: Vec<Segment>,
}

/// Seconds rounded to the millisecond, so the file stays short and stable.
fn ms(seconds: f64) -> Value {
    Value::from_f64((seconds * 1000.0).round() / 1000.0)
}

impl Transcript {
    pub const VERSION: i32 = 1;

    /// Every word in order.
    pub fn words(&self) -> impl Iterator<Item = &Word> {
        self.segments.iter().flat_map(|segment| segment.words.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(|segment| segment.words.is_empty() && segment.text.trim().is_empty())
    }

    /// The whole text, one segment per line.
    pub fn text(&self) -> String {
        self.segments.iter().map(|segment| segment.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    pub fn to_json(&self) -> Object {
        let segments: Vec<Value> = self
            .segments
            .iter()
            .map(|segment| {
                let words: Vec<Value> = segment
                    .words
                    .iter()
                    .map(|word| {
                        Object::from_iter([
                            ("start", ms(word.start)),
                            ("end", ms(word.end)),
                            ("text", Value::from(word.text.as_str())),
                        ])
                        .into()
                    })
                    .collect();
                Object::from_iter([
                    ("start", ms(segment.start)),
                    ("end", ms(segment.end)),
                    ("text", Value::from(segment.text.as_str())),
                    ("words", Value::from(words)),
                ])
                .into()
            })
            .collect();
        Object::from_iter([
            ("version", Value::from(Transcript::VERSION)),
            ("backend", Value::from(self.backend.as_str())),
            ("model", Value::from(self.model.as_str())),
            ("language", Value::from(self.language.as_str())),
            ("wordTiming", Value::from(self.word_timing.name())),
            ("segments", Value::from(segments)),
        ])
    }

    /// Reads what [`Transcript::to_json`] wrote; anything missing takes its default, and words
    /// with no text or times that are not finite are dropped.
    pub fn from_json(json: &Object) -> Transcript {
        let mut transcript = Transcript {
            backend: json.value("backend").to_string_or(""),
            model: json.value("model").to_string_or(""),
            language: json.value("language").to_string_or(""),
            word_timing: if json.value("wordTiming").to_string_or("") == "measured" {
                WordTiming::Measured
            } else {
                WordTiming::Estimated
            },
            segments: Vec::new(),
        };
        for value in json.value("segments").to_array() {
            let o = value.to_object();
            let mut segment = Segment {
                start: o.value("start").to_double(0.0),
                end: o.value("end").to_double(0.0),
                text: o.value("text").to_string_or(""),
                words: Vec::new(),
            };
            for word in o.value("words").to_array() {
                let w = word.to_object();
                let word = Word {
                    start: w.value("start").to_double(f64::NAN),
                    end: w.value("end").to_double(f64::NAN),
                    text: w.value("text").to_string_or(""),
                };
                if word.text.trim().is_empty() || !word.start.is_finite() || !word.end.is_finite() {
                    continue;
                }
                segment.words.push(word);
            }
            if !segment.start.is_finite() || !segment.end.is_finite() {
                continue;
            }
            transcript.segments.push(segment);
        }
        transcript
    }

    /// The bundle's transcript: `None` when there is none, an error when it does not read.
    pub fn load(bundle: &str) -> Result<Option<Transcript>, String> {
        let path = transcript_path(bundle);
        if !Path::new(&path).exists() {
            return Ok(None);
        }
        let bytes = qfile::read_all(&path)?;
        match qjson::from_json(&bytes) {
            Ok(Value::Object(object)) => Ok(Some(Transcript::from_json(&object))),
            Ok(_) => Err(format!("{path} is not a JSON object")),
            Err(error) => Err(format!("{path}: {}", error.error_string())),
        }
    }

    /// Writes the bundle's transcript atomically.
    pub fn save(&self, bundle: &str) -> Result<(), String> {
        qsavefile::save(&transcript_path(bundle), self.to_json().to_json(Format::Indented).as_bytes())
    }
}

/// Splits text into words on white space.
pub fn split_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

/// Words for a phrase whose engine gave text only: spread over the phrase's speech (`speech`,
/// sorted spans inside it) by their length, a space counting as one character, so a word
/// never lands in a silence the speech finder saw.
pub fn estimate_words(text: &str, speech: &[(f64, f64)]) -> Vec<Word> {
    let words = split_words(text);
    let total_speech: f64 = speech.iter().map(|(start, end)| (end - start).max(0.0)).sum();
    if words.is_empty() || total_speech <= 0.0 {
        return Vec::new();
    }
    let weights: Vec<f64> = words.iter().map(|word| word.chars().count() as f64 + 1.0).collect();
    let total_weight: f64 = weights.iter().sum();
    // Speech time (counted over the spans only) to source time.
    let place = |speech_time: f64| -> f64 {
        let mut left = speech_time;
        for &(start, end) in speech {
            let length = (end - start).max(0.0);
            if left <= length {
                return start + left;
            }
            left -= length;
        }
        speech.last().map_or(0.0, |span| span.1)
    };
    let mut result = Vec::with_capacity(words.len());
    let mut cursor = 0.0;
    for (word, weight) in words.into_iter().zip(weights) {
        let length = weight / total_weight * total_speech;
        let start = place(cursor);
        // The trailing space's share stays a gap.
        let end = place(cursor + length * (weight - 1.0).max(0.5) / weight);
        result.push(Word { start, end: end.max(start), text: word });
        cursor += length;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Transcript {
        Transcript {
            backend: "voxtype".into(),
            model: "base.en".into(),
            language: "en".into(),
            word_timing: WordTiming::Estimated,
            segments: vec![Segment {
                start: 1.0,
                end: 2.5,
                text: "Hello there.".into(),
                words: vec![
                    Word { start: 1.0, end: 1.4, text: "Hello".into() },
                    Word { start: 1.5, end: 2.5004, text: "there.".into() },
                ],
            }],
        }
    }

    #[test]
    fn json_round_trips_and_rounds_to_milliseconds() {
        let json = sample().to_json().to_json(Format::Indented);
        let read = Transcript::from_json(&qjson::object_from_json(json.as_bytes()).unwrap());
        let mut expected = sample();
        expected.segments[0].words[1].end = 2.5;
        assert_eq!(read, expected);
        assert!(json.contains("\"wordTiming\": \"estimated\""));
        assert!(json.contains("\"version\": 1"));
    }

    #[test]
    fn bad_words_are_dropped() {
        let json = br#"{"wordTiming":"measured","segments":[{"start":0,"end":1,"text":"a b","words":[
            {"start":0,"end":0.2,"text":"a"},{"start":"x","end":0.4,"text":"b"},{"start":0.5,"end":0.6,"text":" "}]}]}"#;
        let read = Transcript::from_json(&qjson::object_from_json(json).unwrap());
        assert_eq!(read.word_timing, WordTiming::Measured);
        assert_eq!(read.words().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn estimated_words_skip_silences_and_keep_order() {
        let words = estimate_words("one two three", &[(0.0, 1.0), (3.0, 4.0)]);
        assert_eq!(words.len(), 3);
        for pair in words.windows(2) {
            assert!(pair[0].end <= pair[1].start);
        }
        for word in &words {
            let inside = |t: f64| (0.0..=1.0).contains(&t) || (3.0..=4.0).contains(&t);
            assert!(inside(word.start) && inside(word.end), "{word:?}");
        }
        assert_eq!(words[0].start, 0.0);
        assert!(words[2].end > 3.0);
        assert!(estimate_words("", &[(0.0, 1.0)]).is_empty());
        assert!(estimate_words("word", &[]).is_empty());
    }
}
