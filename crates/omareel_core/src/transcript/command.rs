//! `omareel transcribe` and the subtitle half of `omareel export`.

use super::backend::{self, Choice, Control};
use super::{transcript_path, Transcript, WordTiming};
use crate::captions;
use crate::compat::qdir;
use crate::compat::qfile;
use crate::project::{Clip, Project};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

/// `omareel transcribe BUNDLE [--backend NAME] [--force]`: prints the transcript's path, or
/// an error; returns the exit status (2 for usage errors).
pub fn transcribe_command(arguments: &[String]) -> i32 {
    let usage = |error: &str| {
        let _ = std::io::stderr().write_all(crate::cli::usage_text(error).as_bytes());
        crate::cli::usage_status(error)
    };
    let mut bundle: Option<String> = None;
    let mut choice = Choice::Auto;
    let mut force = false;
    let mut i = 0;
    while i < arguments.len() {
        match arguments[i].as_str() {
            "--force" => force = true,
            "--backend" => {
                i += 1;
                let Some(name) = arguments.get(i) else { return usage("--backend requires a value") };
                let Some(parsed) = Choice::parse(name) else { return usage(&format!("unknown backend {name}")) };
                choice = parsed;
            }
            other if other.starts_with('-') => return usage(&format!("unknown transcribe option {other}")),
            other if bundle.is_none() => bundle = Some(qdir::absolute_path(other)),
            other => return usage(&format!("unexpected argument {other}")),
        }
        i += 1;
    }
    let Some(bundle) = bundle else { return usage("transcribe requires a bundle") };
    let fail = |message: &str| {
        let _ = writeln!(std::io::stderr(), "omareel: {message}");
        1
    };
    let screen = qdir::file_path(&bundle, "screen.mp4");
    if !Path::new(&screen).is_file() {
        return fail(&format!("{bundle} is not a recording (no screen.mp4)"));
    }
    let path = transcript_path(&bundle);
    if !force && Path::new(&path).is_file() {
        let _ = writeln!(std::io::stderr(), "omareel: {path} exists; --force transcribes again");
        println!("{path}");
        return 0;
    }
    let progress: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(|fraction| {
        let _ = write!(std::io::stderr(), "\rTranscribing {:3.0}%", fraction * 100.0);
    });
    let control = Control { progress: Some(progress), ..Control::default() };
    let result = backend::transcribe_media(&screen, choice, &control);
    let _ = writeln!(std::io::stderr());
    let transcript = match result {
        Ok(transcript) => transcript,
        Err(error) => return fail(&error),
    };
    if let Err(error) = transcript.save(&bundle) {
        return fail(&error);
    }
    let words = transcript.words().count();
    let timing = if transcript.word_timing == WordTiming::Measured { "measured" } else { "estimated" };
    let model = if transcript.model.is_empty() { String::new() } else { format!(", {}", transcript.model) };
    let _ = writeln!(
        std::io::stderr(),
        "{} phrases, {words} words ({}{model}; word times {timing})",
        transcript.segments.len(),
        transcript.backend
    );
    println!("{path}");
    0
}

/// The clips subtitles follow: the project's, or the whole recording when it has none yet.
fn subtitle_clips(bundle: &str) -> Result<Vec<Clip>, String> {
    let project_path = qdir::file_path(bundle, "project.json");
    let clips = if Path::new(&project_path).is_file() {
        let bytes = qfile::read_all(&project_path)?;
        let json = crate::compat::qjson::object_from_json(&bytes).ok_or(format!("{project_path} does not read"))?;
        Project::from_json(&json).clips
    } else {
        Vec::new()
    };
    Ok(if clips.is_empty() { vec![Clip { id: "c1".into(), in_: 0.0, out: 1e12, ..Clip::default() }] } else { clips })
}

/// Writes the captions of `bundle` as SubRip or WebVTT (by `output`'s extension), on the
/// edited timeline. Returns how many captions it wrote.
pub fn export_subtitles(bundle: &str, output: &str) -> Result<usize, String> {
    let transcript = Transcript::load(bundle)?
        .ok_or_else(|| format!("{bundle} has no transcript yet; run `omareel transcribe {bundle}` first"))?;
    let cues = captions::build_cues(&transcript, &subtitle_clips(bundle)?);
    let lower = output.to_lowercase();
    let text = if lower.ends_with(".srt") {
        captions::to_srt(&cues)
    } else if lower.ends_with(".vtt") {
        captions::to_vtt(&cues)
    } else {
        return Err("subtitles must end in .srt or .vtt".into());
    };
    crate::compat::qsavefile::save(output, text.as_bytes())?;
    Ok(cues.len())
}

/// `omareel export BUNDLE -o FILE.srt|FILE.vtt`: prints the file's path; the exit status.
pub fn export_subtitles_command(bundle: &str, output: &str) -> i32 {
    match export_subtitles(bundle, output) {
        Ok(_) => {
            println!("{output}");
            0
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "omareel: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;
    use crate::transcript::{Segment, Word};

    #[test]
    fn subtitles_follow_the_project_clips() {
        let dir = TempDir::new("subtitles");
        let bundle = dir.join("a.omareel");
        std::fs::create_dir_all(&bundle).unwrap();
        let out = dir.join("a.srt");
        assert!(export_subtitles(&bundle, &out).unwrap_err().contains("no transcript"));
        let words = vec![
            Word { start: 1.0, end: 1.5, text: "Hello".into() },
            Word { start: 5.0, end: 5.5, text: "again".into() },
        ];
        Transcript { segments: vec![Segment { start: 1.0, end: 5.5, text: "Hello again".into(), words }], ..Transcript::default() }
            .save(&bundle)
            .unwrap();
        // No project yet: source times.
        assert_eq!(export_subtitles(&bundle, &out).unwrap(), 2);
        let srt = std::fs::read_to_string(&out).unwrap();
        assert!(srt.starts_with("1\n00:00:01,000 --> 00:00:02,100\nHello\n\n2\n00:00:05,000 --> "), "{srt}");
        // A project that cuts 2..4: "again" moves 2 s earlier.
        let mut project = Project::defaults("a", 6.0);
        project.clips = vec![
            Clip { id: "a".into(), in_: 0.0, out: 2.0, ..Clip::default() },
            Clip { id: "b".into(), in_: 4.0, out: 6.0, ..Clip::default() },
        ];
        project.save(&qdir::file_path(&bundle, "project.json")).unwrap();
        let vtt = dir.join("a.VTT");
        export_subtitles(&bundle, &vtt).unwrap();
        let text = std::fs::read_to_string(&vtt).unwrap();
        assert!(text.contains("\n00:00:03.000 --> 00:00:04.000\nagain\n"), "{text}");
        assert!(export_subtitles(&bundle, &dir.join("a.txt")).is_err());
    }
}
