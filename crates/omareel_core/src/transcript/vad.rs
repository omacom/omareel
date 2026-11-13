//! Finding speech by loudness: where the recording's sound rises well above its own quiet.
//! Deterministic, needs no model, and works without any speech engine, so pause trimming works
//! for everyone. Speech engines that give text only are timed by it, phrase by phrase.
//!
//! The level is measured in 20 ms frames. The threshold sits between the recording's quiet (its
//! 10th percentile) and its loud (95th percentile), never below -50 dBFS; frames above it are
//! speech, gaps shorter than [`VadSettings::bridge`] are joined, and blips shorter than
//! [`VadSettings::min_speech`] are dropped.

use super::audio::SAMPLE_RATE;

/// One level per 20 ms.
pub const FRAME_SECONDS: f64 = 0.02;
const FRAME: usize = (SAMPLE_RATE as usize) / 50;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VadSettings {
    /// Gaps in speech shorter than this are part of it (seconds).
    pub bridge: f64,
    /// Speech shorter than this is noise (seconds).
    pub min_speech: f64,
    /// Added to both ends of each span of speech (seconds), for the soft starts and ends of
    /// words the threshold misses.
    pub pad: f64,
    /// The lowest threshold, dBFS.
    pub floor_db: f64,
}

impl Default for VadSettings {
    fn default() -> VadSettings {
        VadSettings { bridge: 0.3, min_speech: 0.12, pad: 0.06, floor_db: -50.0 }
    }
}

/// The level of each 20 ms frame in dBFS (-100 for silence).
pub fn frame_levels(samples: &[i16]) -> Vec<f64> {
    samples
        .chunks(FRAME)
        .map(|frame| {
            let energy: f64 = frame.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum::<f64>() / frame.len().max(1) as f64;
            if energy <= 1e-10 { -100.0 } else { (10.0 * energy.log10()).max(-100.0) }
        })
        .collect()
}

fn percentile(levels: &[f64], fraction: f64) -> f64 {
    let mut sorted = levels.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() - 1) as f64 * fraction).round() as usize]
}

/// The level above which a frame is speech, or `None` when the sound never rises clearly above
/// its quiet (silence, or a steady noise).
pub fn threshold(levels: &[f64], settings: &VadSettings) -> Option<f64> {
    if levels.is_empty() {
        return None;
    }
    let quiet = percentile(levels, 0.10);
    let loud = percentile(levels, 0.95);
    if loud < settings.floor_db || loud - quiet < 10.0 {
        return None;
    }
    Some((quiet + ((loud - quiet) * 0.35).clamp(8.0, 25.0)).max(settings.floor_db))
}

/// Spans of speech in seconds, sorted and apart.
pub fn speech_regions(samples: &[i16], settings: &VadSettings) -> Vec<(f64, f64)> {
    let levels = frame_levels(samples);
    let duration = samples.len() as f64 / SAMPLE_RATE as f64;
    speech_regions_from_levels(&levels, duration, settings)
}

pub fn speech_regions_from_levels(levels: &[f64], duration: f64, settings: &VadSettings) -> Vec<(f64, f64)> {
    let Some(threshold) = threshold(levels, settings) else { return Vec::new() };
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &level) in levels.iter().enumerate() {
        match (level > threshold, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, levels.len()));
    }
    let bridge = (settings.bridge / FRAME_SECONDS).round() as usize;
    let mut joined: Vec<(usize, usize)> = Vec::new();
    for run in runs {
        match joined.last_mut() {
            Some(last) if run.0 - last.1 < bridge => last.1 = run.1,
            _ => joined.push(run),
        }
    }
    let min_frames = (settings.min_speech / FRAME_SECONDS).round() as usize;
    let mut regions: Vec<(f64, f64)> = Vec::new();
    for (from, to) in joined.into_iter().filter(|(from, to)| to - from >= min_frames.max(1)) {
        let start = (from as f64 * FRAME_SECONDS - settings.pad).max(0.0);
        let end = (to as f64 * FRAME_SECONDS + settings.pad).min(duration);
        match regions.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => regions.push((start, end)),
        }
    }
    regions
}

/// Pieces to hand a speech engine: spans of speech joined while the silence between them is
/// under `join_gap` and the piece stays under `max_length`; a single longer span is cut at its
/// quietest frame. Each piece is `(start, end, speech spans inside it)`.
pub fn transcription_chunks(
    regions: &[(f64, f64)],
    levels: &[f64],
    join_gap: f64,
    max_length: f64,
) -> Vec<(f64, f64, Vec<(f64, f64)>)> {
    // Cut over-long spans first.
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for &(start, end) in regions {
        let mut start = start;
        while end - start > max_length {
            // The quietest frame in the last third of the allowed length.
            let from = ((start + max_length * 2.0 / 3.0) / FRAME_SECONDS) as usize;
            let to = (((start + max_length) / FRAME_SECONDS) as usize).min(levels.len());
            let cut = (from..to)
                .min_by(|&a, &b| levels[a].total_cmp(&levels[b]).then(b.cmp(&a)))
                .map_or(start + max_length, |frame| frame as f64 * FRAME_SECONDS);
            let cut = if cut <= start { start + max_length } else { cut };
            spans.push((start, cut));
            start = cut;
        }
        spans.push((start, end));
    }
    let mut chunks: Vec<(f64, f64, Vec<(f64, f64)>)> = Vec::new();
    for span in spans {
        match chunks.last_mut() {
            Some(last) if span.0 - last.1 < join_gap && span.1 - last.0 <= max_length => {
                last.1 = span.1;
                last.2.push(span);
            }
            _ => chunks.push((span.0, span.1, vec![span])),
        }
    }
    chunks
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Silence with 440 Hz tone bursts at the given spans (seconds), plus a little noise so the
    /// quiet is not digital silence.
    pub(crate) fn bursts(duration: f64, spans: &[(f64, f64)], amplitude: f64) -> Vec<i16> {
        let n = (duration * SAMPLE_RATE as f64) as usize;
        let mut seed: u32 = 12345;
        (0..n)
            .map(|i| {
                let t = i as f64 / SAMPLE_RATE as f64;
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let noise = ((seed >> 16) as f64 / 65536.0 - 0.5) * 60.0;
                let tone = if spans.iter().any(|&(a, b)| t >= a && t < b) {
                    (t * 440.0 * std::f64::consts::TAU).sin() * amplitude * 32767.0
                } else {
                    0.0
                };
                (tone + noise) as i16
            })
            .collect()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.1
    }

    #[test]
    fn finds_bursts_with_gaps() {
        let samples = bursts(6.0, &[(0.5, 1.5), (2.5, 3.0), (4.0, 5.5)], 0.3);
        let regions = speech_regions(&samples, &VadSettings::default());
        assert_eq!(regions.len(), 3, "{regions:?}");
        for (found, expected) in regions.iter().zip([(0.5, 1.5), (2.5, 3.0), (4.0, 5.5)]) {
            assert!(close(found.0, expected.0) && close(found.1, expected.1), "{found:?} vs {expected:?}");
        }
        // Deterministic.
        assert_eq!(regions, speech_regions(&samples, &VadSettings::default()));
    }

    #[test]
    fn short_gaps_are_bridged_and_blips_dropped() {
        let samples = bursts(4.0, &[(0.5, 1.0), (1.15, 2.0), (3.0, 3.05)], 0.3);
        let regions = speech_regions(&samples, &VadSettings::default());
        assert_eq!(regions.len(), 1, "{regions:?}");
        assert!(close(regions[0].0, 0.5) && close(regions[0].1, 2.0));
    }

    #[test]
    fn silence_and_steady_noise_have_no_speech() {
        assert!(speech_regions(&vec![0; 32_000], &VadSettings::default()).is_empty());
        assert!(speech_regions(&bursts(2.0, &[], 0.0), &VadSettings::default()).is_empty());
        assert!(speech_regions(&bursts(2.0, &[(0.0, 2.0)], 0.3), &VadSettings::default()).is_empty());
    }

    #[test]
    fn quiet_speech_counts() {
        let samples = bursts(3.0, &[(1.0, 2.0)], 0.02);
        let regions = speech_regions(&samples, &VadSettings::default());
        assert_eq!(regions.len(), 1, "{regions:?}");
    }

    #[test]
    fn chunks_join_close_spans_and_cut_long_ones() {
        let levels = vec![-20.0; 3000]; // 60 s
        let chunks = transcription_chunks(&[(0.0, 2.0), (2.3, 4.0), (6.0, 7.0)], &levels, 0.5, 20.0);
        assert_eq!(chunks.len(), 2);
        assert_eq!((chunks[0].0, chunks[0].1, chunks[0].2.len()), (0.0, 4.0, 2));
        assert_eq!((chunks[1].0, chunks[1].1), (6.0, 7.0));

        let mut levels = vec![-20.0; 3000];
        levels[800] = -60.0; // 16 s
        let chunks = transcription_chunks(&[(0.0, 50.0)], &levels, 0.5, 20.0);
        assert!(chunks.iter().all(|c| c.1 - c.0 <= 20.0 + 1e-9), "{chunks:?}");
        assert!((chunks[0].1 - 16.0).abs() < 1e-9);
        assert_eq!(chunks.last().unwrap().1, 50.0);
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
    }
}
