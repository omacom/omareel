//! cameratimeline: `camera_timeline::map_camera_time` against `Omareel::mapCameraTime` (README
//! "cameratimeline").

use crate::support::*;
use omareel_core::camera_timeline::map_camera_time;

#[test]
fn map_txt() {
    let mut out = Text::new();
    for offset in [-2.0, -0.5, 0.0, 0.25, 1.5, 3.0, f64::NAN] {
        for duration in [-1.0, 0.0, 0.5, 4.0, 8.0, f64::INFINITY, f64::NAN] {
            out.section(&format!("offset={} duration={}", num(offset), num(duration)));
            let mut times = uniform(4, -12, 48);
            for edge in [offset, offset + duration] {
                if edge.is_finite() {
                    times.extend([before(edge), edge, after(edge)]);
                }
            }
            let mut times = sorted_unique(times);
            times.extend([-0.0, f64::NEG_INFINITY, f64::INFINITY, f64::NAN]);
            for t in times {
                let mapped = map_camera_time(t, offset, duration);
                out.row([
                    num(t),
                    num(mapped.seconds),
                    boolean(mapped.before_start).into(),
                    boolean(mapped.beyond_end).into(),
                ]);
            }
        }
    }
    assert_golden("cameratimeline/map.txt", &out.into_string());
}
