//! Where the webcam recording is at a screen time (`CameraTimeline`).

use crate::compat::cpp;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraTime {
    pub seconds: f64,
    pub before_start: bool,
    pub beyond_end: bool,
}

/// The camera time at `screen_time` when the camera started `camera_offset` seconds after the
/// screen, held at its first and last frames.
pub fn map_camera_time(screen_time: f64, camera_offset: f64, camera_duration: f64) -> CameraTime {
    let raw = screen_time - camera_offset;
    CameraTime {
        seconds: cpp::clamp(raw, 0.0, cpp::max(0.0, camera_duration)),
        before_start: raw < 0.0,
        beyond_end: raw > camera_duration,
    }
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

    // tests/test_cameratimeline.cpp
    #[test]
    fn maps_offset_and_holds_ends() {
        let delayed = map_camera_time(1.0, 0.4, 2.0);
        assert!(qcompare(delayed.seconds, 0.6));
        assert!(!delayed.before_start);
        assert!(!delayed.beyond_end);

        let before = map_camera_time(0.1, 0.4, 2.0);
        assert!(qcompare(before.seconds, 0.0));
        assert!(before.before_start);

        let early_camera = map_camera_time(0.25, -0.5, 2.0);
        assert!(qcompare(early_camera.seconds, 0.75));

        let held = map_camera_time(4.0, 0.4, 2.0);
        assert!(qcompare(held.seconds, 2.0));
        assert!(held.beyond_end);
    }
}
