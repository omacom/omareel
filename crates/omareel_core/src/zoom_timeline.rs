//! Automatic zooms and the zoom target over time (`ZoomTimeline`).

use crate::compat::cpp;
use crate::compat::geometry::PointF;
use crate::compat::qjson::{Object, Value};
use crate::cursor_path::CursorSample;
use crate::project::{ZoomSegment, ZoomStyle};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomFrame {
    pub scale: f64,
    pub center: PointF,
}

impl Default for ZoomFrame {
    fn default() -> ZoomFrame {
        ZoomFrame { scale: 1.0, center: PointF::new(0.5, 0.5) }
    }
}

/// Zooms around clicks: each starts 0.3 s before a click and lasts 2.5 s after it, ending
/// at least 0.8 s before the end; clicks within 2.5 s of a zoom extend it.
pub fn generate(click_down_times: &[f64], duration: f64) -> Vec<ZoomSegment> {
    let mut clicks = click_down_times.to_vec();
    clicks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut result: Vec<ZoomSegment> = Vec::new();
    for click in clicks {
        if duration <= 0.0 || click < 0.0 || click >= duration - 1.0 {
            continue;
        }
        let start = cpp::max(0.0, click - 0.3);
        let end = cpp::max(start, cpp::min(click + 2.5, duration - 0.8));
        match result.last_mut() {
            Some(last) if start - last.end <= 2.5 => last.end = cpp::max(last.end, end),
            _ => {
                let id = format!("z{}", result.len() + 1);
                result.push(ZoomSegment { id, start, end, level: 2.0, automatic_target: true, target: PointF::new(0.5, 0.5) });
            }
        }
    }
    result
}

/// Keeps a zoom's centre far enough from the edges that the zoomed view stays inside.
pub fn clamp_center(center: PointF, level: f64) -> PointF {
    let half = 0.5 / cpp::max(1.0, level);
    PointF::new(cpp::clamp(center.x, half, 1.0 - half), cpp::clamp(center.y, half, 1.0 - half))
}

fn snap_edges(target: PointF, level: f64, ratio: f64) -> PointF {
    let visible = 1.0 / cpp::max(1.0, level);
    let r = cpp::min(cpp::max(0.0, ratio), visible);
    let denominator = cpp::max(0.0001, 1.0 - 2.0 * r + 0.0001);
    let snapped = PointF::new(
        cpp::clamp((target.x - r) / denominator, 0.0, 1.0),
        cpp::clamp((target.y - r) / denominator, 0.0, 1.0),
    );
    clamp_center(snapped, level)
}

/// The zoom at `time`: the latest-starting segment covering it, aimed at a fixed target or
/// at the group of cursor positions the view can hold.
pub fn target_at(segments: &[ZoomSegment], samples: &[CursorSample], time: f64, style: &ZoomStyle) -> ZoomFrame {
    let mut active: Option<&ZoomSegment> = None;
    for segment in segments {
        if time >= segment.start && time <= segment.end && active.is_none_or(|a| segment.start > a.start) {
            active = Some(segment);
        }
    }
    let Some(active) = active else { return ZoomFrame::default() };
    if !active.automatic_target {
        return ZoomFrame { scale: active.level, center: clamp_center(active.target, active.level) };
    }

    let mut relevant: Vec<CursorSample> =
        samples.iter().filter(|s| s.time >= active.start && s.time <= active.end).copied().collect();
    if relevant.is_empty() && !samples.is_empty() {
        // std::lower_bound: the first sample at or after the segment's start.
        let it = samples.partition_point(|s| s.time < active.start);
        if it == 0 {
            relevant.push(samples[0]);
        } else if it == samples.len() {
            relevant.push(samples[samples.len() - 1]);
        } else if samples[it].time - active.start < active.start - samples[it - 1].time {
            relevant.push(samples[it]);
        } else {
            relevant.push(samples[it - 1]);
        }
    }
    let Some(first) = relevant.first() else {
        return ZoomFrame { scale: active.level, center: PointF::new(0.5, 0.5) };
    };

    let max_width = 0.5 / active.level;
    let max_height = 0.7 / active.level;
    struct Group {
        start: f64,
        minimum: PointF,
        maximum: PointF,
    }
    let mut groups = vec![Group { start: first.time, minimum: first.position, maximum: first.position }];
    for sample in &relevant[1..] {
        let group = groups.last_mut().expect("starts with one group");
        let p = sample.position;
        let next_min = PointF::new(cpp::min(group.minimum.x, p.x), cpp::min(group.minimum.y, p.y));
        let next_max = PointF::new(cpp::max(group.maximum.x, p.x), cpp::max(group.maximum.y, p.y));
        if next_max.x - next_min.x <= max_width && next_max.y - next_min.y <= max_height {
            group.minimum = next_min;
            group.maximum = next_max;
        } else {
            groups.push(Group { start: sample.time, minimum: p, maximum: p });
        }
    }
    let mut active_group: Option<&Group> = None;
    for group in &groups {
        if group.start <= time {
            active_group = Some(group);
        } else {
            break;
        }
    }
    let Some(group) = active_group else {
        return ZoomFrame { scale: active.level, center: PointF::new(0.5, 0.5) };
    };
    let center = (group.minimum + group.maximum) / 2.0;
    ZoomFrame { scale: active.level, center: snap_edges(center, active.level, style.snap_to_edges_ratio) }
}

/// `"auto"`, or the fixed target as `{"x", "y"}`.
pub fn target_to_json(segment: &ZoomSegment) -> Value {
    if segment.automatic_target {
        return Value::from("auto");
    }
    Value::Object(Object::from_iter([("x", segment.target.x), ("y", segment.target.y)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fuzzy(a: f64, b: f64) -> bool {
        (a - b).abs() * 1e12 <= a.abs().min(b.abs())
    }

    fn sample(time: f64, x: f64, y: f64) -> CursorSample {
        CursorSample { time, position: PointF::new(x, y) }
    }

    fn zoom(start: f64, end: f64) -> ZoomSegment {
        ZoomSegment { id: "z".into(), start, end, level: 2.0, automatic_target: true, target: PointF::default() }
    }

    // tests/test_zoomtimeline.cpp
    #[test]
    fn amendment_generation_rules() {
        let single = generate(&[5.0], 20.0);
        assert_eq!(single.len(), 1);
        assert!(fuzzy(single[0].start, 4.7) && fuzzy(single[0].end, 7.5));
        let merged = generate(&[5.0, 7.0], 20.0);
        assert_eq!(merged.len(), 1);
        assert!(fuzzy(merged[0].start, 4.7) && fuzzy(merged[0].end, 9.5));
        assert!(generate(&[19.5], 20.0).is_empty());
        assert!(fuzzy(generate(&[18.9], 20.0)[0].end, 19.2));
    }

    #[test]
    fn auto_groups_hold_and_retarget() {
        let style = ZoomStyle { snap_to_edges_ratio: 0.0, ..ZoomStyle::default() };
        let zooms = [zoom(0.0, 4.0)];
        let samples = [sample(0.0, 0.20, 0.30), sample(1.0, 0.30, 0.40), sample(2.0, 0.70, 0.65), sample(3.0, 0.75, 0.70)];
        let first = target_at(&zooms, &samples, 0.5, &style);
        let held = target_at(&zooms, &samples, 1.5, &style);
        assert_eq!(first.center, held.center);
        assert!((first.center.x - 0.25).abs() < 0.0001);
        assert!((first.center.y - 0.35).abs() < 0.0001);
        let second = target_at(&zooms, &samples, 2.1, &style);
        assert!(second.center.x > 0.6);
        assert_ne!(second.center, first.center);
    }

    #[test]
    fn edge_snap_keeps_viewport_inside_frame() {
        let frame = target_at(&[zoom(0.0, 2.0)], &[sample(0.0, 0.02, 0.98)], 1.0, &ZoomStyle::default());
        assert!(fuzzy(frame.center.x, 0.25) && fuzzy(frame.center.y, 0.75));
        let half = 0.5 / frame.scale;
        assert!(frame.center.x - half >= 0.0);
        assert!(frame.center.y + half <= 1.0);
    }

    #[test]
    fn targets_serialize_as_auto_or_a_point() {
        let mut segment = zoom(0.0, 1.0);
        assert_eq!(target_to_json(&segment), Value::from("auto"));
        segment.automatic_target = false;
        segment.target = PointF::new(0.25, 1.0);
        assert_eq!(target_to_json(&segment).to_json(crate::compat::qjson::Format::Compact), r#"{"x":0.25,"y":1}"#);
    }
}
