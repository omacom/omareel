//! Cleaning up the recording's sound (`audio.cleanup` in project.json): noise suppression and
//! loudness normalisation, as ffmpeg filters.
//!
//! Noise suppression is RNNoise, through ffmpeg's `arnndn`, with GregorR's "somnolent-hogwash"
//! model (speech in a recording environment: fans, air conditioning, computers), built into
//! the binary and written next to the export's other temporary files when a filter needs it.
//! On synthetic noisy speech it lowered the noise floor by 11-20 dB where `afftdn` managed 0-2
//! dB at any setting (docs/USAGE.md has the numbers); `anlmdn` corrupts the heap in ffmpeg 9.0.
//!
//! Loudness normalisation is two-pass: [`loudness_measure_filter`] measures the finished mix,
//! and [`loudness_apply_filter`] applies the gain that brings it to the target, linearly when
//! the gain keeps the true peak under [`TRUE_PEAK`], otherwise with loudnorm's limiter.
//!
//! Export puts both into its audio graph; the editor renders the cleaned recording to a cache
//! file ([`CleanAudioJob`]) that the preview plays instead of the recording's own sound.

use crate::compat::qdir::{self, FileInfo};
use crate::compat::qjson;
use crate::compat::qnumber::number_f64;
use crate::compat::qprocess::{Process, ProcessState};
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

/// Writes [`MODEL`] into `directory` (once per model: the name carries its hash) and returns its
/// path.
pub fn model_file(directory: &str) -> Result<String, String> {
    let name = format!("omareel-rnnoise-{}.rnnn", &qdir::sha256_hex(MODEL)[..12]);
    let path = qdir::file_path(directory, &name);
    if FileInfo::new(&path).size() == MODEL.len() as i64 {
        return Ok(path);
    }
    qdir::mkpath(directory);
    qsavefile::save(&path, MODEL).map_err(|error| format!("Could not prepare noise reduction: {error}"))?;
    Ok(path)
}

/// Where [`model_file`] puts the model by default: the runtime directory, or the temporary one.
pub fn default_model_directory() -> String {
    match qdir::env_var("XDG_RUNTIME_DIR") {
        Some(runtime) if !runtime.is_empty() && FileInfo::new(&runtime).is_writable() => runtime,
        _ => qdir::temp_path(),
    }
}

/// The noise suppression filter for 48 kHz audio: RNNoise with the model at `model_path`, its
/// result mixed with the original by `strength` (1: all denoised). arnndn's output runs one
/// 10 ms frame (480 samples) late, wet and dry alike; the trim takes that back, so the sound
/// stays on the picture.
pub fn denoise_filter(model_path: &str, strength: f64) -> String {
    format!(
        "arnndn=m={}:mix={},atrim=start_sample=480,asetpts=PTS-STARTPTS",
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
/// at 192 kHz). `None` for silence, which is left alone.
pub fn loudness_apply_filter(target: f64, measured: &LoudnessMeasurement) -> Option<String> {
    if !measured.integrated.is_finite() || measured.integrated < SILENCE_LUFS {
        return None;
    }
    let number = |value: f64, low: f64, high: f64| number_f64(value.clamp(low, high), 'f', 2);
    // loudnorm only stays linear when the range it aims for is at least the measured one.
    let range = measured.range.clamp(LOUDNESS_RANGE, 50.0).max(LOUDNESS_RANGE);
    Some(format!(
        "loudnorm=I={}:TP={}:LRA={}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true:print_format=none,aresample=48000",
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
    /// The cache file's name for the recording `fingerprint`
    /// ([`crate::editor::video_fingerprint`]): it changes with the settings and the model.
    pub fn cache_name(&self, fingerprint: &str) -> String {
        let key = format!(
            "v1|{}|{}|{}",
            self.denoise.map_or("off".into(), |strength| number_f64(strength, 'g', 6)),
            self.loudness.map_or("off".into(), |target| number_f64(target, 'g', 6)),
            &qdir::sha256_hex(MODEL)[..12]
        );
        format!("{fingerprint}-{}.m4a", &qdir::sha256_hex(key.as_bytes())[..12])
    }
}

/// The directory the editor keeps cleaned sound in, under its cache location.
pub fn cache_directory(cache_location: &str) -> String {
    qdir::file_path(cache_location, "audio-cleanup")
}

/// How many cleaned versions of one recording stay cached, so switching back is instant.
const KEEP_PER_RECORDING: usize = 3;

/// Renders a recording's sound, cleaned, to an AAC file on the recording's timeline (time 0 is
/// the recording's start, as the screen player counts it) for the editor's preview. It runs
/// ffmpeg at a low priority, measuring the loudness first when it normalises; the measurement
/// covers the whole recording, where export measures the edited mix, so the two can differ by
/// a little when edits remove much louder or quieter parts.
#[derive(Clone, Debug)]
pub struct CleanAudioJob {
    pub video_path: String,
    /// The finished file; written next to it first, then renamed.
    pub output_path: String,
    pub settings: CleanSettings,
    /// The recording's length in seconds, for progress.
    pub duration: f64,
    pub cancelled: Arc<AtomicBool>,
}

impl CleanAudioJob {
    /// The source chain: the recording's first audio track on its own timeline, at 48 kHz
    /// stereo, denoised when asked.
    fn chain(&self, model: Option<&str>, start: f64) -> String {
        let mut chain = format!(
            "[0:a:0]asetpts=PTS-{}/TB,aresample=48000:async=1:first_pts=0,aformat=sample_fmts=fltp:channel_layouts=stereo",
            number_f64(start, 'f', 6)
        );
        if let (Some(strength), Some(model)) = (self.settings.denoise, model) {
            chain += ",";
            chain += &denoise_filter(model, strength);
        }
        chain
    }

    /// Makes the file; `Err("cancelled")` when cancelled.
    pub fn run(&self) -> Result<String, String> {
        let directory = FileInfo::new(&self.output_path).absolute_path();
        qdir::mkpath(&directory);
        let model = match self.settings.denoise {
            Some(_) => Some(model_file(&directory)?),
            None => None,
        };
        let start = container_start(&self.video_path);
        let mut chain = self.chain(model.as_deref(), start);
        let base: Vec<String> = ["-n", "10", "ffmpeg", "-hide_banner", "-nostdin", "-nostats", "-progress", "pipe:1"]
            .map(String::from)
            .into();
        if let Some(target) = self.settings.loudness {
            let mut args = base.clone();
            args.extend(["-v".into(), "info".into(), "-i".into(), self.video_path.clone(), "-filter_complex".into()]);
            args.push(format!("{chain},{}[a]", loudness_measure_filter(target)));
            args.extend(["-map", "[a]", "-f", "null", "-"].map(String::from));
            let log = run_ffmpeg("nice", &args, self.duration, &self.cancelled, &mut |_| {})?;
            let measured = parse_loudnorm(&log).ok_or("Could not measure the loudness")?;
            if let Some(apply) = loudness_apply_filter(target, &measured) {
                chain = format!("{chain},{apply}");
            }
        }
        let partial = format!("{}.part.m4a", self.output_path);
        let mut args = base;
        args.extend(["-v".into(), "error".into(), "-y".into(), "-i".into(), self.video_path.clone(), "-filter_complex".into()]);
        args.push(format!("{chain}[a]"));
        args.extend(["-map", "[a]", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"].map(String::from));
        args.push(partial.clone());
        let result = run_ffmpeg("nice", &args, self.duration, &self.cancelled, &mut |_| {});
        if let Err(error) = result {
            qfile::remove(&partial);
            return Err(error);
        }
        qfile::remove(&self.output_path);
        if !qfile::rename(&partial, &self.output_path) {
            qfile::remove(&partial);
            return Err("Could not keep the cleaned audio".into());
        }
        prune_cache(&directory, &self.output_path);
        Ok(self.output_path.clone())
    }
}

/// The container's start time in seconds (where the screen player's 0 is), 0 when unknown.
fn container_start(path: &str) -> f64 {
    let Some(probe) = crate::compat::qprocess::run(
        "ffprobe",
        &["-v", "error", "-show_entries", "format=start_time", "-of", "csv=p=0", path],
        15000,
    ) else {
        return 0.0;
    };
    let start = String::from_utf8_lossy(&probe.stdout).trim().parse::<f64>().unwrap_or(0.0);
    if start.is_finite() { start } else { 0.0 }
}

/// Keeps the newest [`KEEP_PER_RECORDING`] files of `kept`'s recording (its name up to the first
/// `-`), `kept` among them, and removes leftovers of interrupted renders.
fn prune_cache(directory: &str, kept: &str) {
    let kept_name = FileInfo::new(kept).file_name();
    let Some(prefix) = kept_name.split('-').next().map(|p| format!("{p}-")) else { return };
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    let mut siblings: Vec<(std::time::SystemTime, std::path::PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || name == kept_name {
            continue;
        }
        if name.ends_with(".part.m4a") {
            continue;
        }
        let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        siblings.push((modified, entry.path()));
    }
    siblings.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in siblings.into_iter().skip(KEEP_PER_RECORDING - 1) {
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
            "arnndn=m=/run/m.rnnn:mix=0.85,atrim=start_sample=480,asetpts=PTS-STARTPTS"
        );
        assert!(denoise_filter("/run/m.rnnn", 1.0).starts_with("arnndn=m=/run/m.rnnn:mix=1,"));
        assert_eq!(loudness_measure_filter(-16.0), "loudnorm=I=-16:TP=-2:LRA=11:print_format=json");
        let measured =
            LoudnessMeasurement { integrated: -27.61, true_peak: -4.47, range: 18.06, threshold: -39.2, offset: 0.58 };
        assert_eq!(
            loudness_apply_filter(-14.0, &measured).unwrap(),
            "loudnorm=I=-14:TP=-2:LRA=18.06:measured_I=-27.61:measured_TP=-4.47:measured_LRA=18.06:\
             measured_thresh=-39.20:offset=0.58:linear=true:print_format=none,aresample=48000"
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
    fn cache_names_follow_the_settings() {
        let a = CleanSettings { denoise: Some(0.85), loudness: None };
        let b = CleanSettings { denoise: Some(0.85), loudness: Some(-16.0) };
        assert_ne!(a.cache_name("f00d"), b.cache_name("f00d"));
        assert_eq!(a.cache_name("f00d"), a.cache_name("f00d"));
        assert!(a.cache_name("f00d").starts_with("f00d-") && a.cache_name("f00d").ends_with(".m4a"));
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

    #[test]
    fn denoising_lowers_the_noise_floor() {
        if !have_ffmpeg() {
            eprintln!("ffmpeg/ffprobe unavailable");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-denoise-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (video, speech) = noisy_recording(&dir);
        let output = dir.join("clean.m4a").to_string_lossy().into_owned();
        let job = CleanAudioJob {
            video_path: video.clone(),
            output_path: output.clone(),
            settings: CleanSettings { denoise: Some(0.85), loudness: None },
            duration: 12.0,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        assert_eq!(job.run(), Ok(output.clone()));
        // The noise alone (0.2-1.8 s) and the voice over it (3-8 s), before and after.
        let (noise_before, noise_after) = (rms_db(&video, 0.2, 1.8), rms_db(&output, 0.2, 1.8));
        let (voice_before, voice_after) = (rms_db(&video, 3.0, 8.0), rms_db(&output, 3.0, 8.0));
        eprintln!("noise floor {noise_before:.1} -> {noise_after:.1} dB, voice {voice_before:.1} -> {voice_after:.1} dB");
        assert!(noise_before - noise_after >= 10.0, "the noise floor fell {:.1} dB", noise_before - noise_after);
        if speech {
            assert!(voice_before - voice_after <= 3.0, "the voice fell {:.1} dB", voice_before - voice_after);
        }
        // Same length: the preview plays it on the recording's timeline.
        let probe = crate::compat::qprocess::run(
            "ffprobe", &["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", &output], 15000).unwrap();
        let duration: f64 = String::from_utf8_lossy(&probe.stdout).trim().parse().unwrap();
        assert!((duration - 12.0).abs() < 0.1, "duration {duration}");

        // Loudness on its own: the whole recording at -16 LUFS.
        let loud = dir.join("loud.m4a").to_string_lossy().into_owned();
        let job = CleanAudioJob {
            output_path: loud.clone(),
            settings: CleanSettings { denoise: speech.then_some(1.0), loudness: Some(-16.0) },
            ..job
        };
        assert_eq!(job.run(), Ok(loud.clone()));
        let lufs = integrated_lufs(&loud);
        assert!((lufs + 16.0).abs() <= 1.0, "integrated loudness {lufs}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cancelled_job_leaves_nothing() {
        if !have_ffmpeg() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-denoise-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("clean.m4a").to_string_lossy().into_owned();
        let job = CleanAudioJob {
            video_path: dir.join("missing.mp4").to_string_lossy().into_owned(),
            output_path: output.clone(),
            settings: CleanSettings { denoise: None, loudness: Some(-16.0) },
            duration: 1.0,
            cancelled: Arc::new(AtomicBool::new(true)),
        };
        assert_eq!(job.run(), Err("cancelled".into()));
        assert!(!FileInfo::new(&output).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
