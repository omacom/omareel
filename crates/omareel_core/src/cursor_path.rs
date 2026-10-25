//! Cursor samples: thinning and interpolation (`CursorPath`).

use crate::compat::cpp;
use crate::compat::geometry::PointF;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorSample {
    pub time: f64,
    pub position: PointF,
}

/// Drops samples closer than one frame window (1/fps) or less than `min_distance` from the
/// last kept sample. `min_distance` is in the same units as the sample positions: pass 1.0
/// for pixel coordinates, or 1/max(width,height) for normalized ones.
pub fn decimate(raw: &[CursorSample], fps: f64, min_distance: f64) -> Vec<CursorSample> {
    let Some(&first) = raw.first() else { return Vec::new() };
    let mut result = vec![first];
    let window = 1.0 / cpp::max(1.0, fps);
    for sample in &raw[1..] {
        let last = result[result.len() - 1];
        let delta = sample.position - last.position;
        if sample.time - last.time < window || delta.x.hypot(delta.y) < min_distance {
            continue;
        }
        result.push(*sample);
    }
    let end = raw[raw.len() - 1];
    if end.time > result[result.len() - 1].time {
        result.push(end);
    }
    result
}

/// The interpolated position at `time`; the centre when there are no samples.
pub fn position_at(samples: &[CursorSample], time: f64) -> PointF {
    if samples.is_empty() {
        return PointF::new(0.5, 0.5);
    }
    // std::upper_bound: the first sample whose time is greater than `time`, with its NaN
    // behaviour, hence the negated comparison.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    let upper = samples.partition_point(|sample| !(time < sample.time));
    if upper == 0 {
        return samples[0].position;
    }
    if upper == samples.len() {
        return samples[samples.len() - 1].position;
    }
    let (a, b) = (samples[upper - 1], samples[upper]);
    let alpha = if b.time > a.time { cpp::clamp((time - a.time) / (b.time - a.time), 0.0, 1.0) } else { 0.0 };
    a.position + (b.position - a.position) * alpha
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(time: f64, x: f64, y: f64) -> CursorSample {
        CursorSample { time, position: PointF::new(x, y) }
    }

    // tests/test_cursorpath.cpp
    #[test]
    fn decimates_and_interpolates() {
        let raw = [s(0.0, 0.0, 0.0), s(0.005, 10.0, 0.0), s(0.020, 0.5, 0.0), s(0.040, 20.0, 10.0)];
        assert_eq!(decimate(&raw, 60.0, 1.0).len(), 2);
        assert_eq!(position_at(&[s(0.0, 0.0, 0.0), s(1.0, 100.0, 50.0)], 0.5), PointF::new(50.0, 25.0));
        assert_eq!(position_at(&[], 1.0), PointF::new(0.5, 0.5));
    }
}
