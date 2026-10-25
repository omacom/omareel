//! The zoom and cursor motion of a recording, simulated with springs at 240 Hz (`MotionTrack`).

use crate::compat::cpp;
use crate::compat::geometry::PointF;
use crate::cursor_path::{self, CursorSample};
use crate::input_log::{InputEvent, InputKind};
use crate::project::{Project, Spring};
use crate::zoom_timeline;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSample {
    pub zoom_scale: f64,
    pub zoom_cx: f64,
    pub zoom_cy: f64,
    pub zoom_center_velocity: f64,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_scale: f64,
    pub cursor_opacity: f64,
    pub cursor_rotation: f64,
}

impl Default for MotionSample {
    fn default() -> MotionSample {
        MotionSample {
            zoom_scale: 1.0,
            zoom_cx: 0.5,
            zoom_cy: 0.5,
            zoom_center_velocity: 0.0,
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_scale: 1.0,
            cursor_opacity: 1.0,
            cursor_rotation: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RippleSample {
    pub x: f64,
    pub y: f64,
    pub progress: f64,
}

impl Default for RippleSample {
    fn default() -> RippleSample {
        RippleSample { x: 0.5, y: 0.5, progress: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Click {
    down: f64,
    up: f64,
    position: PointF,
}

/// Built once per project change (the editor does it on a worker thread, so it is `Send`), then
/// sampled every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MotionTrack {
    duration: f64,
    samples: Vec<MotionSample>,
    clicks: Vec<Click>,
}

#[derive(Clone, Copy)]
struct SpringState {
    x: f64,
    velocity: f64,
}

impl SpringState {
    fn new(x: f64, velocity: f64) -> SpringState {
        SpringState { x, velocity }
    }
}

fn integrate(state: &mut SpringState, target: f64, spring: &Spring, dt: f64) {
    if spring.mass <= 0.0 || spring.stiffness <= 0.0 {
        state.x = target;
        state.velocity = 0.0;
        return;
    }
    let acceleration = (spring.stiffness * (target - state.x) - spring.damping * state.velocity) / spring.mass;
    state.velocity += acceleration * dt;
    state.x += state.velocity * dt;
}

impl MotionTrack {
    pub const SAMPLE_RATE: f64 = 240.0;

    /// Simulates `duration` seconds of a `source_width` x `source_height` recording from its
    /// input events and the project's zoom and cursor settings.
    pub fn build(
        duration: f64,
        source_width: i32,
        source_height: i32,
        events: &[InputEvent],
        project: &Project,
        capture_fps: f64,
    ) -> MotionTrack {
        let mut track = MotionTrack { duration: cpp::max(0.0, duration), ..MotionTrack::default() };
        let mut raw: Vec<CursorSample> = Vec::new();
        for event in events {
            let normalized = PointF::new(
                event.position.x / f64::from(cpp::max(1, source_width)),
                event.position.y / f64::from(cpp::max(1, source_height)),
            );
            match event.kind {
                InputKind::Move => raw.push(CursorSample { time: event.time, position: normalized }),
                InputKind::ButtonDown => track.clicks.push(Click {
                    down: event.time,
                    up: cpp::min(duration, event.time + 0.13),
                    position: normalized,
                }),
                InputKind::ButtonUp => {
                    if let Some(click) = track.clicks.iter_mut().rev().find(|click| click.up <= click.down + 0.130001) {
                        click.up = cpp::max(click.down, event.time);
                    }
                }
                _ => {}
            }
        }
        // Samples are normalized, so the one-pixel threshold must be normalized too — otherwise
        // every move is "less than a pixel" and only the first and last samples survive.
        let raw = {
            let min_distance = 1.0 / f64::from(cpp::max(1, cpp::max(source_width, source_height)));
            let mut decimated = cursor_path::decimate(&raw, capture_fps, min_distance);
            if decimated.is_empty() {
                decimated.push(CursorSample { time: 0.0, position: PointF::new(0.5, 0.5) });
            }
            decimated
        };

        // int(ceil(...)) + 1 in the C++: out of int range (a duration over about 103 days) x86-64
        // gives INT_MIN + 1 and aarch64 INT_MAX + 1, which wraps; either way the count is 1, as
        // the saturating cast and wrapping add give here.
        let count = cpp::max(1, ((track.duration * MotionTrack::SAMPLE_RATE).ceil() as i32).wrapping_add(1));
        track.samples.reserve(usize::try_from(count).unwrap_or(1));
        let initial_cursor = cursor_path::position_at(&raw, 0.0);
        let mut zoom_scale = SpringState::new(1.0, 0.0);
        let mut zoom_x = SpringState::new(0.5, 0.0);
        let mut zoom_y = SpringState::new(0.5, 0.0);
        let mut cursor_x = SpringState::new(initial_cursor.x, 0.0);
        let mut cursor_y = SpringState::new(initial_cursor.y, 0.0);
        let mut cursor_scale = SpringState::new(1.0, 0.0);
        let mut cursor_opacity = SpringState::new(if project.cursor.visible { 1.0 } else { 0.0 }, 0.0);
        let mut cursor_rotation = SpringState::new(0.0, 0.0);
        let click_spring = Spring { mass: 0.3, stiffness: 300.0, damping: 30.0 };
        let dt = 1.0 / MotionTrack::SAMPLE_RATE;

        for i in 0..count {
            let time = cpp::min(track.duration, f64::from(i) / MotionTrack::SAMPLE_RATE);
            let cursor_target = cursor_path::position_at(&raw, time);
            let zoom_target = zoom_timeline::target_at(&project.zooms, &raw, time, &project.zoom_style);
            let pressed = track.clicks.iter().any(|click| time >= click.down - 0.13 && time <= click.up);

            let mut hidden = !project.cursor.visible;
            if !hidden && project.cursor.hide_when_idle_ms >= 0 {
                let mut previous = raw[0].time;
                let mut next = track.duration + 1.0;
                for movement in &raw {
                    if movement.time <= time {
                        previous = movement.time;
                    } else {
                        next = movement.time;
                        break;
                    }
                }
                hidden = time - previous > f64::from(project.cursor.hide_when_idle_ms) / 1000.0 && time < next - 0.25;
            }
            let scale_target = if pressed {
                project.cursor.click_shrink
            } else if hidden {
                0.8
            } else {
                1.0
            };
            let rotation_target = cpp::clamp(
                (cursor_target.x - cursor_path::position_at(&raw, time - 0.4).x)
                    * f64::from(source_width)
                    * 0.03
                    * project.cursor.rotate_on_x_movement_ratio,
                -20.0,
                20.0,
            );
            let near_boundary = project.zoom_style.instant_animation
                && project.zooms.iter().any(|z| (time - z.start).abs() <= 0.1 || (time - z.end).abs() <= 0.1);
            if near_boundary {
                zoom_scale = SpringState::new(zoom_target.scale, 0.0);
                zoom_x = SpringState::new(zoom_target.center.x, 0.0);
                zoom_y = SpringState::new(zoom_target.center.y, 0.0);
            } else if i > 0 {
                integrate(&mut zoom_scale, zoom_target.scale, &project.zoom_style.spring, dt);
                integrate(&mut zoom_x, zoom_target.center.x, &project.zoom_style.spring, dt);
                integrate(&mut zoom_y, zoom_target.center.y, &project.zoom_style.spring, dt);
            }
            if i > 0 {
                if project.cursor.smoothing {
                    integrate(&mut cursor_x, cursor_target.x, &project.cursor.spring, dt);
                    integrate(&mut cursor_y, cursor_target.y, &project.cursor.spring, dt);
                } else {
                    cursor_x = SpringState::new(cursor_target.x, 0.0);
                    cursor_y = SpringState::new(cursor_target.y, 0.0);
                }
                integrate(&mut cursor_scale, scale_target, &click_spring, dt);
                integrate(&mut cursor_opacity, if hidden { 0.0 } else { 1.0 }, &click_spring, dt);
                integrate(&mut cursor_rotation, rotation_target, &project.cursor.spring, dt);
            }
            track.samples.push(MotionSample {
                zoom_scale: zoom_scale.x,
                zoom_cx: zoom_x.x,
                zoom_cy: zoom_y.x,
                zoom_center_velocity: zoom_x.velocity.hypot(zoom_y.velocity),
                cursor_x: cursor_x.x,
                cursor_y: cursor_y.x,
                cursor_scale: cursor_scale.x,
                cursor_opacity: cursor_opacity.x,
                cursor_rotation: cursor_rotation.x,
            });
        }
        // Ripples ride on the cursor the viewer actually sees (the smoothed one),
        // not on the raw click position the spring may still be catching up to.
        for i in 0..track.clicks.len() {
            let at = track.sample(track.clicks[i].down);
            track.clicks[i].position = PointF::new(at.cursor_x, at.cursor_y);
        }
        track
    }

    /// The motion at `time`, interpolated between the two nearest samples.
    pub fn sample(&self, time: f64) -> MotionSample {
        if self.samples.is_empty() {
            return MotionSample::default();
        }
        let index = cpp::clamp(time, 0.0, self.duration) * MotionTrack::SAMPLE_RATE;
        let last = (self.samples.len() - 1) as i32;
        // int(index) in the C++ is out of range only for a NaN time, or an index past INT_MAX
        // (an infinite duration, or one over about 103 days, which has one sample). x86-64 then
        // indexes the samples with INT_MIN (undefined behaviour); this saturates and maps NaN to
        // 0 as aarch64 does, which gives NaN for a NaN or infinite index and the one sample
        // otherwise.
        let a = cpp::min(index as i32, last);
        let b = cpp::min(a + 1, last);
        let t = index - f64::from(a);
        let mix = |x: f64, y: f64| x + (y - x) * t;
        let (sa, sb) = (&self.samples[a as usize], &self.samples[b as usize]);
        MotionSample {
            zoom_scale: mix(sa.zoom_scale, sb.zoom_scale),
            zoom_cx: mix(sa.zoom_cx, sb.zoom_cx),
            zoom_cy: mix(sa.zoom_cy, sb.zoom_cy),
            zoom_center_velocity: mix(sa.zoom_center_velocity, sb.zoom_center_velocity),
            cursor_x: mix(sa.cursor_x, sb.cursor_x),
            cursor_y: mix(sa.cursor_y, sb.cursor_y),
            cursor_scale: mix(sa.cursor_scale, sb.cursor_scale),
            cursor_opacity: mix(sa.cursor_opacity, sb.cursor_opacity),
            cursor_rotation: mix(sa.cursor_rotation, sb.cursor_rotation),
        }
    }

    /// The click ripples showing at `time`: 0.45 s from each press.
    pub fn ripples(&self, time: f64) -> Vec<RippleSample> {
        let mut result = Vec::new();
        for click in &self.clicks {
            let age = time - click.down;
            if (0.0..=0.45).contains(&age) {
                result.push(RippleSample { x: click.position.x, y: click.position.y, progress: age / 0.45 });
            }
        }
        result
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// The number of samples (`qsizetype` in the C++).
    pub fn size(&self) -> usize {
        self.samples.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const _: () = {
        const fn assert_send<T: Send>() {}
        assert_send::<MotionTrack>();
    };

    fn moves_to(time: f64, x: f64, y: f64) -> InputEvent {
        InputEvent { kind: InputKind::Move, time, position: PointF::new(x, y), ..InputEvent::default() }
    }

    // tests/test_cursorpath.cpp
    // Regression: samples are normalized before decimation, so the one-pixel threshold must be
    // normalized too. With the bug only the first and last samples survived and the cursor
    // drifted in a straight line from start to end for the whole recording.
    #[test]
    fn decimation_keeps_intermediate_samples_in_normalized_space() {
        let mut project = Project::defaults("decimate", 2.0);
        project.zooms.clear();
        let mut events = Vec::new();
        for i in 0..=200 {
            let leg = f64::from(if i <= 100 { i } else { 200 - i }); // out to (100,100) and back to (0,0)
            events.push(moves_to(f64::from(i) / 100.0, leg, leg));
        }
        let track = MotionTrack::build(2.0, 200, 200, &events, &project, 60.0);
        let mid = track.sample(1.05); // just after the turnaround, spring settled
        assert!(
            mid.cursor_x > 0.35 && mid.cursor_y > 0.35,
            "cursor at {},{} — path collapsed to its endpoints",
            mid.cursor_x,
            mid.cursor_y
        );
    }

    #[test]
    fn spring_converges_without_two_percent_overshoot() {
        let mut project = Project::defaults("spring", 2.0);
        project.zooms.clear();
        let events = [moves_to(0.0, 0.0, 50.0), moves_to(0.1, 100.0, 50.0)];
        let track = MotionTrack::build(2.0, 100, 100, &events, &project, 60.0);
        let mut maximum = 0.0;
        for i in 0..=480 {
            maximum = cpp::max(maximum, track.sample(f64::from(i) / 240.0).cursor_x);
        }
        assert!(maximum <= 1.02);
        assert!(track.sample(2.0).cursor_x > 0.999);
    }

    // Not in the C++ tests.
    #[test]
    fn clicks_ripple_where_the_smoothed_cursor_is() {
        let project = Project::defaults("ripples", 1.0);
        let down = InputEvent { kind: InputKind::ButtonDown, time: 0.5, position: PointF::new(50.0, 50.0), ..InputEvent::default() };
        let up = InputEvent { kind: InputKind::ButtonUp, time: 0.7, ..down.clone() };
        let track = MotionTrack::build(1.0, 100, 100, &[moves_to(0.0, 10.0, 20.0), down, up], &project, 60.0);
        assert_eq!(track.size(), 241);
        assert_eq!(track.duration(), 1.0);
        assert!(track.ripples(0.49).is_empty());
        let ripples = track.ripples(0.5 + 0.225);
        assert_eq!(ripples.len(), 1);
        assert_eq!((ripples[0].x, ripples[0].y), (track.sample(0.5).cursor_x, track.sample(0.5).cursor_y));
        assert!((ripples[0].progress - 0.5).abs() < 1e-12);
        assert!(track.ripples(0.96).is_empty());
        assert_eq!(MotionTrack::default().sample(1.0), MotionSample::default());
        assert_eq!(MotionTrack::build(f64::INFINITY, 100, 100, &[], &project, 60.0).size(), 1);
    }
}
