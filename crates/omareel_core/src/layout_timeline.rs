//! What each clip's layout shows over output time (`LayoutTimeline`).

use crate::compat::cpp;
use crate::easing::ease_in_out_cubic;
use crate::project::Clip;

/// What a clip's layout shows at an output time. `camera_full` is how far the webcam has grown
/// from its bubble to full frame; `bubble` is how visible the bubble is while the screen shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutSample {
    pub camera_full: f64,
    pub bubble: f64,
}

impl Default for LayoutSample {
    fn default() -> LayoutSample {
        LayoutSample { camera_full: 0.0, bubble: 1.0 }
    }
}

/// The `transition` the C++ `sampleLayout` takes when none is given; every caller uses it.
pub const DEFAULT_TRANSITION: f64 = 0.5;

fn target(clip: &Clip, camera_available: bool, bubble_enabled: bool) -> LayoutSample {
    if !camera_available || clip.is_media() {
        return LayoutSample { camera_full: 0.0, bubble: 0.0 };
    }
    if clip.layout == "camera" {
        return LayoutSample { camera_full: 1.0, bubble: 0.0 };
    }
    if clip.layout == "screen" {
        return LayoutSample { camera_full: 0.0, bubble: 0.0 };
    }
    LayoutSample { camera_full: 0.0, bubble: if bubble_enabled { 1.0 } else { 0.0 } }
}

/// Layouts change at clip boundaries. The new clip eases from the previous clip's layout to its
/// own over its first `transition` seconds of output (less for shorter clips), so every clip ends
/// on its own layout. Output time, not source time, keeps transitions intact across cuts and
/// speed changes. Inserted videos show no camera and cut in and out.
pub fn sample_layout(
    clips: &[Clip],
    output_time: f64,
    camera_available: bool,
    bubble_enabled: bool,
    transition: f64,
) -> LayoutSample {
    let Some(first) = clips.first() else { return target(&Clip::default(), camera_available, bubble_enabled) };
    let mut start = 0.0;
    let mut previous = target(first, camera_available, bubble_enabled);
    for (i, clip) in clips.iter().enumerate() {
        let length = cpp::max(0.0, (clip.out - clip.in_) / cpp::max(0.000001, clip.speed));
        let next = target(clip, camera_available, bubble_enabled);
        if output_time < start + length || i == clips.len() - 1 {
            let span = cpp::min(transition, length);
            // Inserted videos cut in and out; only recording clips morph between layouts.
            if i == 0 || span <= 0.0 || clip.is_media() || clips[i - 1].is_media() {
                return next;
            }
            let progress = ease_in_out_cubic((output_time - start) / span);
            return LayoutSample {
                camera_full: previous.camera_full + (next.camera_full - previous.camera_full) * progress,
                bubble: previous.bubble + (next.bubble - previous.bubble) * progress,
            };
        }
        previous = next;
        start += length;
    }
    previous
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qround::{q_fuzzy_compare, q_fuzzy_is_null};

    /// `QCOMPARE` on doubles: fuzzy, and an expected (fuzzy) zero needs an actual one.
    fn qcompare(actual: f64, expected: f64) -> bool {
        if expected.is_nan() {
            actual.is_nan()
        } else if expected.is_infinite() {
            actual == expected
        } else if q_fuzzy_is_null(expected) {
            q_fuzzy_is_null(actual)
        } else {
            q_fuzzy_compare(actual, expected)
        }
    }

    fn clip(id: &str, in_: f64, out: f64, layout: &str) -> Clip {
        clip_at_speed(id, in_, out, layout, 1.0)
    }

    fn clip_at_speed(id: &str, in_: f64, out: f64, layout: &str, speed: f64) -> Clip {
        Clip { id: id.into(), in_, out, speed, layout: layout.into(), ..Clip::default() }
    }

    fn sample(clips: &[Clip], output_time: f64, camera_available: bool, bubble_enabled: bool) -> LayoutSample {
        sample_layout(clips, output_time, camera_available, bubble_enabled, DEFAULT_TRANSITION)
    }

    // tests/test_layouttimeline.cpp
    #[test]
    fn each_layout_has_its_target() {
        let clips = [clip("a", 0.0, 2.0, "camera")];
        assert!(qcompare(sample(&clips, 1.0, true, true).camera_full, 1.0));
        assert!(qcompare(sample(&clips, 1.0, true, true).bubble, 0.0));
        let screen = [clip("a", 0.0, 2.0, "screen")];
        assert!(qcompare(sample(&screen, 1.0, true, true).camera_full, 0.0));
        assert!(qcompare(sample(&screen, 1.0, true, true).bubble, 0.0));
        let standard = [clip("a", 0.0, 2.0, "default")];
        assert!(qcompare(sample(&standard, 1.0, true, true).bubble, 1.0));
        assert!(qcompare(sample(&standard, 1.0, true, false).bubble, 0.0));
    }

    #[test]
    fn without_camera_every_layout_shows_the_screen() {
        let clips = [clip("a", 0.0, 2.0, "camera")];
        assert!(qcompare(sample(&clips, 1.0, false, true).camera_full, 0.0));
        assert!(qcompare(sample(&clips, 1.0, false, true).bubble, 0.0));
    }

    #[test]
    fn first_clip_starts_on_its_layout() {
        let clips = [clip("intro", 0.0, 2.0, "camera"), clip("main", 2.0, 6.0, "default")];
        assert!(qcompare(sample(&clips, 0.0, true, true).camera_full, 1.0));
    }

    #[test]
    fn next_clip_eases_from_the_previous_layout() {
        let clips = [clip("intro", 0.0, 2.0, "camera"), clip("main", 2.0, 6.0, "default")];
        assert!(qcompare(sample(&clips, 1.999, true, true).camera_full, 1.0));
        assert!(qcompare(sample(&clips, 2.0, true, true).camera_full, 1.0));
        let middle = sample(&clips, 2.25, true, true);
        assert!((middle.camera_full - 0.5).abs() < 1e-9);
        assert!((middle.bubble - 0.5).abs() < 1e-9);
        assert!(qcompare(sample(&clips, 2.5, true, true).camera_full, 0.0));
        assert!(qcompare(sample(&clips, 2.5, true, true).bubble, 1.0));
        let early = sample(&clips, 2.05, true, true).camera_full;
        let late = sample(&clips, 2.45, true, true).camera_full;
        assert!(early > 0.9 && early < 1.0);
        assert!(late > 0.0 && late < 0.1);
    }

    #[test]
    fn transition_uses_output_time_at_any_speed() {
        // The second clip plays 4 s of source at 2x: 2 s of output, transition still 0.5 s.
        let clips = [clip("intro", 0.0, 1.0, "default"), clip_at_speed("outro", 1.0, 5.0, "camera", 2.0)];
        assert!((sample(&clips, 1.25, true, true).camera_full - 0.5).abs() < 1e-9);
        assert!(qcompare(sample(&clips, 1.5, true, true).camera_full, 1.0));
    }

    #[test]
    fn short_clips_still_end_on_their_layout() {
        let clips = [clip("a", 0.0, 1.0, "camera"), clip("b", 1.0, 1.2, "screen"), clip("c", 1.2, 3.0, "camera")];
        assert!(qcompare(sample(&clips, 1.1, true, true).camera_full, 0.5));
        assert!(sample(&clips, 1.1999, true, true).camera_full < 1e-6);
        assert!(qcompare(sample(&clips, 1.2, true, true).camera_full, 0.0));
        assert!((sample(&clips, 1.45, true, true).camera_full - 0.5).abs() < 1e-9);
    }

    #[test]
    fn bubble_fades_when_the_screen_loses_it() {
        let clips = [clip("a", 0.0, 1.0, "default"), clip("b", 1.0, 3.0, "screen")];
        assert!((sample(&clips, 1.25, true, true).bubble - 0.5).abs() < 1e-9);
        assert!(qcompare(sample(&clips, 1.25, true, true).camera_full, 0.0));
        assert!(qcompare(sample(&clips, 2.0, true, true).bubble, 0.0));
    }

    #[test]
    fn inserted_videos_cut_in_and_out() {
        let mut insert = clip("m", 0.0, 1.0, "default");
        insert.media = "media/demo.mp4".into();
        let clips = [clip("a", 0.0, 1.0, "camera"), insert, clip("b", 1.0, 3.0, "default")];
        // No camera during the insert, and the clip after it starts on its own layout.
        assert!(qcompare(sample(&clips, 1.1, true, true).camera_full, 0.0));
        assert!(qcompare(sample(&clips, 1.1, true, true).bubble, 0.0));
        assert!(qcompare(sample(&clips, 2.0, true, true).bubble, 1.0));
        assert!(qcompare(sample(&clips, 2.0, true, true).camera_full, 0.0));
    }

    #[test]
    fn past_the_end_holds_the_last_layout() {
        let clips = [clip("a", 0.0, 1.0, "default"), clip("b", 1.0, 3.0, "camera")];
        assert!(qcompare(sample(&clips, 10.0, true, true).camera_full, 1.0));
        assert!(qcompare(sample(&[], 1.0, true, true).bubble, 1.0));
    }
}
