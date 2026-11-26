//! Transcribes synthesised speech with whichever real engines are installed (voxtype,
//! whisper-cli). Skips, passing, without espeak-ng, ffmpeg, or an engine.

use omareel_core::transcript::audio::{self, Audio};
use omareel_core::transcript::backend::{engine, transcribe_audio, BackendKind, Control};
use omareel_core::transcript::trim::{pause_ranges, PauseSettings};
use omareel_core::transcript::vad::{speech_regions, VadSettings};
use std::process::Command;

fn have(program: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {program}")]).output().is_ok_and(|o| o.status.success())
}

fn speech(text: &str, dir: &std::path::Path, name: &str) -> Audio {
    let path = dir.join(name);
    let status = Command::new("espeak-ng").args(["-s", "150", "-w"]).arg(&path).arg(text).status().unwrap();
    assert!(status.success());
    audio::extract(&path.to_string_lossy()).unwrap()
}

#[test]
fn transcribes_speech_with_installed_engines() {
    if !have("espeak-ng") || !have("ffmpeg") {
        eprintln!("skipped: espeak-ng or ffmpeg missing");
        return;
    }
    let engines: Vec<_> = [BackendKind::WhisperCpp, BackendKind::Voxtype].into_iter().filter_map(engine).collect();
    if engines.is_empty() {
        eprintln!("skipped: no whisper-cli (with a model) or voxtype");
        return;
    }
    let dir = std::env::temp_dir().join(format!("omareel-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let first = speech("Hello there. This is a test of the transcription.", &dir, "a.wav");
    let second = speech("The second sentence comes after a long pause.", &dir, "b.wav");
    let _ = std::fs::remove_dir_all(&dir);
    let gap = 3.0;
    let mut samples = vec![0i16; 8_000];
    samples.extend(&first.samples);
    let gap_start = samples.len() as f64 / 16_000.0;
    samples.extend(vec![0i16; (gap * 16_000.0) as usize]);
    samples.extend(&second.samples);
    samples.extend(vec![0i16; 8_000]);
    let audio = Audio { samples };

    let regions = speech_regions(&audio.samples, &VadSettings::default());
    let pauses = pause_ranges(&regions, audio.duration(), &[], &[(0.0, audio.duration())], &PauseSettings::default());
    assert!(
        pauses.iter().any(|&(a, b)| a < gap_start + 0.5 && b > gap_start + gap - 0.5 && b - a > gap - 1.0),
        "no pause found in {pauses:?} (speech {regions:?})"
    );

    for engine in engines {
        let transcript = transcribe_audio(&audio, &[engine.clone()], &Control::default())
            .unwrap_or_else(|error| panic!("{}: {error}", engine.kind.name()));
        eprintln!("{} ({}, {:?}): {:?}", transcript.backend, transcript.model, transcript.word_timing, transcript.text());
        let text = transcript.text().to_lowercase();
        assert!(text.contains("second") && text.contains("test"), "{text}");
        let words: Vec<_> = transcript.words().collect();
        assert!(words.len() >= 10);
        for pair in words.windows(2) {
            assert!(pair[0].start <= pair[1].start + 1e-6, "{pair:?}");
        }
        // "second" is spoken after the pause.
        let second = words.iter().find(|w| w.text.to_lowercase().starts_with("second")).unwrap();
        assert!(second.start > gap_start + gap - 0.3, "{second:?}");
        let hello = words.iter().find(|w| w.text.to_lowercase().starts_with("hello")).unwrap();
        assert!(hello.start < gap_start, "{hello:?}");
    }
}

/// One sentence in a long, otherwise quiet recording (a silent demo with one comment) is still
/// found and transcribed: the speech fills under 5% of it.
#[test]
fn transcribes_one_sentence_in_a_long_recording() {
    if !have("espeak-ng") || !have("ffmpeg") {
        eprintln!("skipped: espeak-ng or ffmpeg missing");
        return;
    }
    let Some(engine) = [BackendKind::WhisperCpp, BackendKind::Voxtype].into_iter().find_map(engine) else {
        eprintln!("skipped: no whisper-cli (with a model) or voxtype");
        return;
    };
    let dir = std::env::temp_dir().join(format!("omareel-e2e-sparse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sentence = speech("Here is the settings page.", &dir, "s.wav");
    let _ = std::fs::remove_dir_all(&dir);
    // 90 s of faint room noise with the sentence at 20 s.
    let mut seed: u32 = 7;
    let mut samples: Vec<i16> = (0..90 * 16_000)
        .map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (((seed >> 16) as f64 / 65536.0 - 0.5) * 80.0) as i16
        })
        .collect();
    let at = 20 * 16_000;
    for (i, &s) in sentence.samples.iter().enumerate() {
        samples[at + i] = samples[at + i].saturating_add(s);
    }
    let audio = Audio { samples };
    let regions = speech_regions(&audio.samples, &VadSettings::default());
    assert_eq!(regions.len(), 1, "{regions:?}");
    let transcript = transcribe_audio(&audio, &[engine.clone()], &Control::default()).unwrap();
    eprintln!("{} ({}): {:?}", transcript.backend, transcript.model, transcript.text());
    assert!(transcript.text().to_lowercase().contains("settings"), "{:?}", transcript.text());
}
