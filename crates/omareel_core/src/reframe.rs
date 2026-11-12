//! Auto-reframe: for a canvas narrower (or wider) than the recording, such as 9:16 or 1:1 from a
//! 16:9 screen, the screen fills the frame as a crop window that follows what matters instead of
//! shrinking to a strip. Project `reframe: "follow"` turns it on.
//!
//! Where the window is at each moment is decided here, from the motion track's own inputs (built
//! with it, see [`MotionTrack::build`](crate::motion_track::MotionTrack::build)), so the preview and
//! the export read the same window for the same time:
//! - while a zoom is active the window centres on the zoom's target, and shrinks with the zoom's
//!   spring-smoothed scale (so a zoom in a reframed video is a tighter crop, not a scale);
//! - otherwise it follows the smoothed cursor, but only once the cursor leaves the dead zone, the
//!   middle [`DEAD_ZONE`] of the window, so small movements don't pan;
//! - the window's centre moves on the project's zoom spring, so pans feel like its zooms;
//! - a cursor within [`EDGE_SNAP`] of a window width from a screen edge puts the window flush
//!   against that edge (the middle stretches to meet it, so the aim never jumps), and the window
//!   never leaves the recording.
//!
//! Coordinates are normalised to the project's crop of the recording (0..1 across what the crop
//! keeps). Composition.qml draws the window: the frame shows that part of the recording, and the
//! cursor and ripples are mapped into it.

use crate::compat::cpp;
use crate::compat::geometry::PointF;
use crate::export::plan::output_aspect;
use crate::project::{Project, Spring};

/// The reframe modes `project.json` accepts. A project without `reframe` fits the whole
/// recording into the canvas, as before.
pub const MODES: [&str; 1] = ["follow"];

/// The middle share of the window (each way) the cursor may move in without moving the window.
pub const DEAD_ZONE: f64 = 0.5;

/// Within this share of the window's size from a screen edge, the window snaps flush to it.
pub const EDGE_SNAP: f64 = 0.15;

/// The part of the (cropped) recording the canvas shows, normalised to the crop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReframeWindow {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for ReframeWindow {
    fn default() -> ReframeWindow {
        ReframeWindow { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }
    }
}

impl ReframeWindow {
    pub fn center(&self) -> PointF {
        PointF::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// What the window follows at one step of the motion track, in crop coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Focus {
    /// The smoothed cursor.
    pub cursor: PointF,
    /// The zoom's spring-smoothed scale (1 when not zoomed).
    pub zoom_scale: f64,
    /// The active zoom's target, while a zoom segment is active.
    pub zoom_target: Option<PointF>,
}

impl Default for Focus {
    fn default() -> Focus {
        Focus { cursor: PointF::new(0.5, 0.5), zoom_scale: 1.0, zoom_target: None }
    }
}

/// Whether the project reframes.
pub fn enabled(project: &Project) -> bool {
    MODES.contains(&project.reframe.as_str())
}

/// The aspect of the screen's frame on the canvas: the canvas less the padding and the frame's
/// border, as Composition.qml lays it out (the same at any output height).
pub fn frame_aspect(project: &Project, source_width: i32, source_height: i32) -> f64 {
    let aspect = output_aspect(&project.aspect, source_width, source_height, &project.crop);
    let height = 1080.0;
    let width = height * aspect;
    if project.background.type_ == "none" {
        return aspect;
    }
    let pad = cpp::min(width, height) * project.frame.padding;
    let border = if project.frame.border.enabled { project.frame.border.width } else { 0.0 };
    let inset = 2.0 * (pad + border);
    cpp::max(1.0, width - inset) / cpp::max(1.0, height - inset)
}

/// The unzoomed window's size in crop coordinates: as tall as the recording and narrower for a
/// canvas narrower than it, as wide and shorter for a wider one.
pub fn base_size(project: &Project, source_width: i32, source_height: i32) -> (f64, f64) {
    let content = (f64::from(source_width) * project.crop.width)
        / cpp::max(1.0, f64::from(source_height) * project.crop.height);
    let frame = frame_aspect(project, source_width, source_height);
    if !(content > 0.0) || !(frame > 0.0) {
        return (1.0, 1.0);
    }
    if frame < content { (frame / content, 1.0) } else { (1.0, content / frame) }
}

/// Moves `goal` the least it takes to bring `point` within `half` of it.
fn follow(goal: f64, point: f64, half: f64) -> f64 {
    if point > goal + half {
        point - half
    } else if point < goal - half {
        point + half
    } else {
        goal
    }
}

/// Where the window's centre aims for `goal` along one axis, for a window `size` long: inside
/// the recording, and flush with an edge the goal comes within [`EDGE_SNAP`] of. The middle is
/// stretched to meet the snapped ends, so the aim never jumps.
fn place(goal: f64, size: f64) -> f64 {
    if size >= 1.0 {
        return 0.5;
    }
    let low = size / 2.0;
    let high = 1.0 - size / 2.0;
    let snap = EDGE_SNAP * size;
    let inner_low = low + snap;
    let inner_high = high - snap;
    if inner_high <= inner_low {
        return cpp::clamp(goal, low, high);
    }
    cpp::clamp(low + (goal - inner_low) * (high - low) / (inner_high - inner_low), low, high)
}

/// `goal` kept far enough from the edges for a window `size` long.
fn clamp_center(goal: f64, size: f64) -> f64 {
    if size >= 1.0 { 0.5 } else { cpp::clamp(goal, size / 2.0, 1.0 - size / 2.0) }
}

fn integrate(x: &mut f64, velocity: &mut f64, target: f64, spring: &Spring, dt: f64) {
    if spring.mass <= 0.0 || spring.stiffness <= 0.0 {
        *x = target;
        *velocity = 0.0;
        return;
    }
    let acceleration = (spring.stiffness * (target - *x) - spring.damping * *velocity) / spring.mass;
    *velocity += acceleration * dt;
    *x += *velocity * dt;
}

/// The window over time, simulated once and then sampled, like the motion track.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReframeTrack {
    duration: f64,
    sample_rate: f64,
    samples: Vec<ReframeWindow>,
}

impl ReframeTrack {
    /// Simulates the window from one [`Focus`] per step at `sample_rate`, the first at time 0.
    /// `base` is the unzoomed window's size ([`base_size`]); the centre moves on `spring`.
    pub fn build(duration: f64, sample_rate: f64, focus: &[Focus], base: (f64, f64), spring: &Spring) -> ReframeTrack {
        let mut track = ReframeTrack { duration: cpp::max(0.0, duration), sample_rate, samples: Vec::with_capacity(focus.len()) };
        let dt = 1.0 / sample_rate;
        let (mut goal_x, mut goal_y) = (0.5, 0.5);
        let (mut x, mut y, mut vx, mut vy) = (0.5, 0.5, 0.0, 0.0);
        for (i, f) in focus.iter().enumerate() {
            let scale = if f.zoom_scale.is_finite() { cpp::max(1.0, f.zoom_scale) } else { 1.0 };
            let w = cpp::min(1.0, base.0 / scale);
            let h = cpp::min(1.0, base.1 / scale);
            let zoomed = f.zoom_target.is_some();
            match f.zoom_target {
                Some(target) => {
                    goal_x = cpp::clamp(target.x, 0.0, 1.0);
                    goal_y = cpp::clamp(target.y, 0.0, 1.0);
                }
                None => {
                    let point = PointF::new(cpp::clamp(f.cursor.x, 0.0, 1.0), cpp::clamp(f.cursor.y, 0.0, 1.0));
                    if i == 0 {
                        (goal_x, goal_y) = (point.x, point.y);
                    }
                    goal_x = follow(goal_x, point.x, DEAD_ZONE * w / 2.0);
                    goal_y = follow(goal_y, point.y, DEAD_ZONE * h / 2.0);
                }
            }
            // A zoom's target already keeps to the edges it is near (zoom_timeline's snap).
            let aim_x = if zoomed { clamp_center(goal_x, w) } else { place(goal_x, w) };
            let aim_y = if zoomed { clamp_center(goal_y, h) } else { place(goal_y, h) };
            if i == 0 {
                (x, y) = (aim_x, aim_y);
            } else {
                integrate(&mut x, &mut vx, aim_x, spring, dt);
                integrate(&mut y, &mut vy, aim_y, spring, dt);
            }
            // The window may have grown with a zoom out faster than the centre moved.
            let cx = if w >= 1.0 { 0.5 } else { cpp::clamp(x, w / 2.0, 1.0 - w / 2.0) };
            let cy = if h >= 1.0 { 0.5 } else { cpp::clamp(y, h / 2.0, 1.0 - h / 2.0) };
            track.samples.push(ReframeWindow { x: cx - w / 2.0, y: cy - h / 2.0, w, h });
        }
        track
    }

    /// The window at `time`, interpolated between the two nearest steps.
    pub fn sample(&self, time: f64) -> ReframeWindow {
        if self.samples.is_empty() {
            return ReframeWindow::default();
        }
        let index = if time.is_nan() { 0.0 } else { cpp::clamp(time, 0.0, self.duration) * self.sample_rate };
        let last = self.samples.len() - 1;
        let a = cpp::min(index as usize, last);
        let b = cpp::min(a + 1, last);
        let t = index - a as f64;
        let (sa, sb) = (&self.samples[a], &self.samples[b]);
        let mix = |p: f64, q: f64| p + (q - p) * t;
        ReframeWindow { x: mix(sa.x, sb.x), y: mix(sa.y, sb.y), w: mix(sa.w, sb.w), h: mix(sa.h, sb.h) }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_log::{InputEvent, InputKind};
    use crate::motion_track::MotionTrack;
    use crate::project::ZoomSegment;

    const RATE: f64 = 240.0;
    // 9:16 from 16:9 with no padding: the window is (9/16)/(16/9) of the width, full height.
    const PORTRAIT: (f64, f64) = (0.31640625, 1.0);

    fn spring() -> Spring {
        crate::project::ZoomStyle::default().spring
    }

    fn cursor_at(x: f64, y: f64) -> Focus {
        Focus { cursor: PointF::new(x, y), ..Focus::default() }
    }

    fn inside(window: &ReframeWindow) -> bool {
        let e = 1e-9;
        window.x >= -e && window.y >= -e && window.x + window.w <= 1.0 + e && window.y + window.h <= 1.0 + e
    }

    #[test]
    fn base_size_follows_the_canvas() {
        let mut project = Project::defaults("reframe", 1.0);
        project.background.type_ = "none".into();
        project.aspect = "9:16".into();
        let (w, h) = base_size(&project, 1920, 1080);
        assert!((w - PORTRAIT.0).abs() < 1e-12 && h == 1.0, "{w} {h}");
        project.aspect = "1:1".into();
        assert_eq!(base_size(&project, 1920, 1080), (0.5625, 1.0));
        project.aspect = "16:9".into();
        assert_eq!(base_size(&project, 1080, 1920), (1.0, 0.31640625));
        project.aspect = "auto".into();
        assert_eq!(base_size(&project, 1920, 1080), (1.0, 1.0));
        // Padding takes the same on every side, which makes a portrait frame narrower still.
        project.background.type_ = "gradient".into();
        project.aspect = "9:16".into();
        project.frame.padding = 0.1;
        project.frame.border.enabled = false;
        let (w, _) = base_size(&project, 1920, 1080);
        let frame = (607.5 - 121.5) / (1080.0 - 121.5);
        assert!((w - frame / (16.0 / 9.0)).abs() < 1e-12);
    }

    #[test]
    fn follows_the_cursor_with_a_dead_zone() {
        // The cursor wanders a little around the middle, then moves to the right for good.
        let mut focus = Vec::new();
        for i in 0..240 {
            let wobble = 0.05 * (f64::from(i) / 10.0).sin();
            focus.push(cursor_at(0.5 + wobble, 0.5));
        }
        for _ in 0..480 {
            focus.push(cursor_at(0.7, 0.5));
        }
        let track = ReframeTrack::build(3.0, RATE, &focus, PORTRAIT, &spring());
        // Inside the dead zone (a quarter of the window each way, about 0.079), nothing moves.
        for i in 0..240 {
            let window = track.sample(f64::from(i) / RATE);
            assert!((window.center().x - 0.5).abs() < 1e-9, "moved at step {i}: {window:?}");
        }
        // Then the window follows until the cursor is back at the dead zone's edge.
        let settled = track.sample(3.0);
        let expected = place(0.7 - DEAD_ZONE * PORTRAIT.0 / 2.0, PORTRAIT.0);
        assert!((settled.center().x - expected).abs() < 1e-3, "{settled:?} vs {expected}");
        assert!(track.sample(1.1).center().x > 0.5 && track.sample(1.1).center().x < settled.center().x + 1e-9);
        assert_eq!(settled.w, PORTRAIT.0);
        assert_eq!((settled.y, settled.h), (0.0, 1.0));
    }

    #[test]
    fn follows_the_zoom_target_and_shrinks_with_the_zoom() {
        let mut focus = vec![cursor_at(0.2, 0.5); 240];
        // A 2x zoom aimed at the right third while the cursor stays on the left.
        let mut scale = 1.0;
        for _ in 0..480 {
            scale += (2.0 - scale) * 0.05;
            focus.push(Focus { cursor: PointF::new(0.2, 0.5), zoom_scale: scale, zoom_target: Some(PointF::new(0.7, 0.4)) });
        }
        let track = ReframeTrack::build(3.0, RATE, &focus, PORTRAIT, &spring());
        let zoomed = track.sample(3.0);
        assert!((zoomed.w - PORTRAIT.0 / 2.0).abs() < 1e-6);
        assert!((zoomed.h - 0.5).abs() < 1e-6);
        assert!((zoomed.center().x - 0.7).abs() < 1e-3, "{zoomed:?}");
        assert!((zoomed.center().y - 0.4).abs() < 1e-3, "{zoomed:?}");
    }

    #[test]
    fn stays_inside_and_snaps_to_edges() {
        let mut focus = Vec::new();
        // Corners, off-screen positions, and a zoom out near the edge.
        for (i, &(x, y)) in [(0.0, 0.0), (1.0, 1.0), (-0.3, 0.5), (1.4, 0.2), (0.97, 0.5), (0.03, 0.5)].iter().enumerate() {
            for _ in 0..240 {
                let zoom = if i == 4 { Some(PointF::new(0.95, 0.9)) } else { None };
                focus.push(Focus { cursor: PointF::new(x, y), zoom_scale: if i == 4 { 2.5 } else { 1.0 }, zoom_target: zoom });
            }
        }
        let track = ReframeTrack::build(6.0, RATE, &focus, PORTRAIT, &spring());
        for i in 0..=6 * 240 {
            let window = track.sample(f64::from(i) / RATE);
            assert!(inside(&window), "outside at {i}: {window:?}");
        }
        // A cursor near (not at) the right edge puts the window flush with it.
        let mut near = vec![cursor_at(0.5, 0.5)];
        near.extend(vec![cursor_at(0.93, 0.5); 480]);
        let track = ReframeTrack::build(2.0, RATE, &near, PORTRAIT, &spring());
        let window = track.sample(2.0);
        assert!((window.x + window.w - 1.0).abs() < 1e-3, "{window:?}");
        // And a full-size window never moves.
        let track = ReframeTrack::build(2.0, RATE, &near, (1.0, 1.0), &spring());
        assert_eq!(track.sample(1.0), ReframeWindow::default());
    }

    #[test]
    fn deterministic_for_the_same_time() {
        let mut project = Project::defaults("reframe", 4.0);
        project.aspect = "9:16".into();
        project.reframe = "follow".into();
        project.zooms = vec![ZoomSegment { id: "z1".into(), start: 2.0, end: 3.5, ..ZoomSegment::default() }];
        let mut events = Vec::new();
        for i in 0..=400 {
            let t = f64::from(i) / 100.0;
            let x = 200.0 + 1500.0 * (t / 4.0);
            events.push(InputEvent { kind: InputKind::Move, time: t, position: PointF::new(x, 540.0), ..InputEvent::default() });
        }
        let a = MotionTrack::build(4.0, 1920, 1080, &events, &project, 60.0);
        let b = MotionTrack::build(4.0, 1920, 1080, &events, &project, 60.0);
        assert_eq!(a, b);
        let mut previous = 0.0;
        for i in 0..=40 {
            let t = f64::from(i) / 10.0;
            let window = a.reframe(t).expect("reframed");
            assert_eq!(Some(window), a.reframe(t));
            assert_eq!(Some(window), b.reframe(t));
            assert!(inside(&window));
            // Before the zoom the cursor only moves right, so the window never moves left.
            if t < 2.0 {
                assert!(window.center().x >= previous - 1e-9, "{t}: {window:?}");
                previous = window.center().x;
            }
        }
        assert!(a.reframe(4.0).unwrap().center().x > 0.6);
        // Off without the setting.
        project.reframe.clear();
        assert_eq!(MotionTrack::build(4.0, 1920, 1080, &events, &project, 60.0).reframe(1.0), None);
    }
}
