//! "Speed up quiet parts": the stretches pause trimming leaves alone because the screen changes
//! (an installer running, a build, scrolling, silent typing) play faster instead of being cut.
//! These functions only say which source ranges and how fast;
//! `editor::Document::speed_up_source_ranges` splits them out of the clips as one undo step.

use super::trim::merge_ranges;
use crate::project::allowed_clip_speeds;

/// How quiet parts are found and how fast they play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedUpSettings {
    /// Shorter stretches stay as they are (seconds, after the margins).
    pub min_length: f64,
    /// Seconds next to speech that keep their speed, so a sentence is never sped up.
    pub margin: f64,
    /// The length (seconds) a stretch should play in; the speed is chosen to come close.
    pub target: f64,
}

impl Default for SpeedUpSettings {
    fn default() -> SpeedUpSettings {
        SpeedUpSettings { min_length: 3.0, margin: 0.3, target: 2.0 }
    }
}

/// A source range to play faster.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuietPart {
    pub start: f64,
    pub end: f64,
    pub speed: f64,
}

impl QuietPart {
    /// Output seconds the speed-up saves.
    pub fn saved(&self) -> f64 {
        let length = self.end - self.start;
        length - length / self.speed
    }
}

/// The speeds a quiet part may get: the clip speeds offered, from 2x up.
pub fn speeds() -> Vec<f64> {
    allowed_clip_speeds().into_iter().filter(|&speed| speed >= 2.0).collect()
}

/// The speed that plays `length` seconds closest to `target` seconds (closest by ratio, so 4.5 s
/// for a 2 s target is as far off as 0.89 s), at least 2x.
pub fn choose_speed(length: f64, target: f64) -> f64 {
    let speeds = speeds();
    let target = if target > 0.0 { target } else { SpeedUpSettings::default().target };
    let off = |speed: f64| (length / speed / target).ln().abs();
    speeds.iter().copied().fold(speeds[0], |best, speed| if off(speed) < off(best) - 1e-9 { speed } else { best })
}

/// The quiet stretches of a recording of `duration` seconds: the silences the screen does not
/// stay still through (`moving`, as `stillness::check_windows` found them), less `margin` next
/// to speech (none at the recording's start and end), at least `min_length` long.
pub fn quiet_ranges(moving: &[(f64, f64)], duration: f64, settings: &SpeedUpSettings) -> Vec<(f64, f64)> {
    let mut ranges = Vec::new();
    for &(start, end) in &merge_ranges(moving) {
        let from = if start <= 1e-6 { start } else { start + settings.margin };
        let to = if end >= duration - 1e-6 { end } else { end - settings.margin };
        if to - from >= settings.min_length {
            ranges.push((from, to));
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_bring_stretches_near_the_target() {
        assert_eq!(speeds(), vec![2.0, 3.0, 4.0, 8.0, 16.0, 24.0]);
        // 3 s at 2x is 1.5 s; never slower than 2x.
        assert_eq!(choose_speed(3.0, 2.0), 2.0);
        assert_eq!(choose_speed(1.0, 2.0), 2.0);
        assert_eq!(choose_speed(6.0, 2.0), 3.0);
        assert_eq!(choose_speed(8.5, 2.0), 4.0);
        assert_eq!(choose_speed(16.0, 2.0), 8.0);
        assert_eq!(choose_speed(30.0, 2.0), 16.0);
        // Long installs go as fast as allowed.
        assert_eq!(choose_speed(600.0, 2.0), 24.0);
        // A longer target slows them down.
        assert_eq!(choose_speed(16.0, 4.0), 4.0);
        let part = QuietPart { start: 10.0, end: 18.0, speed: 4.0 };
        assert_eq!(part.saved(), 6.0);
    }

    #[test]
    fn quiet_ranges_keep_margins_next_to_speech() {
        let settings = SpeedUpSettings::default();
        // Moving silences: the start of the recording, a short one, a long one, the tail.
        let moving = [(0.0, 4.0), (6.0, 8.0), (10.0, 20.0), (30.0, 40.0)];
        assert_eq!(quiet_ranges(&moving, 40.0, &settings), vec![(0.0, 3.7), (10.3, 19.7), (30.3, 40.0)]);
        // 3.5 s less 0.6 s of margins is too short.
        assert!(quiet_ranges(&[(5.0, 8.5)], 40.0, &settings).is_empty());
        assert!(quiet_ranges(&[], 40.0, &settings).is_empty());
    }
}
