//! Cleaning up the recording's sound (`audio.cleanup` in project.json): noise suppression and
//! loudness normalisation, as ffmpeg filters.
//!
//! Noise suppression is RNNoise, through ffmpeg's `arnndn`, with GregorR's "somnolent-hogwash"
//! model (speech in a recording environment: fans, air conditioning, computers), built into
//! the binary and written to a directory private to the user ([`default_model_directory`]) when
//! a filter needs it. On synthetic noisy speech it lowered the noise floor by 11-20 dB where
//! `afftdn` managed 0-2 dB at any setting (docs/USAGE.md has the numbers); `anlmdn` corrupts the
//! heap in ffmpeg 9.0. It runs over the recording's whole sound once, before the clips cut it.
//!
//! Loudness normalisation is two-pass: [`loudness_measure_filter`] measures the program (the
//! clips' joined sound, before the volume and the click sound), and [`loudness_apply_filter`]
//! applies the gain that brings it to the target, linearly when the gain keeps the true peak
//! under [`TRUE_PEAK`], otherwise with loudnorm's limiter. The volume trims after it.
//!
//! Export puts both into its audio graph; the editor renders the cleaned sound of the recording
//! and of inserted videos to cache files ([`CleanAudioJob`]) that the preview plays in place of
//! their own, measured the way export measures.

use crate::compat::qdir::{self, FileInfo};
use crate::compat::qjson;
use crate::compat::qnumber::number_f64;
use crate::compat::qprocess::{Process, ProcessState};
use crate::compat::qstandardpaths::{self, Location};
use crate::compat::{qfile, qsavefile, qstring};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// RNNoise's "somnolent-hogwash" model (rnnoise-nu format, 297 KB), from
/// <https://github.com/GregorR/rnnoise-models> (LICENSES/rnnoise-models.txt).
pub const MODEL: &[u8] = include_bytes!("../../../assets/audio/rnnoise-sh.rnnn");

/// The true peak loudnorm limits to, in dBTP: half a decibel under the -1.5 dBTP delivery
/// target, for the AAC encoder's overshoot (denoised speech came out at -1.2 dBTP when limited at
/// -1.5).
pub const TRUE_PEAK: f64 = -2.0;

/// The loudness range loudnorm aims for unless the sound already has a wider one, which it then
/// keeps (so the gain stays linear).
const LOUDNESS_RANGE: f64 = 11.0;

/// Integrated loudness below this (or none at all) is silence: nothing to normalise.
const SILENCE_LUFS: f64 = -70.0;

/// `value` escaped for a filter option inside a filter graph: first for the option (`\`, `'`,
/// `:`), then for the graph (`\`, `'`, `[`, `]`, `,`, `;`).
pub fn escape_filter_value(value: &str) -> String {
    let mut option = String::new();
    for c in value.chars() {
        if matches!(c, '\\' | '\'' | ':') {
            option.push('\\');
        }
        option.push(c);
    }
    let mut graph = String::new();
    for c in option.chars() {
        if matches!(c, '\\' | '\'' | '[' | ']' | ',' | ';') {
            graph.push('\\');
        }
        graph.push(c);
    }
    graph
}

/// Writes [`MODEL`] into `directory`, a private one ([`private_directory`]), and returns its
/// path. A file already there is used only when it holds exactly the model (its name carries
/// the model's hash); anything else in its place is replaced.
pub fn model_file(directory: &str) -> Result<String, String> {
    let failed = |error: String| format!("Could not prepare noise reduction: {error}");
    private_directory(directory).map_err(failed)?;
    let name = format!("omareel-rnnoise-{}.rnnn", &qdir::sha256_hex(MODEL)[..12]);
    let path = qdir::file_path(directory, &name);
    if FileInfo::new(&path).size() == MODEL.len() as i64 && qfile::read_all(&path).is_ok_and(|bytes| bytes == MODEL) {
        return Ok(path);
    }
    qsavefile::save(&path, MODEL).map_err(|error| failed(error.to_string()))?;
    Ok(path)
}

/// Makes sure `directory` exists, belongs to this user and only this user can use it (0700):
/// created that way, or an existing one of ours tightened to it. Another user's directory, or
/// one others may write to that cannot be tightened, is refused.
pub fn private_directory(directory: &str) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let path = std::path::Path::new(directory);
    if !path.is_dir() {
        std::fs::create_dir_all(path).map_err(|error| format!("{directory}: {error}"))?;
    }
    // Not through a symlink someone else could point elsewhere.
    let metadata = std::fs::symlink_metadata(path).map_err(|error| format!("{directory}: {error}"))?;
    // SAFETY: geteuid has no failure mode.
    let me = unsafe { libc::geteuid() };
    if !metadata.is_dir() || metadata.uid() != me {
        return Err(format!("{directory} is not a directory of this user's"));
    }
    if metadata.mode() & 0o077 != 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("{directory}: {error}"))?;
    }
    Ok(())
}

/// Where [`model_file`] puts the model by default: the runtime directory (private to the user
/// by the XDG specification) when it is one of the user's own, otherwise `omareel` in the
/// user's cache directory (~/.cache/omareel), made private. Never a shared temporary directory.
pub fn default_model_directory() -> String {
    use std::os::unix::fs::MetadataExt;
    if let Some(runtime) = qdir::env_var("XDG_RUNTIME_DIR").filter(|runtime| !runtime.is_empty()) {
        // SAFETY: geteuid has no failure mode.
        let me = unsafe { libc::geteuid() };
        let private = std::fs::symlink_metadata(&runtime)
            .is_ok_and(|metadata| metadata.is_dir() && metadata.uid() == me && metadata.mode() & 0o022 == 0);
        if private {
            return runtime;
        }
    }
    qdir::file_path(&qstandardpaths::writable_location(Location::GenericCache), "omareel")
}

/// The noise suppression filter for 48 kHz audio: RNNoise with the model at `model_path`, its
/// result mixed with the original by `strength` (1: all denoised). arnndn's output runs one
/// 10 ms frame (480 samples) late, wet and dry alike, and it does not flush that frame at the
/// end: 480 samples of padding push the last real ones out, and the trim takes the delay back,
/// so the sound keeps its length and stays on the picture. It is for a whole recording, once
/// ([`crate::export::plan::audio_inputs`]): a denoiser per clip would lose 10 ms at every cut.
pub fn denoise_filter(model_path: &str, strength: f64) -> String {
    format!(
        "apad=pad_len=480,arnndn=m={}:mix={},atrim=start_sample=480,asetpts=PTS-STARTPTS",
        escape_filter_value(model_path),
        number_f64(strength, 'g', 6)
    )
}

/// What loudnorm's first pass measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoudnessMeasurement {
    /// Integrated loudness, LUFS.
    pub integrated: f64,
    /// True peak, dBTP.
    pub true_peak: f64,
    /// Loudness range, LU.
    pub range: f64,
    /// The gating threshold, LUFS.
    pub threshold: f64,
    /// loudnorm's `target_offset`, LU.
    pub offset: f64,
}

/// The first pass: loudnorm measuring for `target` LUFS and printing what it found as JSON on
/// its log (ffmpeg's standard error at `-v info`), which [`parse_loudnorm`] reads.
pub fn loudness_measure_filter(target: f64) -> String {
    format!(
        "loudnorm=I={}:TP={}:LRA={}:print_format=json",
        number_f64(target, 'g', 6),
        number_f64(TRUE_PEAK, 'g', 6),
        number_f64(LOUDNESS_RANGE, 'g', 6)
    )
}

/// The measurement in loudnorm's JSON report, the last one in `log`; `None` without one.
pub fn parse_loudnorm(log: &str) -> Option<LoudnessMeasurement> {
    let start = log.rfind("\"input_i\"").and_then(|at| log[..at].rfind('{'))?;
    let end = start + log[start..].find('}')? + 1;
    let report = qjson::object_from_json(log[start..end].as_bytes())?;
    // The numbers come as text, "-inf" for silence.
    let number = |key: &str| report.value(key).as_str().and_then(|text| text.trim().parse::<f64>().ok());
    Some(LoudnessMeasurement {
        integrated: number("input_i")?,
        true_peak: number("input_tp")?,
        range: number("input_lra")?,
        threshold: number("input_thresh")?,
        offset: number("target_offset").unwrap_or(0.0),
    })
}

/// The second pass: loudnorm with the first pass's measurement, back at 48 kHz (loudnorm works
/// at 192 kHz). When the gain alone would pass the true peak loudnorm turns dynamic, and then
/// stamps its output frames with times that run slow (0.02 s for each 0.1 s frame): an encoder
/// downstream of a mix makes them one-sample packets, misplacing the sound. The timestamps are
/// counted again from the samples. The format is pinned to the clips' (48 kHz float stereo):
/// otherwise the mix with the mono click sound after it negotiates the whole export down to
/// mono. `None` for silence, which is left alone.
pub fn loudness_apply_filter(target: f64, measured: &LoudnessMeasurement) -> Option<String> {
    if !measured.integrated.is_finite() || measured.integrated < SILENCE_LUFS {
        return None;
    }
    let number = |value: f64, low: f64, high: f64| number_f64(value.clamp(low, high), 'f', 2);
    // loudnorm only stays linear when the range it aims for is at least the measured one.
    let range = measured.range.clamp(LOUDNESS_RANGE, 50.0).max(LOUDNESS_RANGE);
    Some(format!(
        "loudnorm=I={}:TP={}:LRA={}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true:print_format=none,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=N/SR/TB",
        number_f64(target, 'g', 6),
        number_f64(TRUE_PEAK, 'g', 6),
        number(range, 1.0, 50.0),
        number(measured.integrated, -99.0, 0.0),
        number(if measured.true_peak.is_finite() { measured.true_peak } else { -99.0 }, -99.0, 99.0),
        number(measured.range, 0.0, 99.0),
        number(if measured.threshold.is_finite() { measured.threshold } else { -99.0 }, -99.0, 0.0),
        number(measured.offset, -99.0, 99.0),
    ))
}

/// Runs `program` (ffmpeg, possibly behind `nice`) with `arguments`, which should ask for
/// `-progress pipe:1`: each `out_time_us` it reports goes to `progress` as a fraction of
/// `duration`. Stops it when `cancelled` is set ("cancelled"). Its standard error on success,
/// or why it failed.
pub fn run_ffmpeg(
    program: &str,
    arguments: &[String],
    duration: f64,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<String, String> {
    let mut process = Process::new();
    process.start(program, arguments);
    if !process.wait_for_started(5000) || process.state() == ProcessState::NotRunning {
        return Err(format!("Could not start {program}: {}", process.error_string()));
    }
    let mut pending = String::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            process.kill();
            process.wait_for_finished(5000);
            return Err("cancelled".into());
        }
        let ready = process.wait_for_ready_read(50);
        pending += &qstring::from_utf8(&process.read_all_standard_output());
        while let Some(end) = pending.find('\n') {
            let line: String = pending.drain(..=end).collect();
            if let Some(micros) = line.trim().strip_prefix("out_time_us=").and_then(|v| v.parse::<i64>().ok()) {
                if duration > 0.0 {
                    progress((micros as f64 / 1e6 / duration).clamp(0.0, 1.0));
                }
            }
        }
        if !ready && process.state() == ProcessState::NotRunning {
            break;
        }
    }
    process.wait_for_finished(5000);
    let log = qstring::from_utf8(&process.read_all_standard_error());
    if process.exit_code() != 0 {
        let last = log.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("").trim().to_owned();
        return Err(if last.is_empty() { format!("{program} failed") } else { last });
    }
    Ok(log)
}

/// What the cleaned sound is made with: the denoise strength (`None`: no denoising) and the
/// loudness target in LUFS (`None`: as recorded).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanSettings {
    pub denoise: Option<f64>,
    pub loudness: Option<f64>,
}

impl CleanSettings {
    /// What identifies these settings in cache keys: they and the model.
    pub fn key(&self) -> String {
        format!(
            "v2|{}|{}|{}",
            self.denoise.map_or("off".into(), |strength| number_f64(strength, 'g', 6)),
            self.loudness.map_or("off".into(), |target| number_f64(target, 'g', 6)),
            &qdir::sha256_hex(MODEL)[..12]
        )
    }
}

/// The directory the editor keeps cleaned sound in, under its cache location.
pub fn cache_directory(cache_location: &str) -> String {
    qdir::file_path(cache_location, "audio-cleanup")
}

/// A cache file's name: the fingerprint of the file it was made from
/// ([`crate::editor::video_fingerprint`]), `-`, a hash of how, and `suffix`.
fn cache_name(fingerprint: &str, how: &str, suffix: &str) -> String {
    format!("{fingerprint}-{}{suffix}", &qdir::sha256_hex(how.as_bytes())[..16])
}

/// How many cleaned versions of one file stay cached, so switching back is instant.
const KEEP_PER_FILE: usize = 3;
/// How many loudness measurements of one recording stay cached (a few bytes each).
const KEEP_MEASUREMENTS_PER_FILE: usize = 50;
/// A cache entry unused this long goes.
const CACHE_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);
/// The cache stays under this size, the least recently used entries going first.
const CACHE_MAX_BYTES: u64 = 2 << 30;
/// A partial file nobody wrote to this long is a crashed render's leftover.
const PART_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

/// The edited program's sound, as export joins it ([`crate::export::plan::audio_inputs`]): what
/// loudness normalisation measures, so the preview normalises with the gain the export will.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgramSound {
    pub clips: Vec<crate::project::Clip>,
    /// The inserted videos (bundle-relative paths) that have sound.
    pub insert_audio: Vec<String>,
    /// The recording's sound is in the program (`audio.desktop` or `audio.mic`).
    pub use_source_audio: bool,
}

impl ProgramSound {
    /// The measuring pass's filter graph and inputs (from input 0), denoising with `denoise`.
    pub fn measure_graph(&self, bundle: &str, video_path: &str, denoise: Option<&str>, target: f64) -> Option<(Vec<String>, String)> {
        let has_audio = |media: &str| self.insert_audio.iter().any(|path| path == media);
        let sources = crate::export::plan::audio_inputs(&self.clips, &has_audio, self.use_source_audio, bundle, video_path, 0, denoise, &[]);
        if sources.segments.is_empty() {
            return None;
        }
        let filter =
            crate::export::plan::program_measure_filter(&sources.prelude, &sources.segments, &loudness_measure_filter(target));
        Some((sources.arguments, filter))
    }
}

/// One inserted video whose sound the preview plays cleaned.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertSound {
    /// Its bundle-relative path, as clips name it.
    pub media: String,
    pub fingerprint: String,
}

/// What a [`CleanAudioJob`] made: the files the preview plays in place of the recording's and
/// the inserted videos' own sound (empty or missing: their own), and the loudness it applied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CleanAudioResult {
    pub recording: String,
    /// (bundle-relative path, cleaned file) of each inserted video.
    pub inserts: Vec<(String, String)>,
    /// The measurement the normalisation used; `None` without normalisation or sound.
    pub applied: Option<LoudnessMeasurement>,
}

/// Two measurements this close normalise alike to the ear (well under 1 dB apart): the preview
/// keeps the files it has instead of making them again after every small edit.
pub fn close_enough(a: &LoudnessMeasurement, b: &LoudnessMeasurement) -> bool {
    (a.integrated - b.integrated).abs() <= 0.5
        && (a.true_peak - b.true_peak).abs() <= 0.5
        && (a.range - b.range).abs() <= 1.0
        && (a.threshold - b.threshold).abs() <= 1.0
}

/// Makes the preview's cleaned sound at a low priority: the recording's, denoised (as one
/// continuous stream, like export) and normalised, and, when normalising, each inserted video's
/// with the same gain. The gain comes from measuring the edited program, exactly as export's
/// measuring pass does, so the preview and the export agree; the measurement is cached.
#[derive(Clone, Debug)]
pub struct CleanAudioJob {
    /// Tells this job from any other, finished ones included.
    pub id: u64,
    pub bundle: String,
    pub video_path: String,
    /// The recording's [`crate::editor::video_fingerprint`].
    pub fingerprint: String,
    pub cache_directory: String,
    pub settings: CleanSettings,
    /// The program to measure when normalising.
    pub program: ProgramSound,
    pub inserts: Vec<InsertSound>,
    /// What the files playing now were normalised with: used again when the program measures
    /// [`close_enough`] to it.
    pub previous: Option<LoudnessMeasurement>,
    pub cancelled: Arc<AtomicBool>,
}

impl CleanAudioJob {
    /// Makes the files; `Err("cancelled")` when cancelled. Files already made are used again
    /// (and count as used, for the cache's eviction).
    pub fn run(&self) -> Result<CleanAudioResult, String> {
        let directory = &self.cache_directory;
        private_directory(directory)?;
        let model = match self.settings.denoise {
            Some(_) => Some(model_file(&default_model_directory())?),
            None => None,
        };
        let denoise = self.settings.denoise.zip(model.as_deref()).map(|(strength, model)| denoise_filter(model, strength));
        let mut result = CleanAudioResult::default();
        let mut apply = None;
        if let Some(target) = self.settings.loudness {
            if let Some(measured) = self.measure(target, denoise.as_deref())? {
                let applied = match self.previous {
                    Some(previous) if close_enough(&previous, &measured) => previous,
                    _ => measured,
                };
                apply = loudness_apply_filter(target, &applied);
                result.applied = apply.is_some().then_some(applied);
            }
        }
        let chain = |source: &str, cleanup: &[Option<&str>]| {
            let cleanup: Vec<&str> = cleanup.iter().flatten().copied().collect();
            crate::export::plan::recording_audio_chain(source, &cleanup.join(","))
                .trim_end_matches(',')
                .to_owned()
        };
        if denoise.is_some() || apply.is_some() {
            let how = format!("recording|{}|{}", self.settings.key(), apply.as_deref().unwrap_or(""));
            let output = qdir::file_path(directory, &cache_name(&self.fingerprint, &how, ".m4a"));
            self.render(&self.video_path, &chain("[0:a:0]", &[denoise.as_deref(), apply.as_deref()]), &output)?;
            result.recording = output;
        }
        if let Some(apply) = &apply {
            for insert in &self.inserts {
                let how = format!("insert|{apply}");
                let output = qdir::file_path(directory, &cache_name(&insert.fingerprint, &how, ".m4a"));
                let source = qdir::file_path(&self.bundle, &insert.media);
                self.render(&source, &chain("[0:a:0]", &[Some(apply)]), &output)?;
                result.inserts.push((insert.media.clone(), output));
            }
        }
        let mut keep: Vec<String> = result.inserts.iter().map(|(_, path)| path.clone()).collect();
        keep.push(result.recording.clone());
        prune_cache(directory, &keep);
        Ok(result)
    }

    /// The program's loudness, from the cache or measured (and cached); `None` when it has
    /// no sound.
    fn measure(&self, target: f64, denoise: Option<&str>) -> Result<Option<LoudnessMeasurement>, String> {
        let Some((inputs, filter)) = self.program.measure_graph(&self.bundle, &self.video_path, denoise, target) else {
            return Ok(None);
        };
        // The graph names the model's path and the inserts' paths, and the fingerprints say
        // whether the files changed.
        let fingerprints: Vec<&str> = self.inserts.iter().map(|insert| insert.fingerprint.as_str()).collect();
        let how = format!("measure|{}|{filter}|{}", self.settings.key(), fingerprints.join(","));
        let cached = qdir::file_path(&self.cache_directory, &cache_name(&self.fingerprint, &how, ".loudness.json"));
        if let Some(measured) = qfile::read_all(&cached).ok().and_then(|bytes| parse_loudnorm(&qstring::from_utf8(&bytes))) {
            touch(&cached);
            return Ok(Some(measured));
        }
        let mut args: Vec<String> =
            ["-n", "10", "ffmpeg", "-hide_banner", "-nostdin", "-nostats", "-v", "info"].map(String::from).into();
        args.extend(inputs);
        args.extend(["-filter_complex".into(), filter, "-map".into(), "[aout]".into(), "-f".into(), "null".into(), "-".into()]);
        let log = run_ffmpeg("nice", &args, 0.0, &self.cancelled, &mut |_| {})?;
        let measured = parse_loudnorm(&log).ok_or("Could not measure the loudness")?;
        // The report as loudnorm wrote it, for parse_loudnorm to read back.
        let report = format!(
            "{{\"input_i\" : \"{}\", \"input_tp\" : \"{}\", \"input_lra\" : \"{}\", \"input_thresh\" : \"{}\", \"target_offset\" : \"{}\"}}\n",
            measured.integrated, measured.true_peak, measured.range, measured.threshold, measured.offset
        );
        let _ = qsavefile::save(&cached, report.as_bytes());
        Ok(Some(measured))
    }

    /// Renders `input`'s first audio track through `chain` to `output` (AAC), unless it is
    /// there already. The render goes to a partial file of this job's own, renamed into place
    /// only when the job is still wanted, so jobs for the same file never share one.
    fn render(&self, input: &str, chain: &str, output: &str) -> Result<(), String> {
        if FileInfo::new(output).is_file() {
            touch(output);
            return Ok(());
        }
        let partial = format!("{output}.{}-{}.part.m4a", self.id, &crate::editor::random_uuid()[..8]);
        let mut args: Vec<String> =
            ["-n", "10", "ffmpeg", "-hide_banner", "-nostdin", "-nostats", "-v", "error", "-n", "-i"].map(String::from).into();
        args.push(input.to_owned());
        args.extend(["-filter_complex".into(), format!("{chain}[a]")]);
        args.extend(["-map", "[a]", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"].map(String::from));
        args.push(partial.clone());
        let result = run_ffmpeg("nice", &args, 0.0, &self.cancelled, &mut |_| {});
        if self.cancelled.load(Ordering::SeqCst) {
            qfile::remove(&partial);
            return Err("cancelled".into());
        }
        if let Err(error) = result {
            qfile::remove(&partial);
            return Err(error);
        }
        if std::fs::rename(&partial, output).is_err() {
            qfile::remove(&partial);
            return Err("Could not keep the cleaned audio".into());
        }
        Ok(())
    }
}

/// Marks a cache entry as just used: eviction goes by when entries were last used.
fn touch(path: &str) {
    if let Ok(file) = std::fs::File::options().append(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

/// Keeps the cache in bounds; `keep` (the files just made or used) always stays. Removes
/// partial files nobody has written to for an hour (a crashed render's), anything unused for
/// [`CACHE_MAX_AGE`], all but the newest [`KEEP_PER_FILE`] cleaned versions of each file (and
/// [`KEEP_MEASUREMENTS_PER_FILE`] measurements), then the least recently used entries until the
/// whole is under [`CACHE_MAX_BYTES`]. Other recordings' entries count too: a deleted
/// recording's go by age and size.
pub fn prune_cache(directory: &str, keep: &[String]) {
    prune_cache_at(directory, keep, std::time::SystemTime::now(), CACHE_MAX_BYTES)
}

fn prune_cache_at(directory: &str, keep: &[String], now: std::time::SystemTime, max_bytes: u64) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    let kept: Vec<String> = keep.iter().filter(|path| !path.is_empty()).map(|path| FileInfo::new(path).file_name()).collect();
    let age = |modified: std::time::SystemTime| now.duration_since(modified).unwrap_or_default();
    // (last used, size, path, name) of each entry that may go.
    let mut candidates: Vec<(std::time::SystemTime, u64, std::path::PathBuf, String)> = Vec::new();
    let mut total = 0u64;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(metadata) = entry.metadata() else { continue };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        if name.ends_with(".part.m4a") {
            if age(modified) > PART_MAX_AGE {
                let _ = std::fs::remove_file(entry.path());
            } else {
                total += metadata.len();
            }
            continue;
        }
        if !name.ends_with(".m4a") && !name.ends_with(".loudness.json") {
            continue;
        }
        if !kept.contains(&name) && age(modified) > CACHE_MAX_AGE {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        total += metadata.len();
        if !kept.contains(&name) {
            candidates.push((modified, metadata.len(), entry.path(), name));
        }
    }
    // Newest first; per file (the fingerprint before the first '-') and kind, all but the
    // newest few go, counting the kept ones.
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let group = |name: &str| {
        let fingerprint = name.split('-').next().unwrap_or("").to_owned();
        (fingerprint, name.ends_with(".loudness.json"))
    };
    let mut counts: std::collections::HashMap<(String, bool), usize> = std::collections::HashMap::new();
    for name in &kept {
        *counts.entry(group(name)).or_default() += 1;
    }
    let mut remaining = Vec::new();
    for candidate in candidates {
        let key = group(&candidate.3);
        let limit = if key.1 { KEEP_MEASUREMENTS_PER_FILE } else { KEEP_PER_FILE };
        let count = counts.entry(key).or_default();
        *count += 1;
        if *count > limit {
            total -= candidate.1;
            let _ = std::fs::remove_file(&candidate.2);
        } else {
            remaining.push(candidate);
        }
    }
    // Over the size limit: the least recently used first.
    while total > max_bytes {
        let Some((_, size, path, _)) = remaining.pop() else { break };
        total -= size;
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qstandardpaths::find_executable;

    #[test]
    fn filter_values_are_escaped_twice() {
        assert_eq!(escape_filter_value("/run/user/1000/m.rnnn"), "/run/user/1000/m.rnnn");
        assert_eq!(escape_filter_value("a:b"), "a\\\\:b");
        assert_eq!(escape_filter_value("a,b[c];d"), "a\\,b\\[c\\]\\;d");
        assert_eq!(escape_filter_value("it's"), "it\\\\\\'s");
    }

    #[test]
    fn filter_strings() {
        assert_eq!(
            denoise_filter("/run/m.rnnn", 0.85),
            "apad=pad_len=480,arnndn=m=/run/m.rnnn:mix=0.85,atrim=start_sample=480,asetpts=PTS-STARTPTS"
        );
        assert!(denoise_filter("/run/m.rnnn", 1.0).contains(",arnndn=m=/run/m.rnnn:mix=1,"));
        assert_eq!(loudness_measure_filter(-16.0), "loudnorm=I=-16:TP=-2:LRA=11:print_format=json");
        let measured =
            LoudnessMeasurement { integrated: -27.61, true_peak: -4.47, range: 18.06, threshold: -39.2, offset: 0.58 };
        assert_eq!(
            loudness_apply_filter(-14.0, &measured).unwrap(),
            "loudnorm=I=-14:TP=-2:LRA=18.06:measured_I=-27.61:measured_TP=-4.47:measured_LRA=18.06:\
             measured_thresh=-39.20:offset=0.58:linear=true:print_format=none,aresample=48000,\
             aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=N/SR/TB"
        );
        let narrow = LoudnessMeasurement { range: 4.0, ..measured };
        assert!(loudness_apply_filter(-16.0, &narrow).unwrap().contains(":LRA=11.00:"));
        let silent = LoudnessMeasurement { integrated: f64::NEG_INFINITY, ..measured };
        assert_eq!(loudness_apply_filter(-16.0, &silent), None);
    }

    #[test]
    fn reads_loudnorms_report() {
        let log = "[Parsed_loudnorm_0 @ 0x55] \n{\n\t\"input_i\" : \"-27.61\",\n\t\"input_tp\" : \"-4.47\",\n\
                   \t\"input_lra\" : \"18.06\",\n\t\"input_thresh\" : \"-39.20\",\n\t\"output_i\" : \"-16.58\",\n\
                   \t\"output_tp\" : \"-1.50\",\n\t\"output_lra\" : \"14.78\",\n\t\"output_thresh\" : \"-27.71\",\n\
                   \t\"normalization_type\" : \"dynamic\",\n\t\"target_offset\" : \"0.58\"\n}\n[out#0/null @ 0x56] video:0KiB";
        assert_eq!(
            parse_loudnorm(log),
            Some(LoudnessMeasurement { integrated: -27.61, true_peak: -4.47, range: 18.06, threshold: -39.2, offset: 0.58 })
        );
        let silent = log.replace("\"-27.61\"", "\"-inf\"");
        assert_eq!(parse_loudnorm(&silent).unwrap().integrated, f64::NEG_INFINITY);
        assert_eq!(parse_loudnorm("no report"), None);
    }

    #[test]
    fn cache_keys_follow_the_settings() {
        let a = CleanSettings { denoise: Some(0.85), loudness: None };
        let b = CleanSettings { denoise: Some(0.85), loudness: Some(-16.0) };
        assert_ne!(a.key(), b.key());
        assert_eq!(a.key(), a.key());
        assert!(cache_name("f00d", &a.key(), ".m4a").starts_with("f00d-"));
        assert_ne!(cache_name("f00d", &a.key(), ".m4a"), cache_name("f00d", &b.key(), ".m4a"));
    }

    #[test]
    fn the_model_goes_to_a_private_directory_and_is_checked_before_use() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("omareel-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("cache/omareel").to_string_lossy().into_owned();
        let path = model_file(&models).unwrap();
        let mode = |path: &str| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&models), 0o700);
        assert_eq!(std::fs::read(&path).unwrap(), MODEL);
        // Something else of the same size in its place is replaced, not used.
        let mut forged = MODEL.to_vec();
        forged[100] ^= 0xff;
        std::fs::write(&path, &forged).unwrap();
        assert_eq!(model_file(&models).unwrap(), path);
        assert_eq!(std::fs::read(&path).unwrap(), MODEL);
        // A directory of ours others could use is closed to them.
        std::fs::set_permissions(&models, std::fs::Permissions::from_mode(0o777)).unwrap();
        model_file(&models).unwrap();
        assert_eq!(mode(&models), 0o700);
        // The default: never the shared temporary directory.
        let default = default_model_directory();
        assert!(!default.starts_with(&qdir::temp_path()) || default == qdir::env_var("XDG_RUNTIME_DIR").unwrap_or_default(),
                "{default}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_cache_stays_in_bounds() {
        use std::time::{Duration, SystemTime};
        let dir = std::env::temp_dir().join(format!("omareel-clean-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let make = |name: &str, bytes: usize, age_hours: u64| {
            let path = dir.join(name);
            std::fs::write(&path, vec![0u8; bytes]).unwrap();
            let file = std::fs::File::options().append(true).open(&path).unwrap();
            file.set_modified(now - Duration::from_secs(age_hours * 3600)).unwrap();
            path.to_string_lossy().into_owned()
        };
        let exists = |name: &str| dir.join(name).exists();
        // An interrupted render's leftover goes; one being written stays.
        make("aaaa-1.m4a.7-x.part.m4a", 10, 2);
        make("aaaa-2.m4a.8-y.part.m4a", 10, 0);
        // Another recording, deleted long ago: unused for 40 days.
        make("dead-1.m4a", 10, 40 * 24);
        make("dead-1.loudness.json", 10, 40 * 24);
        // This recording: the kept one and the newest two others stay.
        let kept = make("aaaa-0.m4a", 10, 5);
        for (i, age) in [1, 2, 3, 4].iter().enumerate() {
            make(&format!("aaaa-{}.m4a", i + 1), 10, *age);
        }
        make("aaaa-9.loudness.json", 10, 1);
        // Two other recordings, used recently.
        make("bbbb-1.m4a", 100, 6);
        make("cccc-1.m4a", 100, 7);
        // Not the cache's: left alone.
        make("notes.txt", 10, 50 * 24);
        let dir_path = dir.to_string_lossy().into_owned();
        prune_cache_at(&dir_path, std::slice::from_ref(&kept), now, 1 << 20);
        assert!(!exists("aaaa-1.m4a.7-x.part.m4a") && exists("aaaa-2.m4a.8-y.part.m4a"));
        assert!(!exists("dead-1.m4a") && !exists("dead-1.loudness.json"));
        assert!(exists("aaaa-0.m4a") && exists("aaaa-1.m4a") && exists("aaaa-2.m4a"));
        assert!(!exists("aaaa-3.m4a") && !exists("aaaa-4.m4a"));
        assert!(exists("aaaa-9.loudness.json") && exists("bbbb-1.m4a") && exists("cccc-1.m4a") && exists("notes.txt"));
        // Over the size limit: the least recently used go first, never the kept one.
        prune_cache_at(&dir_path, std::slice::from_ref(&kept), now, 140);
        assert!(!exists("cccc-1.m4a") && !exists("bbbb-1.m4a"), "the oldest two");
        assert!(exists("aaaa-0.m4a") && exists("aaaa-1.m4a"));
        prune_cache_at(&dir_path, std::slice::from_ref(&kept), now, 0);
        assert!(exists("aaaa-0.m4a") && exists("aaaa-2.m4a.8-y.part.m4a"));
        assert!(!exists("aaaa-1.m4a") && !exists("aaaa-9.loudness.json"));
        // Using an entry makes it the newest.
        touch(&kept);
        let modified = std::fs::metadata(&kept).unwrap().modified().unwrap();
        assert!(now.duration_since(modified).unwrap_or_default() < Duration::from_secs(60));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn have_ffmpeg() -> bool {
        !find_executable("ffmpeg").is_empty() && !find_executable("ffprobe").is_empty()
    }

    /// The level of `path` between `from` and `to` seconds, in dBFS RMS (ffmpeg's astats).
    pub(crate) fn rms_db(path: &str, from: f64, to: f64) -> f64 {
        let filter = format!("atrim={from}:{to},astats=measure_overall=RMS_level:measure_perchannel=0");
        let out = crate::compat::qprocess::run(
            "ffmpeg",
            &["-hide_banner", "-nostats", "-v", "info", "-i", path, "-af", &filter, "-f", "null", "-"],
            60000,
        )
        .unwrap();
        let log = String::from_utf8_lossy(&out.stderr);
        let line = log.lines().filter(|l| l.contains("RMS level dB:")).last().expect("astats report");
        line.rsplit(':').next().unwrap().trim().parse().unwrap_or(f64::NEG_INFINITY)
    }

    /// Integrated loudness of `path` in LUFS, from ffmpeg's ebur128.
    pub(crate) fn integrated_lufs(path: &str) -> f64 {
        let out = crate::compat::qprocess::run(
            "ffmpeg",
            &["-hide_banner", "-nostats", "-v", "info", "-i", path, "-af", "ebur128", "-f", "null", "-"],
            60000,
        )
        .unwrap();
        let log = String::from_utf8_lossy(&out.stderr);
        let summary = &log[log.rfind("Integrated loudness:").expect("ebur128 summary")..];
        let line = summary.lines().find(|l| l.trim_start().starts_with("I:")).unwrap();
        line.trim().trim_start_matches("I:").trim().trim_end_matches("LUFS").trim().parse().unwrap()
    }

    /// A 12 s test recording: espeak-ng speech (a buzz talking in syllables without it) from 2 s
    /// to 9 s over pink and brown noise ("fan and hiss"), with a black video. True when the
    /// voice is speech: RNNoise takes a buzz for noise.
    pub(crate) fn noisy_recording(dir: &std::path::Path) -> (String, bool) {
        let path = dir.join("screen.mp4").to_string_lossy().into_owned();
        let speech = dir.join("speech.wav").to_string_lossy().into_owned();
        let espeak = !find_executable("espeak-ng").is_empty()
            && crate::compat::qprocess::run(
                "espeak-ng",
                &["-v", "en-us", "-s", "150", "-w", &speech,
                  "Hello, and welcome to this screen recording. Today I will show you how to set up the editor."],
                30000,
            )
            .is_some_and(|r| r.succeeded());
        let voice = if espeak {
            format!("amovie={},aresample=48000,apad=whole_dur=7,atrim=0:7", escape_filter_value(&speech))
        } else {
            // A buzzy 140 Hz voice that talks in syllables.
            "aevalsrc='0.4*sin(2*PI*140*t)*(0.6+0.4*sin(2*PI*3*t))*gt(sin(2*PI*1.7*t),-0.3)':s=48000:d=7"
                .into()
        };
        let graph = format!(
            "{voice},adelay=2000:all=1,apad=whole_dur=12,volume=0.5[v];\
             anoisesrc=c=pink:r=48000:a=0.02:d=12:seed=1[p];anoisesrc=c=brown:r=48000:a=0.03:d=12:seed=2[b];\
             [v][p][b]amix=inputs=3:normalize=0,aformat=channel_layouts=stereo[a]"
        );
        let out = crate::compat::qprocess::run(
            "ffmpeg",
            &["-y", "-v", "error", "-f", "lavfi", "-i", "color=c=black:s=320x180:r=30:d=12", "-filter_complex", &graph,
              "-map", "0:v", "-map", "[a]", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-b:a", "192k",
              "-shortest", &path],
            120000,
        )
        .unwrap();
        assert!(out.succeeded(), "{}", String::from_utf8_lossy(&out.stderr));
        (path, espeak)
    }

    /// A job for the recording `video` (whole, one clip) in `dir`, with `settings`.
    fn job(dir: &std::path::Path, video: &str, duration: f64, settings: CleanSettings) -> CleanAudioJob {
        CleanAudioJob {
            id: 1,
            bundle: dir.to_string_lossy().into_owned(),
            video_path: video.to_owned(),
            fingerprint: crate::editor::video_fingerprint(video),
            cache_directory: dir.join("cache").to_string_lossy().into_owned(),
            settings,
            program: ProgramSound {
                clips: vec![crate::project::Clip { id: "c".into(), in_: 0.0, out: duration, speed: 1.0, ..Default::default() }],
                insert_audio: Vec::new(),
                use_source_audio: true,
            },
            inserts: Vec::new(),
            previous: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn duration_of(path: &str) -> f64 {
        let probe = crate::compat::qprocess::run(
            "ffprobe", &["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path], 15000).unwrap();
        String::from_utf8_lossy(&probe.stdout).trim().parse().unwrap()
    }

    fn leftovers(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir.join("cache"))
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".part")).collect())
            .unwrap_or_default()
    }

    #[test]
    fn denoising_lowers_the_noise_floor() {
        if !have_ffmpeg() {
            eprintln!("ffmpeg/ffprobe unavailable");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-denoise-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (video, speech) = noisy_recording(&dir);
        let job = job(&dir, &video, 12.0, CleanSettings { denoise: Some(0.85), loudness: None });
        let result = job.run().unwrap();
        let output = result.recording.clone();
        assert!(output.starts_with(&job.cache_directory) && FileInfo::new(&output).is_file());
        assert_eq!(result.applied, None);
        // The noise alone (0.2-1.8 s) and the voice over it (3-8 s), before and after.
        let (noise_before, noise_after) = (rms_db(&video, 0.2, 1.8), rms_db(&output, 0.2, 1.8));
        let (voice_before, voice_after) = (rms_db(&video, 3.0, 8.0), rms_db(&output, 3.0, 8.0));
        eprintln!("noise floor {noise_before:.1} -> {noise_after:.1} dB, voice {voice_before:.1} -> {voice_after:.1} dB");
        assert!(noise_before - noise_after >= 10.0, "the noise floor fell {:.1} dB", noise_before - noise_after);
        if speech {
            assert!(voice_before - voice_after <= 3.0, "the voice fell {:.1} dB", voice_before - voice_after);
        }
        // Same length: the preview plays it on the recording's timeline.
        let duration = duration_of(&output);
        assert!((duration - 12.0).abs() < 0.1, "duration {duration}");
        // Made once: a second run finds it.
        assert_eq!(job.run().unwrap(), result);

        // Loudness too: the whole recording at -16 LUFS, measured once.
        let loud = CleanAudioJob { settings: CleanSettings { denoise: speech.then_some(1.0), loudness: Some(-16.0) }, ..job.clone() };
        let result = loud.run().unwrap();
        assert!(result.applied.is_some());
        let lufs = integrated_lufs(&result.recording);
        assert!((lufs + 16.0).abs() <= 1.0, "integrated loudness {lufs}");
        let measurements = std::fs::read_dir(&job.cache_directory)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".loudness.json"))
            .count();
        assert_eq!(measurements, 1);
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    // W1 -> W2 -> W1 in the editor left two jobs making the same file through the same partial
    // file. Two jobs for one file now each write their own, and both end with the whole file.
    #[test]
    fn jobs_for_the_same_file_do_not_share_a_partial_file() {
        if !have_ffmpeg() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-clean-twice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (video, _) = noisy_recording(&dir);
        let first = job(&dir, &video, 12.0, CleanSettings { denoise: Some(0.5), loudness: Some(-14.0) });
        let second = CleanAudioJob { id: 2, ..first.clone() };
        let runs = [first, second].map(|job| std::thread::spawn(move || job.run()));
        let [a, b] = runs.map(|run| run.join().unwrap().unwrap());
        assert_eq!(a.recording, b.recording);
        assert!((duration_of(&a.recording) - 12.0).abs() < 0.1);
        assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
        // A job cancelled once its render is done does not put its file in place.
        let cancelled = job(&dir, &video, 12.0, CleanSettings { denoise: Some(0.3), loudness: None });
        cancelled.cancelled.store(true, Ordering::SeqCst);
        assert_eq!(cancelled.run(), Err("cancelled".into()));
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    // The preview normalises inserted videos with the program's gain, like export, and keeps
    // the recording's sound where the screen player has it (ffmpeg's timestamps already count
    // from the container's start).
    #[test]
    fn inserted_videos_get_the_programs_gain() {
        if !have_ffmpeg() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-clean-inserts-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("media")).unwrap();
        // The recording: 3 s, a tone from 0.5 s, in a container that starts at 1 s.
        let video = dir.join("screen.mp4").to_string_lossy().into_owned();
        let insert = dir.join("media/clip.mp4").to_string_lossy().into_owned();
        for (path, graph, offset) in [
            (&video, "sine=f=440:d=2.5:sample_rate=48000,volume=0.1,adelay=500:all=1,apad=whole_dur=3", "1"),
            (&insert, "sine=f=660:d=2:sample_rate=48000,volume=0.5", "0"),
        ] {
            let made = crate::compat::qprocess::run(
                "ffmpeg",
                &["-y", "-v", "error", "-f", "lavfi", "-i", "color=c=black:s=64x64:r=30:d=3", "-f", "lavfi", "-i", graph,
                  "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-shortest",
                  "-output_ts_offset", offset, path],
                60000,
            )
            .unwrap();
            assert!(made.succeeded(), "{}", String::from_utf8_lossy(&made.stderr));
        }
        let mut job = job(&dir, &video, 3.0, CleanSettings { denoise: None, loudness: Some(-16.0) });
        job.program.clips.push(crate::project::Clip {
            id: "m".into(), in_: 0.0, out: 2.0, speed: 1.0, media: "media/clip.mp4".into(), ..Default::default()
        });
        job.program.insert_audio = vec!["media/clip.mp4".into()];
        job.inserts = vec![InsertSound { media: "media/clip.mp4".into(), fingerprint: crate::editor::video_fingerprint(&insert) }];
        let result = job.run().unwrap();
        let [(media, cleaned_insert)] = &result.inserts[..] else { panic!("{result:?}") };
        assert_eq!(media, "media/clip.mp4");
        // One gain for both.
        let recording_gain = rms_db(&result.recording, 1.0, 2.5) - rms_db(&video, 1.0, 2.5);
        let insert_gain = rms_db(cleaned_insert, 0.5, 1.5) - rms_db(&insert, 0.5, 1.5);
        assert!(recording_gain > 3.0, "the quiet program is raised: {recording_gain:.1} dB");
        assert!((recording_gain - insert_gain).abs() < 0.3, "recording {recording_gain:.1} dB, insert {insert_gain:.1} dB");
        // The tone still starts where the screen player has it: silence until 0.5 s, then the
        // tone, in the recording and in its cleaned sound alike (it was a second early).
        for path in [&video, &result.recording] {
            assert!(rms_db(path, 0.05, 0.45) < -70.0, "{path} before the tone");
            assert!(rms_db(path, 0.52, 0.6) > -50.0, "{path} at the tone");
        }
        // A measurement close to the one playing keeps it: nothing is made again.
        let applied = result.applied.unwrap();
        let nudged = LoudnessMeasurement { integrated: applied.integrated + 0.3, ..applied };
        let again = CleanAudioJob { id: 2, previous: Some(nudged), ..job.clone() };
        assert_eq!(again.run().unwrap().applied, Some(nudged));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cancelled_job_leaves_nothing() {
        if !have_ffmpeg() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-denoise-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("missing.mp4").to_string_lossy().into_owned();
        let job = job(&dir, &missing, 1.0, CleanSettings { denoise: None, loudness: Some(-16.0) });
        job.cancelled.store(true, Ordering::SeqCst);
        assert_eq!(job.run(), Err("cancelled".into()));
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
