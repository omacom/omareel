//! Chapters, a title and a short description for a recording: `chapters.json` in the bundle.
//!
//! Chapters are kept at **source** times (seconds in `screen.mp4`), which no edit moves, and
//! mapped onto the edited timeline whenever they are shown, so they stay right after later
//! cuts, speed changes and inserted videos. A chapter whose start was cut begins where the
//! recording picks up again after the cut; one whose whole stretch was cut disappears.
//!
//! ```json
//! {
//!     "chapters": [
//!         {"source": 0, "title": "Opening the terminal"},
//!         {"source": 14.2, "title": "Installing the theme"}
//!     ],
//!     "description": "How to install and switch themes in Omarchy.",
//!     "title": "Omarchy themes in two minutes",
//!     "version": 1
//! }
//! ```
//!
//! Every field may be missing. `omareel apply`'s `set-chapters` writes the file and
//! `omareel describe` lists it with output times; the editor shows the chapters on its ruler and
//! copies them as a YouTube description.

use crate::compat::qdir;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::project::Clip;

pub const FILE_NAME: &str = "chapters.json";
/// YouTube's shortest chapter; closer chapters are left out of the description.
pub const MIN_CHAPTER_SECONDS: f64 = 10.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chapter {
    /// Seconds in the recording (`screen.mp4`).
    pub source: f64,
    pub title: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chapters {
    pub title: String,
    pub description: String,
    /// In source order.
    pub chapters: Vec<Chapter>,
}

/// A chapter where it plays in the edited video.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputChapter {
    pub output: f64,
    pub source: f64,
    pub title: String,
}

/// chapters.json written aside by [`Chapters::stage`].
pub struct StagedChapters {
    target: String,
    /// `None` when the chapters are empty: committing removes chapters.json.
    temp: Option<String>,
}

impl StagedChapters {
    /// Moves the chapters into place (removes chapters.json when they are empty).
    pub fn commit(mut self) -> Result<(), String> {
        match self.temp.take() {
            Some(temp) => std::fs::rename(&temp, &self.target).map_err(|error| {
                let _ = std::fs::remove_file(&temp);
                format!("cannot write {}: {error}", self.target)
            }),
            None => match std::fs::remove_file(&self.target) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!("cannot remove {}: {error}", self.target)),
                _ => Ok(()),
            },
        }
    }
}

impl Drop for StagedChapters {
    fn drop(&mut self) {
        if let Some(temp) = self.temp.take() {
            let _ = std::fs::remove_file(temp);
        }
    }
}

pub fn path(bundle: &str) -> String {
    qdir::file_path(bundle, FILE_NAME)
}

impl Chapters {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.description.is_empty() && self.chapters.is_empty()
    }

    pub fn from_json(root: &Object) -> Chapters {
        let mut chapters: Vec<Chapter> = root
            .value("chapters")
            .as_array()
            .unwrap_or(&[])
            .iter()
            .filter_map(|item| {
                let item = item.as_object()?;
                let source = item.value("source").to_double(f64::NAN);
                let title = item.value("title").as_str()?.trim().to_owned();
                (source.is_finite() && source >= 0.0 && !title.is_empty()).then_some(Chapter { source, title })
            })
            .collect();
        chapters.sort_by(|a, b| a.source.total_cmp(&b.source));
        Chapters {
            title: root.value("title").to_string_or("").trim().to_owned(),
            description: root.value("description").to_string_or("").trim().to_owned(),
            chapters,
        }
    }

    pub fn to_json(&self) -> Object {
        let list = self
            .chapters
            .iter()
            .map(|chapter| {
                Object::from_iter([
                    ("source", Value::from(crate::agent::ms(chapter.source))),
                    ("title", chapter.title.as_str().into()),
                ])
                .into()
            })
            .collect();
        Object::from_iter([
            ("version", Value::from(1)),
            ("title", self.title.as_str().into()),
            ("description", self.description.as_str().into()),
            ("chapters", Value::array(list)),
        ])
    }

    /// The bundle's chapters; `None` when it has no chapters.json.
    pub fn load(bundle: &str) -> Result<Option<Chapters>, String> {
        let file = path(bundle);
        if !std::path::Path::new(&file).is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&file).map_err(|error| format!("cannot read {file}: {error}"))?;
        let root = qjson::object_from_json(&bytes).ok_or_else(|| format!("{file} is not a JSON object"))?;
        Ok(Some(Chapters::from_json(&root)))
    }

    /// Writes chapters.json aside, to move into place with [`StagedChapters::commit`] once
    /// whatever goes with it is saved; dropped uncommitted, the file is left as it was.
    pub fn stage(&self, bundle: &str) -> Result<StagedChapters, String> {
        let target = path(bundle);
        if self.is_empty() {
            return Ok(StagedChapters { target, temp: None });
        }
        let temp = format!("{target}.{}.tmp", std::process::id());
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(self.to_json().to_json(Format::Indented).as_bytes())?;
            file.sync_all()
        };
        if let Err(error) = write() {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("cannot write {temp}: {error}"));
        }
        Ok(StagedChapters { target, temp: Some(temp) })
    }

    /// Writes chapters.json, or removes it when there is nothing to keep.
    pub fn save(&self, bundle: &str) -> Result<(), String> {
        let file = path(bundle);
        if self.is_empty() {
            return match std::fs::remove_file(&file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!("cannot remove {file}: {error}")),
                _ => Ok(()),
            };
        }
        crate::compat::qsavefile::save(&file, self.to_json().to_json(Format::Indented).as_bytes())
    }

    /// The chapters on the edited timeline, in output order. A chapter starts at the first place
    /// its source time plays; when that moment was cut, where the recording continues after
    /// it. Chapters that land on the same moment (everything between them was cut) keep the
    /// later one, whose title describes what plays there.
    pub fn on_timeline(&self, clips: &[Clip]) -> Vec<OutputChapter> {
        let mut placed: Vec<OutputChapter> = self
            .chapters
            .iter()
            .filter_map(|chapter| {
                output_time(clips, chapter.source).map(|output| OutputChapter {
                    output,
                    source: chapter.source,
                    title: chapter.title.clone(),
                })
            })
            .collect();
        placed.sort_by(|a, b| a.output.total_cmp(&b.output).then(a.source.total_cmp(&b.source)));
        let mut result: Vec<OutputChapter> = Vec::with_capacity(placed.len());
        for chapter in placed {
            match result.last_mut() {
                Some(last) if (chapter.output - last.output).abs() < 0.05 => *last = chapter,
                _ => result.push(chapter),
            }
        }
        result
    }

    /// The text to paste into YouTube's description box: the title, the description, and a
    /// `00:00 Title` line per chapter. The first chapter starts at 00:00, and chapters shorter
    /// than [`MIN_CHAPTER_SECONDS`] are left out, as YouTube needs.
    pub fn youtube_description(&self, clips: &[Clip], duration: f64) -> String {
        let mut parts = Vec::new();
        if !self.title.is_empty() {
            parts.push(self.title.clone());
        }
        if !self.description.is_empty() {
            parts.push(self.description.clone());
        }
        let lines: Vec<String> = youtube_chapters(&self.on_timeline(clips), duration)
            .iter()
            .map(|(time, title)| format!("{} {title}", timestamp(*time, duration)))
            .collect();
        if !lines.is_empty() {
            parts.push(lines.join("\n"));
        }
        let mut text = parts.join("\n\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }
}

/// The output time a source time plays at, or where the recording continues after it when it
/// was cut. `None` once nothing of the recording plays after it.
pub fn output_time(clips: &[Clip], source: f64) -> Option<f64> {
    let mut cursor = 0.0;
    let mut after: Option<(f64, f64)> = None;
    for clip in clips {
        let length = (clip.out - clip.in_) / clip.speed;
        if !clip.is_media() {
            if source >= clip.in_ && source < clip.out {
                return Some(cursor + (source - clip.in_) / clip.speed);
            }
            // The clip that continues soonest after the cut source time.
            if clip.in_ > source && after.is_none_or(|(start, _)| clip.in_ < start) {
                after = Some((clip.in_, cursor));
            }
        }
        cursor += length;
    }
    after.map(|(_, output)| output)
}

/// The chapters YouTube accepts: the first at 0, each at least [`MIN_CHAPTER_SECONDS`] long
/// (the last one too, up to `duration`).
pub fn youtube_chapters(chapters: &[OutputChapter], duration: f64) -> Vec<(f64, String)> {
    let mut kept: Vec<(f64, String)> = Vec::new();
    for chapter in chapters {
        let time = if kept.is_empty() { 0.0 } else { chapter.output };
        match kept.last() {
            Some((last, _)) if time - last < MIN_CHAPTER_SECONDS => {}
            _ => kept.push((time, chapter.title.clone())),
        }
    }
    while kept.len() > 1 && duration - kept.last().map_or(0.0, |(time, _)| *time) < MIN_CHAPTER_SECONDS {
        kept.pop();
    }
    kept
}

/// `MM:SS`, or `H:MM:SS` for videos of an hour or more.
pub fn timestamp(seconds: f64, duration: f64) -> String {
    let total = seconds.max(0.0).floor() as i64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if duration >= 3600.0 { format!("{hours}:{minutes:02}:{secs:02}") } else { format!("{:02}:{secs:02}", total / 60) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;

    fn clip(in_: f64, out: f64, speed: f64) -> Clip {
        Clip { id: format!("c{in_}"), in_, out, speed, ..Clip::default() }
    }

    fn chapters(list: &[(f64, &str)]) -> Chapters {
        Chapters {
            title: "Demo".into(),
            description: "A demo.".into(),
            chapters: list.iter().map(|&(source, title)| Chapter { source, title: title.into() }).collect(),
        }
    }

    fn outputs(chapters: &Chapters, clips: &[Clip]) -> Vec<(f64, String)> {
        chapters.on_timeline(clips).into_iter().map(|c| (c.output, c.title)).collect()
    }

    #[test]
    fn chapters_follow_cuts_speed_and_inserts() {
        let set = chapters(&[(0.0, "Intro"), (20.0, "Setup"), (45.0, "Build"), (80.0, "Result")]);
        // Uncut: source times.
        assert_eq!(outputs(&set, &[clip(0.0, 100.0, 1.0)]), vec![
            (0.0, "Intro".into()),
            (20.0, "Setup".into()),
            (45.0, "Build".into()),
            (80.0, "Result".into())
        ]);
        // 0–5 trimmed, 30–50 cut, 50–70 at 2×, a 4 s video inserted after it.
        let clips = vec![
            clip(5.0, 30.0, 1.0),
            clip(50.0, 70.0, 2.0),
            Clip { id: "m".into(), in_: 0.0, out: 4.0, media: "media/x.mp4".into(), ..Clip::default() },
            clip(70.0, 100.0, 1.0),
        ];
        assert_eq!(outputs(&set, &clips), vec![
            // The intro's first seconds were trimmed: it starts where the video does.
            (0.0, "Intro".into()),
            (15.0, "Setup".into()),
            // 45 was cut; the build starts where the recording continues (source 50).
            (25.0, "Build".into()),
            // 25 + 20 / 2 + 4 + (80 - 70).
            (49.0, "Result".into()),
        ]);
    }

    #[test]
    fn a_chapter_cut_away_entirely_gives_way_to_the_next() {
        let set = chapters(&[(0.0, "Intro"), (10.0, "Gone"), (20.0, "Kept"), (95.0, "Past the end")]);
        let clips = vec![clip(0.0, 8.0, 1.0), clip(20.0, 90.0, 1.0)];
        assert_eq!(outputs(&set, &clips), vec![(0.0, "Intro".into()), (8.0, "Kept".into())]);
    }

    #[test]
    fn youtube_text_starts_at_zero_and_skips_short_chapters() {
        let set = chapters(&[(2.0, "Intro"), (30.0, "Short"), (35.0, "Main"), (70.0, "Too close to the end")]);
        let clips = vec![clip(0.0, 75.0, 1.0)];
        assert_eq!(
            set.youtube_description(&clips, 75.0),
            // Main is 5 s after Short; the last chapter would end 5 s later.
            "Demo\n\nA demo.\n\n00:00 Intro\n00:30 Short\n"
        );
        assert_eq!(timestamp(3725.4, 4000.0), "1:02:05");
        assert_eq!(timestamp(605.9, 700.0), "10:05");
        assert_eq!(Chapters::default().youtube_description(&clips, 75.0), "");
    }

    #[test]
    fn saves_loads_and_removes() {
        let dir = TempDir::new("chapters");
        let bundle = dir.join("b.omareel");
        std::fs::create_dir_all(&bundle).unwrap();
        assert_eq!(Chapters::load(&bundle).unwrap(), None);
        let set = chapters(&[(12.3456, "Two"), (1.0, "One")]);
        set.save(&bundle).unwrap();
        let loaded = Chapters::load(&bundle).unwrap().unwrap();
        assert_eq!(loaded.chapters, vec![
            Chapter { source: 1.0, title: "One".into() },
            Chapter { source: 12.346, title: "Two".into() }
        ]);
        assert_eq!(loaded.title, "Demo");
        Chapters::default().save(&bundle).unwrap();
        assert!(!std::path::Path::new(&path(&bundle)).exists());
    }
}
