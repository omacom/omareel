//! The speech engines: Omarchy's dictation backends (Setup > Defaults > Dictation), and
//! whisper.cpp's `whisper-cli`, run as commands on pieces of the recording's sound.
//!
//! What each can do with a file (checked against voxtype 1.1.1, whisper-cpp 1.9.5, Hex 3.4.0
//! and Superwhisper 1.1.2 for Linux):
//! - voxtype: `voxtype transcribe FILE` runs its configured engine and model without the
//!   daemon and prints text only.
//! - Hex: `hex transcribe FILE` prints text only; it needs Hex running and Labs > Transcribe
//!   files on, and exits 4 when it heard no speech.
//! - Superwhisper: `superwhisper transcribe FILE` goes through its service; text only.
//! - whisper-cli: times every token (`-ojf`), so words are measured. It takes a ggml model
//!   file, which voxtype's Whisper models are.
//!
//! The text-only engines get one phrase at a time (found by `vad`), which times the phrases;
//! words are spread over the phrase's speech ([`super::estimate_words`]).

use super::audio::{write_wav, Audio};
use super::vad::{self, VadSettings};
use super::{estimate_words, split_words, Segment, Transcript, Word, WordTiming};
use super::job::{self, Stopped};
pub use super::job::Control;
use crate::compat::qjson;
use crate::compat::qstandardpaths;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Voxtype,
    Hex,
    Superwhisper,
    WhisperCpp,
}

impl BackendKind {
    pub const ALL: [BackendKind; 4] =
        [BackendKind::Voxtype, BackendKind::Hex, BackendKind::Superwhisper, BackendKind::WhisperCpp];

    pub fn name(self) -> &'static str {
        match self {
            BackendKind::Voxtype => "voxtype",
            BackendKind::Hex => "hex",
            BackendKind::Superwhisper => "superwhisper",
            BackendKind::WhisperCpp => "whisper-cpp",
        }
    }

    pub fn parse(name: &str) -> Option<BackendKind> {
        BackendKind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    fn program(self) -> &'static str {
        match self {
            BackendKind::Voxtype => "voxtype",
            BackendKind::Hex => "hex",
            BackendKind::Superwhisper => "superwhisper",
            BackendKind::WhisperCpp => "whisper-cli",
        }
    }

    /// Whether the engine times words itself.
    pub fn timestamps(self) -> bool {
        self == BackendKind::WhisperCpp
    }
}

/// What `--backend` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Auto,
    Only(BackendKind),
}

impl Choice {
    pub fn parse(name: &str) -> Option<Choice> {
        if name == "auto" { Some(Choice::Auto) } else { BackendKind::parse(name).map(Choice::Only) }
    }
}

/// An engine ready to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub kind: BackendKind,
    pub program: String,
    /// whisper-cli's model file.
    pub model_path: String,
    /// The model's name for transcript.json.
    pub model: String,
    /// A language to ask for ("auto" or a code), or empty for the engine's own setting.
    pub language: String,
}

pub const UNAVAILABLE: &str = "No speech-to-text engine. Choose one in Omarchy's Setup > Defaults > Dictation \
(or run `omarchy install dictation voxtype`).";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn config_home() -> PathBuf {
    let value = env("XDG_CONFIG_HOME");
    if value.is_empty() { PathBuf::from(env("HOME")).join(".config") } else { PathBuf::from(value) }
}

fn data_home() -> PathBuf {
    let value = env("XDG_DATA_HOME");
    if value.is_empty() { PathBuf::from(env("HOME")).join(".local/share") } else { PathBuf::from(value) }
}

/// The dictation backend chosen in Omarchy (`omarchy-default-dictation` reads the same file).
pub fn omarchy_dictation() -> Option<String> {
    let text = std::fs::read_to_string(config_home().join("omarchy/defaults/dictation")).ok()?;
    let name = text.lines().next()?.trim().to_owned();
    if name.is_empty() { None } else { Some(name) }
}

/// The few voxtype settings that matter here, from `~/.config/voxtype/config.toml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoxtypeConfig {
    /// Top-level `engine` (empty means Whisper).
    pub engine: String,
    /// `[whisper] model`: a name like `base.en` or a path to a model file.
    pub whisper_model: String,
    /// `[whisper] language`.
    pub language: String,
}

/// Reads `key = "value"` lines by section; enough for voxtype's flat TOML.
pub fn parse_voxtype_config(text: &str) -> VoxtypeConfig {
    let mut config = VoxtypeConfig::default();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').trim().to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches('"').to_owned();
        match (section.as_str(), key.trim()) {
            ("", "engine") => config.engine = value,
            ("whisper", "model") => config.whisper_model = value,
            ("whisper", "language") => config.language = value,
            _ => {}
        }
    }
    config
}

pub fn voxtype_config() -> VoxtypeConfig {
    let mut config = std::fs::read_to_string(config_home().join("voxtype/config.toml"))
        .map(|text| parse_voxtype_config(&text))
        .unwrap_or_default();
    if config.whisper_model.is_empty() {
        config.whisper_model = "base.en".into();
    }
    config
}

fn voxtype_models_dir() -> PathBuf {
    data_home().join("voxtype/models")
}

/// A Whisper model file for whisper-cli: `OMAREEL_WHISPER_MODEL`, else the one voxtype is set
/// to, else the best one voxtype or whisper.cpp downloaded. With the name it goes by.
pub fn whisper_model() -> Option<(String, String)> {
    let label = |path: &Path| {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        stem.strip_prefix("ggml-").unwrap_or(&stem).to_owned()
    };
    let explicit = env("OMAREEL_WHISPER_MODEL");
    if !explicit.is_empty() {
        let path = PathBuf::from(&explicit);
        return path.is_file().then(|| (explicit.clone(), label(&path)));
    }
    let voxtype = voxtype_config();
    if voxtype.engine.is_empty() || voxtype.engine == "whisper" {
        let configured = if voxtype.whisper_model.contains('/') {
            PathBuf::from(&voxtype.whisper_model)
        } else {
            voxtype_models_dir().join(format!("ggml-{}.bin", voxtype.whisper_model))
        };
        if configured.is_file() {
            return Some((configured.to_string_lossy().into_owned(), label(&configured)));
        }
    }
    const PREFERENCE: &[&str] =
        &["large-v3-turbo", "large-v3", "medium.en", "medium", "small.en", "small", "base.en", "base", "tiny.en", "tiny"];
    let directories = [voxtype_models_dir(), data_home().join("whisper.cpp/models"), data_home().join("whisper-cpp/models")];
    for name in PREFERENCE {
        for directory in &directories {
            let path = directory.join(format!("ggml-{name}.bin"));
            if path.is_file() {
                return Some((path.to_string_lossy().into_owned(), (*name).to_owned()));
            }
        }
    }
    None
}

fn installed(program: &str) -> Option<String> {
    let path = qstandardpaths::find_executable(program);
    if path.is_empty() { None } else { Some(path) }
}

/// The engine for `kind` if it is installed (and, for whisper-cli, has a model).
pub fn engine(kind: BackendKind) -> Option<Engine> {
    let program = installed(kind.program())?;
    let mut engine = Engine { kind, program, model_path: String::new(), model: String::new(), language: String::new() };
    match kind {
        BackendKind::WhisperCpp => {
            let (path, model) = whisper_model()?;
            let voxtype = voxtype_config();
            engine.language = if model.ends_with(".en") {
                "en".into()
            } else if !voxtype.language.is_empty() && !voxtype.language.contains(',') {
                voxtype.language
            } else {
                "auto".into()
            };
            engine.model_path = path;
            engine.model = model;
        }
        BackendKind::Voxtype => {
            let voxtype = voxtype_config();
            if voxtype.engine.is_empty() || voxtype.engine == "whisper" {
                engine.model = voxtype.whisper_model;
                if voxtype.language != "auto" {
                    engine.language = voxtype.language;
                }
            } else {
                engine.model = voxtype.engine;
            }
        }
        BackendKind::Hex | BackendKind::Superwhisper => {}
    }
    Some(engine)
}

/// The engines to try, best first. `auto` follows Omarchy's dictation choice. When that is
/// voxtype on a Whisper model and whisper-cli is installed, voxtype's own model runs through
/// whisper-cli first: the same engine and model, with measured word times. Then the chosen
/// backend itself, then whisper-cli with any model, then voxtype if it is installed but not
/// chosen.
pub fn plan(choice: Choice) -> Vec<Engine> {
    if let Choice::Only(kind) = choice {
        return engine(kind).into_iter().collect();
    }
    let default = omarchy_dictation().and_then(|name| BackendKind::parse(&name));
    let mut order: Vec<BackendKind> = Vec::new();
    if default == Some(BackendKind::Voxtype) {
        let voxtype = voxtype_config();
        if voxtype.engine.is_empty() || voxtype.engine == "whisper" {
            order.push(BackendKind::WhisperCpp);
        }
    }
    order.extend(default);
    order.extend([BackendKind::WhisperCpp, BackendKind::Voxtype]);
    let mut engines: Vec<Engine> = Vec::new();
    for kind in order {
        if engines.iter().any(|engine| engine.kind == kind) {
            continue;
        }
        if let Some(engine) = engine(kind) {
            engines.push(engine);
        }
    }
    engines
}

/// Why a piece of work did not give a result.
enum Failure {
    /// Stopped for a recording or an export; run it again once that is over.
    Held,
    Cancelled,
    Error(String),
}

impl From<String> for Failure {
    fn from(error: String) -> Failure {
        Failure::Error(error)
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs an engine to its end in a process group of its own (see `job`), killing the group
/// after `timeout`, once cancelled, or when a recording or an export starts.
fn run(program: &str, args: &[String], timeout: Duration, control: &Control) -> Result<Output, Failure> {
    let mut command = Command::new(program);
    command.args(args);
    match job::run(command, Some(timeout), control) {
        Ok(output) => Ok(Output {
            code: output.code,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: output.stderr,
        }),
        Err(Stopped::Held) => Err(Failure::Held),
        Err(Stopped::Cancelled) => Err(Failure::Cancelled),
        Err(Stopped::TimedOut) => Err(Failure::Error(format!("{program} took too long"))),
        Err(Stopped::Failed(error)) => Err(Failure::Error(error)),
    }
}

/// The last line that says something, for an error message.
fn last_line(text: &str) -> String {
    let line = text.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    // Logs colour their levels.
    let mut plain = String::new();
    let mut escape = false;
    for c in line.chars() {
        match (escape, c) {
            (_, '\u{1b}') => escape = true,
            (true, 'm') => escape = false,
            (true, _) => {}
            (false, c) => plain.push(c),
        }
    }
    plain.chars().take(200).collect()
}

/// Bracketed non-speech tags engines print for silence and noise (`[BLANK_AUDIO]`,
/// `(music)`), and the white space they leave.
pub fn clean_text(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    split_words(&out).join(" ")
}

/// The transcript in what `voxtype transcribe` prints: its own lines about the file come
/// first, then a blank line and the text.
pub fn parse_voxtype_output(stdout: &str) -> String {
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !(line.starts_with("Loading audio file:")
                || line.starts_with("Audio format:")
                || line.starts_with("Processing ")
                || line.contains('\u{1b}')
                || line.contains(" INFO ")
                || line.contains(" WARN "))
        })
        .collect();
    clean_text(&lines.join(" "))
}

/// Words with times from whisper-cli's `-ojf` JSON: tokens that begin with a space begin a
/// word, the others (word pieces, punctuation) join the word before. Special tokens (`[_BEG_]`,
/// `[_TT_…]`) and bracketed tags are dropped. Times are seconds from the file's start.
pub fn parse_whisper_json(json: &[u8]) -> Result<(Vec<Segment>, String), String> {
    let object = qjson::object_from_json(json).ok_or("whisper-cli wrote no JSON")?;
    let language = object.value("result").to_object().value("language").to_string_or("");
    let mut segments = Vec::new();
    for value in object.value("transcription").to_array() {
        let o = value.to_object();
        let offsets = o.value("offsets").to_object();
        let mut segment = Segment {
            start: offsets.value("from").to_double(0.0) / 1000.0,
            end: offsets.value("to").to_double(0.0) / 1000.0,
            text: clean_text(&o.value("text").to_string_or("")),
            words: Vec::new(),
        };
        let mut words: Vec<Word> = Vec::new();
        let mut bracket = 0;
        for token in o.value("tokens").to_array() {
            let t = token.to_object();
            let text = t.value("text").to_string_or("");
            if text.starts_with("[_") {
                continue;
            }
            let offsets = t.value("offsets").to_object();
            let (from, to) = (offsets.value("from").to_double(0.0) / 1000.0, offsets.value("to").to_double(0.0) / 1000.0);
            let trimmed = text.trim();
            if trimmed.starts_with('[') || trimmed.starts_with('(') {
                bracket += 1;
            }
            let inside = bracket > 0;
            if trimmed.ends_with(']') || trimmed.ends_with(')') {
                bracket = (bracket - 1).max(0);
            }
            if inside || trimmed.is_empty() {
                continue;
            }
            let joins = !text.starts_with(' ') || trimmed.chars().all(|c| c.is_ascii_punctuation());
            match words.last_mut() {
                Some(word) if joins => {
                    word.text.push_str(trimmed);
                    // Punctuation is timed at the next pause; a piece of the word extends it.
                    if trimmed.chars().any(char::is_alphanumeric) {
                        word.end = word.end.max(to);
                    }
                }
                _ => words.push(Word { start: from, end: to.max(from), text: trimmed.to_owned() }),
            }
        }
        if segment.text.is_empty() && words.is_empty() {
            continue;
        }
        if segment.text.is_empty() {
            segment.text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        }
        segment.words = words;
        segments.push(segment);
    }
    Ok((segments, language))
}

/// What an engine made of one piece: text, or segments timed from the piece's start.
enum Heard {
    Text(String),
    Timed(Vec<Segment>, String),
}

fn transcribe_text(engine: &Engine, wav: &Path, control: &Control) -> Result<String, Failure> {
    let file = wav.to_string_lossy().into_owned();
    let timeout = Duration::from_secs(300);
    match engine.kind {
        BackendKind::Voxtype => {
            let output = run(&engine.program, &["-q".into(), "transcribe".into(), file], timeout, control)?;
            if output.code != 0 {
                return Err(Failure::Error(format!("voxtype: {}", last_line(&(output.stderr + &output.stdout)))));
            }
            Ok(parse_voxtype_output(&output.stdout))
        }
        BackendKind::Hex => {
            let output = run(&engine.program, &["transcribe".into(), file], timeout, control)?;
            match output.code {
                0 => Ok(clean_text(&output.stdout)),
                4 => Ok(String::new()),
                3 => Err(Failure::Error("Hex isn't running".into())),
                _ => Err(Failure::Error(format!("hex: {}", last_line(&output.stderr)))),
            }
        }
        BackendKind::Superwhisper => {
            let output = run(&engine.program, &["transcribe".into(), file], timeout, control)?;
            if output.code != 0 {
                return Err(Failure::Error(format!("superwhisper: {}", last_line(&(output.stderr + &output.stdout)))));
            }
            Ok(clean_text(&output.stdout))
        }
        BackendKind::WhisperCpp => unreachable!(),
    }
}

/// whisper-cli over several pieces at once (the model loads once); one result per piece.
fn transcribe_whisper(engine: &Engine, wavs: &[PathBuf], seconds: f64, control: &Control) -> Result<Vec<Heard>, Failure> {
    let threads = std::thread::available_parallelism().map_or(4, |n| (n.get() / 2).clamp(1, 8));
    let mut args: Vec<String> = vec!["-m".into(), engine.model_path.clone(), "-ojf".into(), "-np".into()];
    args.extend(["-t".into(), threads.to_string()]);
    if !engine.language.is_empty() {
        args.extend(["-l".into(), engine.language.clone()]);
    }
    for wav in wavs {
        // The JSON goes next to each input, as `<input>.json`.
        let _ = std::fs::remove_file(json_path(wav));
        args.push(wav.to_string_lossy().into_owned());
    }
    let output = run(&engine.program, &args, Duration::from_secs_f64(120.0 + seconds * 10.0), control)?;
    if output.code != 0 {
        return Err(Failure::Error(format!("whisper-cli: {}", last_line(&output.stderr))));
    }
    let heard: Result<Vec<Heard>, String> = wavs
        .iter()
        .map(|wav| {
            let bytes = std::fs::read(json_path(wav)).map_err(|_| format!("whisper-cli: {}", last_line(&output.stderr)))?;
            let (segments, language) = parse_whisper_json(&bytes)?;
            Ok(Heard::Timed(segments, language))
        })
        .collect();
    Ok(heard?)
}

fn json_path(wav: &Path) -> PathBuf {
    let mut name = wav.as_os_str().to_owned();
    name.push(".json");
    PathBuf::from(name)
}

/// A scratch directory removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, String> {
        let cache = env("XDG_CACHE_HOME");
        let cache = if cache.is_empty() { PathBuf::from(env("HOME")).join(".cache") } else { PathBuf::from(cache) };
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let path = cache.join(format!("omareel/transcribe-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Scratch(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One piece of speech to transcribe: where it is and the speech spans in it.
struct Piece {
    start: f64,
    end: f64,
    speech: Vec<(f64, f64)>,
    wav: PathBuf,
}

/// Transcribes `audio` with the first engine of `engines` that works, piece by piece. With no
/// speech in the audio the transcript is empty (and no engine runs). The error, when every
/// engine fails, names what each said.
pub fn transcribe_audio(audio: &Audio, engines: &[Engine], control: &Control) -> Result<Transcript, String> {
    if engines.is_empty() {
        return Err(UNAVAILABLE.into());
    }
    let levels = vad::frame_levels(&audio.samples);
    let regions = vad::speech_regions_from_levels(&levels, audio.duration(), &VadSettings::default());
    let mut failures: Vec<String> = Vec::new();
    for engine in engines {
        if regions.is_empty() {
            return Ok(empty_transcript(engine));
        }
        // Whisper reads 30 s at a time and keeps its context within a piece; text-only engines
        // get phrases, which time the words better.
        let (join_gap, max_length) = if engine.kind.timestamps() { (1.0, 28.0) } else { (0.6, 15.0) };
        let chunks = vad::transcription_chunks(&regions, &levels, join_gap, max_length);
        match transcribe_pieces(audio, engine, &chunks, control) {
            Ok(transcript) => return Ok(transcript),
            Err(error) if control.cancelled() => return Err(error),
            Err(error) => failures.push(format!("{}: {error}", engine.kind.name())),
        }
    }
    Err(failures.join("; "))
}

fn empty_transcript(engine: &Engine) -> Transcript {
    Transcript {
        backend: engine.kind.name().into(),
        model: engine.model.clone(),
        language: engine.language.clone(),
        word_timing: if engine.kind.timestamps() { WordTiming::Measured } else { WordTiming::Estimated },
        segments: Vec::new(),
    }
}

fn transcribe_pieces(
    audio: &Audio,
    engine: &Engine,
    chunks: &[(f64, f64, Vec<(f64, f64)>)],
    control: &Control,
) -> Result<Transcript, String> {
    let scratch = Scratch::new()?;
    let pieces: Vec<Piece> = chunks
        .iter()
        .enumerate()
        .map(|(i, (start, end, speech))| Piece {
            start: *start,
            end: *end,
            speech: speech.clone(),
            wav: scratch.0.join(format!("piece-{i:05}.wav")),
        })
        .collect();
    let mut transcript = empty_transcript(engine);
    let total: f64 = pieces.iter().map(|p| p.end - p.start).sum::<f64>().max(1e-9);
    let mut done = 0.0;
    control.report(0.0);
    // whisper-cli takes several pieces per run; the others one.
    let batch = if engine.kind.timestamps() { 8 } else { 1 };
    for group in pieces.chunks(batch) {
        let seconds: f64 = group.iter().map(|p| p.end - p.start).sum();
        // A recording or an export stops the engine; the batch runs again once it is over.
        let heard: Vec<Heard> = loop {
            if !control.proceed() {
                return Err("cancelled".into());
            }
            for piece in group {
                write_wav(&piece.wav, audio.slice(piece.start, piece.end))?;
            }
            let heard = if engine.kind.timestamps() {
                let wavs: Vec<PathBuf> = group.iter().map(|p| p.wav.clone()).collect();
                transcribe_whisper(engine, &wavs, seconds, control)
            } else {
                transcribe_text(engine, &group[0].wav, control).map(|text| vec![Heard::Text(text)])
            };
            match heard {
                Ok(heard) => break heard,
                Err(Failure::Held) => continue,
                Err(Failure::Cancelled) => return Err("cancelled".into()),
                Err(Failure::Error(error)) => return Err(error),
            }
        };
        for (piece, heard) in group.iter().zip(heard) {
            match heard {
                Heard::Text(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    transcript.segments.push(Segment {
                        start: piece.start,
                        end: piece.end,
                        words: estimate_words(&text, &piece.speech),
                        text,
                    });
                }
                Heard::Timed(segments, language) => {
                    if transcript.language.is_empty() || transcript.language == "auto" {
                        transcript.language = language;
                    }
                    let place = |t: f64| (piece.start + t).clamp(piece.start, piece.end);
                    for segment in segments {
                        let words: Vec<Word> = segment
                            .words
                            .into_iter()
                            .map(|w| Word { start: place(w.start), end: place(w.end), text: w.text })
                            .collect();
                        transcript.segments.push(Segment {
                            start: place(segment.start),
                            end: place(segment.end),
                            text: segment.text,
                            words,
                        });
                    }
                }
            }
            let _ = std::fs::remove_file(&piece.wav);
            let _ = std::fs::remove_file(json_path(&piece.wav));
        }
        done += seconds;
        control.report(done / total);
    }
    Ok(transcript)
}

/// Whether some engine could run (without running it).
pub fn available(choice: Choice) -> bool {
    !plan(choice).is_empty()
}

/// Reads the recording's sound and transcribes it (see [`transcribe_audio`]).
pub fn transcribe_media(media: &str, choice: Choice, control: &Control) -> Result<Transcript, String> {
    let engines = plan(choice);
    if engines.is_empty() {
        return Err(match choice {
            Choice::Auto => UNAVAILABLE.into(),
            Choice::Only(kind) => format!("{} is not installed{}", kind.program(), match kind {
                BackendKind::WhisperCpp if installed("whisper-cli").is_some() => " with a model (run `voxtype setup --download`, or set OMAREEL_WHISPER_MODEL)",
                _ => "",
            }),
        });
    }
    let audio = super::audio::extract_with(media, control)?;
    transcribe_audio(&audio, &engines, control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voxtype_config_reads_the_whisper_section() {
        let config = parse_voxtype_config(
            "state_file = \"auto\"\n[whisper]\n# comment\nmodel = \"small.en\" # trailing\nlanguage = \"en\"\n[output]\nmodel = \"x\"\n",
        );
        assert_eq!(config, VoxtypeConfig { engine: String::new(), whisper_model: "small.en".into(), language: "en".into() });
        assert_eq!(parse_voxtype_config("engine = \"parakeet\"\n").engine, "parakeet");
    }

    #[test]
    fn voxtype_output_keeps_only_the_text() {
        let stdout = "Loading audio file: \"say16.wav\"\nAudio format: 16000 Hz, 1 channel(s), Int\n\
                      Processing 100765 samples (6.30s)...\n\nHello there! Umm, this is a test. [BLANK_AUDIO]\n";
        assert_eq!(parse_voxtype_output(stdout), "Hello there! Umm, this is a test.");
        assert_eq!(parse_voxtype_output("Processing 1 samples\n\n"), "");
    }

    #[test]
    fn clean_text_drops_tags() {
        assert_eq!(clean_text(" (music)  Hello  [BLANK_AUDIO] world "), "Hello world");
    }

    #[test]
    fn whisper_tokens_become_words() {
        let json = br#"{"result":{"language":"en"},"transcription":[{"offsets":{"from":0,"to":3000},
            "text":" Hello, transcription works.","tokens":[
            {"text":"[_BEG_]","offsets":{"from":0,"to":0}},
            {"text":" Hello","offsets":{"from":50,"to":300}},
            {"text":",","offsets":{"from":300,"to":340}},
            {"text":" trans","offsets":{"from":400,"to":700}},
            {"text":"cription","offsets":{"from":700,"to":1100}},
            {"text":" works","offsets":{"from":1200,"to":1600}},
            {"text":".","offsets":{"from":3000,"to":3000}},
            {"text":"[_TT_150]","offsets":{"from":3000,"to":3000}}]},
            {"offsets":{"from":3000,"to":4000},"text":" [BLANK_AUDIO]","tokens":[
            {"text":" [","offsets":{"from":3000,"to":3100}},{"text":"BLANK_AUDIO","offsets":{"from":3100,"to":3500}},
            {"text":"]","offsets":{"from":3500,"to":4000}}]}]}"#;
        let (segments, language) = parse_whisper_json(json).unwrap();
        assert_eq!(language, "en");
        assert_eq!(segments.len(), 1);
        let words: Vec<(f64, f64, &str)> = segments[0].words.iter().map(|w| (w.start, w.end, w.text.as_str())).collect();
        assert_eq!(words, vec![(0.05, 0.3, "Hello,"), (0.4, 1.1, "transcription"), (1.2, 1.6, "works.")]);
        assert_eq!(segments[0].text, "Hello, transcription works.");
    }

    #[test]
    fn choices_parse() {
        assert_eq!(Choice::parse("auto"), Some(Choice::Auto));
        assert_eq!(Choice::parse("whisper-cpp"), Some(Choice::Only(BackendKind::WhisperCpp)));
        assert_eq!(Choice::parse("whisper"), None);
    }

    /// No engine, no transcription; silence needs none.
    #[test]
    fn silence_transcribes_to_nothing_without_running_anything() {
        let engine = Engine {
            kind: BackendKind::Voxtype,
            program: "/nonexistent/voxtype".into(),
            model_path: String::new(),
            model: "base.en".into(),
            language: "en".into(),
        };
        let audio = Audio { samples: vec![0; 32_000] };
        let transcript = transcribe_audio(&audio, &[engine.clone()], &Control::default()).unwrap();
        assert!(transcript.segments.is_empty());
        assert_eq!(transcript.backend, "voxtype");
        assert_eq!(transcribe_audio(&audio, &[], &Control::default()).unwrap_err(), UNAVAILABLE);
        // With speech, a missing program is an error that names the engine.
        let audio = Audio { samples: crate::transcript::vad::tests::bursts(2.0, &[(0.5, 1.5)], 0.3) };
        let error = transcribe_audio(&audio, &[engine], &Control::default()).unwrap_err();
        assert!(error.starts_with("voxtype: could not run"), "{error}");
    }

    /// A fake voxtype in a scratch directory running `body` (a shell script).
    fn fake_voxtype(dir: &Path, body: &str) -> Engine {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let program = dir.join("voxtype");
        std::fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        Engine {
            kind: BackendKind::Voxtype,
            program: program.to_string_lossy().into_owned(),
            model_path: String::new(),
            model: "base.en".into(),
            language: String::new(),
        }
    }

    fn speech_audio() -> Audio {
        Audio { samples: crate::transcript::vad::tests::bursts(2.0, &[(0.5, 1.5)], 0.3) }
    }

    /// An engine that leaves a child holding its output is stopped at once when cancelled (the
    /// editor closing), child and all.
    #[test]
    fn cancelling_kills_an_engine_and_what_it_started() {
        let dir = std::env::temp_dir().join(format!("omareel-fake-engine-{}", std::process::id()));
        let pidfile = dir.join("sleeper");
        let engine = fake_voxtype(&dir, &format!("sleep 500 &\necho $! > {}\nexec sleep 400", pidfile.display()));
        let control = Control::default();
        let stopper = control.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            stopper.stop();
        });
        let started = std::time::Instant::now();
        assert!(transcribe_audio(&speech_audio(), &[engine], &control).is_err());
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        let sleeper: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        // SAFETY: signal 0 only checks that the process exists.
        assert_ne!(unsafe { libc::kill(sleeper, 0) }, 0, "the engine's child outlived it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A recording starting while an engine runs stops it; it runs again once the recording is
    /// over, and the transcript comes out whole.
    #[test]
    fn a_recording_stops_the_engine_and_it_resumes_after() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("omareel-held-engine-{}", std::process::id()));
        let runs = dir.join("runs");
        let engine = fake_voxtype(&dir, &format!("echo run >> {}\nsleep 1.5\necho\necho Hello world.", runs.display()));
        let recording = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&recording);
        let control = Control { hold: Some(Arc::new(move || flag.load(Ordering::Relaxed))), ..Control::default() };
        let toggler = Arc::clone(&recording);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            toggler.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1500));
            toggler.store(false, Ordering::Relaxed);
        });
        let started = std::time::Instant::now();
        let transcript = transcribe_audio(&speech_audio(), &[engine], &control).unwrap();
        let ran = std::fs::read_to_string(&runs).unwrap().lines().count();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(transcript.text(), "Hello world.");
        assert_eq!(ran, 2, "the first run should have been stopped");
        // Stopped within half a second of the recording starting, not after its 1.5 s.
        assert!(started.elapsed() < Duration::from_millis(1800 + 1500 + 1000), "{:?}", started.elapsed());
    }
}
