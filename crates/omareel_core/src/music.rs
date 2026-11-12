//! Background music (`project.music`): where in its file the music is at an output time, how
//! loud it is there (volume, fades, ducking), and the sound levels the editor's preview ducks
//! by. Export builds the same mix with ffmpeg ([`crate::export::plan::music_filter`]); the
//! preview approximates it from the levels measured here.

use crate::compat::cpp;
use crate::compat::qprocess;
use crate::project::Music;

/// The ducking compressor, ffmpeg's `sidechaincompress` keyed on the recording's sound: above
/// the threshold (about -30 dBFS) the music drops by `ratio`, reacting within the attack and
/// recovering over the release.
pub const DUCK_THRESHOLD: f64 = 0.03;
pub const DUCK_RATIO: f64 = 6.0;
pub const DUCK_ATTACK_MS: f64 = 20.0;
pub const DUCK_RELEASE_MS: f64 = 400.0;

/// Where in the music's file an output time falls: the first pass plays from `start` to the
/// end of the file, and with `loop_` the file then starts over from its beginning. `None` once
/// a music that does not loop has ended. `file_duration` is `None` when unknown (the file then
/// never ends).
pub fn file_time(music: &Music, file_duration: Option<f64>, output_time: f64) -> Option<f64> {
    file_position(music, file_duration, output_time).map(|position| position.time)
}

/// Where the music is at an output time: the time in its file, and which pass over the file
/// that is (0 the first, from `start`; 1 the first loop from the file's beginning, and so on).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilePosition {
    pub time: f64,
    pub pass: i64,
}

/// [`file_time`], with the pass it is in.
pub fn file_position(music: &Music, file_duration: Option<f64>, output_time: f64) -> Option<FilePosition> {
    let output_time = cpp::max(0.0, output_time);
    let Some(duration) = file_duration.filter(|duration| *duration > 0.0) else {
        return Some(FilePosition { time: music.start + output_time, pass: 0 });
    };
    let start = first_pass_start(music, duration);
    let first_pass = duration - start;
    if output_time < first_pass {
        return Some(FilePosition { time: start + output_time, pass: 0 });
    }
    if !music.loop_ {
        return None;
    }
    let into_loops = output_time - cpp::max(0.0, first_pass);
    // Whole loops, then what is left of one, kept in [0, duration) against rounding.
    let mut loops = (into_loops / duration).floor();
    let mut time = into_loops - loops * duration;
    if time >= duration {
        loops += 1.0;
        time -= duration;
    }
    Some(FilePosition { time: cpp::max(0.0, time), pass: 1 + loops as i64 })
}

/// Where in a file of `duration` the first pass starts. ffmpeg opens a looping file at
/// `start` on its looped timeline (`-stream_loop -1 -ss start`), so a start past the end
/// lands that far into a later pass: 6 s into a 4 s file starts at 2 s. Without the loop the
/// start stays as it is, and a start past the end plays nothing.
pub fn first_pass_start(music: &Music, duration: f64) -> f64 {
    if music.loop_ && duration > 0.0 && music.start >= duration { music.start % duration } else { music.start }
}

/// How long the music plays in an output of `output_duration`: all of it when it loops,
/// otherwise until the file runs out.
pub fn extent(music: &Music, file_duration: Option<f64>, output_duration: f64) -> f64 {
    match file_duration.filter(|duration| *duration > 0.0) {
        Some(duration) if !music.loop_ => cpp::clamp(duration - first_pass_start(music, duration), 0.0, output_duration),
        _ => cpp::max(0.0, output_duration),
    }
}

/// The fades' lengths in an output of `output_duration`: neither longer than the output.
pub fn fade_lengths(music: &Music, output_duration: f64) -> (f64, f64) {
    let longest = cpp::max(0.0, output_duration);
    (cpp::min(music.fade_in, longest), cpp::min(music.fade_out, longest))
}

/// The fades' gain at an output time: linear ramps, as ffmpeg's `afade` makes by default.
pub fn fade_gain(music: &Music, output_duration: f64, output_time: f64) -> f64 {
    let (fade_in, fade_out) = fade_lengths(music, output_duration);
    let mut gain = 1.0;
    if fade_in > 0.0 && output_time < fade_in {
        gain *= cpp::max(0.0, output_time) / fade_in;
    }
    let left = output_duration - output_time;
    if fade_out > 0.0 && left < fade_out {
        gain *= cpp::max(0.0, left) / fade_out;
    }
    cpp::clamp(gain, 0.0, 1.0)
}

/// The ducking compressor's gain for the recording's sound at `level` (RMS, linear): 1 below
/// the threshold, then the music drops by the ratio. `sidechaincompress` measured on sine keys
/// acts as if on their peak, RMS times the square root of 2 (17.8 dB down for a key at
/// -12 dBFS RMS against 17.4 here); its soft knee, which starts ducking a little below the
/// threshold, is left out.
pub fn duck_gain(level: f64) -> f64 {
    let level = level * std::f64::consts::SQRT_2;
    if level <= DUCK_THRESHOLD {
        return 1.0;
    }
    (level / DUCK_THRESHOLD).powf(1.0 / DUCK_RATIO - 1.0)
}

/// A file's sound level over time: RMS of its first audio stream, mixed to mono, in windows of
/// `1 / rate` seconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioLevels {
    pub rate: f64,
    pub rms: Vec<f32>,
}

/// The windows per second [`measure_levels`] measures.
pub const LEVELS_RATE: f64 = 50.0;
/// The sample rate the sound is measured at: enough for speech and music's loudness.
const LEVELS_SAMPLE_RATE: usize = 8000;

impl AudioLevels {
    /// The level of the window holding `time`; 0 outside the file.
    pub fn at(&self, time: f64) -> f64 {
        if !(time >= 0.0) || self.rate <= 0.0 {
            return 0.0;
        }
        self.rms.get((time * self.rate) as usize).map_or(0.0, |level| f64::from(*level))
    }

    /// The loudest window from `from` to `to`.
    pub fn max_between(&self, from: f64, to: f64) -> f64 {
        if self.rate <= 0.0 || self.rms.is_empty() {
            return 0.0;
        }
        let first = (cpp::max(0.0, from) * self.rate) as usize;
        let last = cpp::min((cpp::max(0.0, to) * self.rate) as usize, self.rms.len() - 1);
        if first > last {
            return 0.0;
        }
        self.rms[first..=last].iter().fold(0.0_f64, |loudest, level| loudest.max(f64::from(*level)))
    }

    /// The loudest window of the file.
    pub fn peak(&self) -> f64 {
        self.rms.iter().fold(0.0_f64, |loudest, level| loudest.max(f64::from(*level)))
    }

    /// RMS windows of mono float samples at [`LEVELS_SAMPLE_RATE`].
    pub fn from_samples(samples: &[f32]) -> AudioLevels {
        let window = (LEVELS_SAMPLE_RATE as f64 / LEVELS_RATE) as usize;
        let rms = samples
            .chunks(window)
            .map(|chunk| {
                let sum: f64 = chunk.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
                (sum / chunk.len() as f64).sqrt() as f32
            })
            .collect();
        AudioLevels { rate: LEVELS_RATE, rms }
    }
}

/// The ducking gain the preview applies with the recording's sound measured as `levels`, at
/// `source_time` in that file: the compressor's gain for the loudest window of the release
/// before it to the attack after it, so the music stays down through short pauses as the
/// compressor's release keeps it. `key_gain` is the gain the recording's sound gets in the mix.
pub fn preview_duck_gain(levels: &AudioLevels, source_time: f64, speed: f64, key_gain: f64) -> f64 {
    let speed = if speed > 0.0 { speed } else { 1.0 };
    let loudest = levels.max_between(
        source_time - DUCK_RELEASE_MS / 1000.0 * speed,
        source_time + DUCK_ATTACK_MS / 1000.0 * speed,
    );
    duck_gain(loudest * key_gain)
}

/// Measures a file's sound with ffmpeg; `None` when it has none or ffmpeg failed.
pub fn measure_levels(path: &str) -> Option<AudioLevels> {
    let rate = LEVELS_SAMPLE_RATE.to_string();
    let finished = qprocess::run(
        "ffmpeg",
        &["-v", "error", "-nostdin", "-i", path, "-map", "0:a:0", "-vn", "-ac", "1", "-ar", &rate, "-f", "f32le", "-"],
        120_000,
    )?;
    if !finished.succeeded() || finished.stdout.len() < 4 {
        return None;
    }
    let samples: Vec<f32> =
        finished.stdout.chunks_exact(4).map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect();
    Some(AudioLevels::from_samples(&samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn music(start: f64, loop_: bool) -> Music {
        Music { file: "media/a.ogg".into(), start, loop_, ..Music::default() }
    }

    #[test]
    fn file_time_starts_at_the_offset_then_loops_the_whole_file() {
        let m = music(1.0, true);
        assert_eq!(file_time(&m, Some(4.0), 0.0), Some(1.0));
        assert_eq!(file_time(&m, Some(4.0), 2.5), Some(3.5));
        assert_eq!(file_time(&m, Some(4.0), 3.0), Some(0.0));
        assert_eq!(file_time(&m, Some(4.0), 8.5), Some(1.5));
        let once = music(1.0, false);
        assert_eq!(file_time(&once, Some(4.0), 2.9), Some(3.9));
        assert_eq!(file_time(&once, Some(4.0), 3.0), None);
        assert_eq!(file_time(&once, None, 30.0), Some(31.0));
        // A start past the end of a file that loops starts that far into the loops, as
        // ffmpeg's `-stream_loop -1 -ss 6` does: 6 s into a 4 s file is 2 s.
        assert_eq!(file_time(&music(6.0, true), Some(4.0), 0.0), Some(2.0));
        assert_eq!(file_time(&music(6.0, true), Some(4.0), 1.0), Some(3.0));
        assert_eq!(file_time(&music(6.0, true), Some(4.0), 2.5), Some(0.5));
        assert_eq!(file_time(&music(10.0, true), Some(4.0), 0.0), Some(2.0));
        assert_eq!(file_time(&music(4.0, true), Some(4.0), 1.0), Some(1.0));
        // Without the loop it plays nothing.
        assert_eq!(file_time(&music(6.0, false), Some(4.0), 1.0), None);
        assert_eq!(extent(&music(6.0, false), Some(4.0), 10.0), 0.0);
    }

    #[test]
    fn file_position_counts_the_passes() {
        let at = |m: &Music, d: f64, t: f64| file_position(m, Some(d), t).unwrap();
        let m = music(1.0, true);
        assert_eq!(at(&m, 4.0, 2.9), FilePosition { time: 3.9, pass: 0 });
        assert_eq!(at(&m, 4.0, 3.0), FilePosition { time: 0.0, pass: 1 });
        assert_eq!(at(&m, 4.0, 6.9).pass, 1);
        assert_eq!(at(&m, 4.0, 7.0), FilePosition { time: 0.0, pass: 2 });
        // A file shorter than the preview's 250 ms tolerance still moves on a pass every loop.
        let short = music(0.0, true);
        assert_eq!(at(&short, 0.03, 0.01).pass, 0);
        assert_eq!(at(&short, 0.03, 0.04).pass, 1);
        assert_eq!(at(&short, 0.03, 0.07).pass, 2);
        assert!((at(&short, 0.03, 0.07).time - 0.01).abs() < 1e-9);
        // At a loop point the pass and the time agree: the next pass, at its start.
        for step in 1..200 {
            let position = at(&music(0.0, true), 0.15, f64::from(step) * 0.15);
            assert!(position.time < 0.15 && (position.time < 1e-9 || position.time > 0.15 - 1e-9), "{step}: {position:?}");
            let expected = if position.time < 1e-9 { step } else { step - 1 };
            assert_eq!(position.pass, i64::from(expected), "{step}: {position:?}");
        }
    }

    #[test]
    fn extent_and_fades() {
        assert_eq!(extent(&music(1.0, false), Some(4.0), 10.0), 3.0);
        assert_eq!(extent(&music(1.0, true), Some(4.0), 10.0), 10.0);
        assert_eq!(extent(&music(0.0, false), Some(40.0), 10.0), 10.0);
        let m = Music { fade_in: 2.0, fade_out: 4.0, ..music(0.0, true) };
        assert_eq!(fade_gain(&m, 10.0, 0.0), 0.0);
        assert_eq!(fade_gain(&m, 10.0, 1.0), 0.5);
        assert_eq!(fade_gain(&m, 10.0, 5.0), 1.0);
        assert_eq!(fade_gain(&m, 10.0, 8.0), 0.5);
        assert_eq!(fade_gain(&m, 10.0, 10.0), 0.0);
        // Fades longer than the output are cut to it.
        assert_eq!(fade_lengths(&m, 3.0), (2.0, 3.0));
    }

    #[test]
    fn ducking_follows_the_compressor_curve() {
        assert_eq!(duck_gain(0.0), 1.0);
        assert_eq!(duck_gain(DUCK_THRESHOLD / std::f64::consts::SQRT_2), 1.0);
        let gain = duck_gain(DUCK_THRESHOLD * 10.0 / std::f64::consts::SQRT_2);
        // 20 dB over the threshold at 6:1 is about 16.7 dB down.
        assert!((20.0 * gain.log10() + 16.67).abs() < 0.1, "{gain}");
        let levels = AudioLevels { rate: 10.0, rms: vec![0.0, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0] };
        assert_eq!(preview_duck_gain(&levels, 0.05, 1.0, 1.0), 1.0);
        // Held down through the release after the loud window, then back up.
        assert!(preview_duck_gain(&levels, 0.45, 1.0, 1.0) < 0.2);
        assert_eq!(preview_duck_gain(&levels, 0.75, 1.0, 1.0), 1.0);
        // A quieter mix ducks less.
        assert!(preview_duck_gain(&levels, 0.25, 1.0, 0.1) > preview_duck_gain(&levels, 0.25, 1.0, 1.0));
    }

    #[test]
    fn levels_are_rms_windows() {
        let mut samples = vec![0.5_f32; 160];
        samples.extend(vec![0.0_f32; 160]);
        let levels = AudioLevels::from_samples(&samples);
        assert_eq!(levels.rms, vec![0.5, 0.0]);
        assert_eq!(levels.at(0.01), 0.5);
        assert_eq!(levels.at(0.03), 0.0);
        assert_eq!(levels.at(1.0), 0.0);
        assert_eq!(levels.peak(), 0.5);
    }
}
