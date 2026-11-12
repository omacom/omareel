//! Auto-reframe: for a canvas narrower (or wider) than the recording, such as 9:16 or 1:1 from a
//! 16:9 screen, the screen fills the frame as a crop window that follows what matters instead of
//! shrinking to a strip. Project `reframe: "follow"` turns it on.
//!
//! Where the window is at each moment is decided here, from the motion track's own inputs (built
//! with it, see [`MotionTrack::build`](crate::motion_track::MotionTrack::build)), so the preview and
//! the export read the same window for the same time:
//! - while a zoom is active the window centres on what the zoom is about (its
//!   [`ZoomInterest`](crate::zoom_timeline::ZoomInterest): a fixed target, or the middle of the
//!   cursor positions an automatic zoom holds), not on the zoom's own centre, which is kept far
//!   enough from the edges for a full-screen zoom and so misses a click near a side edge in a
//!   window this narrow. It shrinks with the zoom's spring-smoothed scale (so a zoom in a
//!   reframed video is a tighter crop, not a scale). An automatic zoom's positions can spread
//!   wider than the window, so the window also keeps the cursor in it;
//! - otherwise it follows the smoothed cursor, but only once the cursor leaves the dead zone, the
//!   middle [`DEAD_ZONE`] of the window, so small movements don't pan;
//! - the window's centre moves on the project's zoom spring, so pans feel like its zooms;
//! - a cursor within [`EDGE_SNAP`] of a window width from a screen edge puts the window flush
//!   against that edge (the middle stretches to meet it, so the aim never jumps), and the window
//!   never leaves the recording.
//!
//! The window has the canvas's shape, and the frame Composition.qml draws for it is the canvas
//! scaled down to fit inside the padding, so the window's aspect is the frame's. A canvas of the
//! recording's own shape (within [`MATCH_TOLERANCE`]) leaves nothing to reframe: no window is
//! built and the project draws as if `reframe` were unset.
//!
//! Coordinates are normalised to the project's crop of the recording (0..1 across what the crop
//! keeps). Composition.qml draws the window: the frame shows that part of the recording, and the
//! cursor and ripples are mapped into it.

use crate::compat::cpp;
use crate::compat::geometry::{PointF, RectF};
use crate::export::plan::output_aspect;
use crate::project::{Project, Spring};

/// The reframe modes `project.json` accepts. A project without `reframe` fits the whole
/// recording into the canvas, as before.
pub const MODES: [&str; 1] = ["follow"];

/// The middle share of the window (each way) the cursor may move in without moving the window.
pub const DEAD_ZONE: f64 = 0.5;

/// Within this share of the window's size from a screen edge, the window snaps flush to it.
pub const EDGE_SNAP: f64 = 0.15;

/// While an automatic zoom is active, the window aims to keep the cursor this share of its size
/// from each side, and never lets it out.
pub const ZOOM_MARGIN: f64 = 0.1;

/// A window this close to the whole crop (each way) means the canvas has the recording's shape:
/// there is nothing to reframe. BottomBar.qml's `reframeUseful` uses the same share.
pub const MATCH_TOLERANCE: f64 = 0.02;

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
    /// What the active zoom is about, while a zoom segment is active.
    pub zoom: Option<ZoomFocus>,
}

/// An active zoom's point of interest, in crop coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomFocus {
    /// Unclamped: a fixed target, or the middle of an automatic zoom's group of positions.
    pub center: PointF,
    /// An automatic zoom: keep the cursor in the window too.
    pub follow_cursor: bool,
}

impl Default for Focus {
    fn default() -> Focus {
        Focus { cursor: PointF::new(0.5, 0.5), zoom_scale: 1.0, zoom: None }
    }
}

/// Whether the project asks to reframe. See [`active`] for whether there is anything to reframe.
pub fn enabled(project: &Project) -> bool {
    MODES.contains(&project.reframe.as_str())
}

/// Whether the project reframes and its canvas has another shape than the (cropped) recording,
/// so the window is smaller than the crop. A project that asks to reframe a canvas of the
/// recording's own shape draws exactly as one that doesn't.
pub fn active(project: &Project, source_width: i32, source_height: i32) -> bool {
    enabled(project) && !fills(base_size(project, source_width, source_height))
}

/// Whether a window of this size is, within [`MATCH_TOLERANCE`], the whole crop.
pub fn fills(size: (f64, f64)) -> bool {
    size.0 >= 1.0 - MATCH_TOLERANCE && size.1 >= 1.0 - MATCH_TOLERANCE
}

/// The aspect of the reframed frame on the canvas, and so of the window: the canvas's own.
/// Composition.qml draws the frame as the canvas scaled down to fit inside the padding and the
/// border, so a canvas of the recording's shape gets the whole recording and padding never
/// crops it.
pub fn frame_aspect(project: &Project, source_width: i32, source_height: i32) -> f64 {
    output_aspect(&project.aspect, source_width, source_height, &project.crop)
}

/// The unzoomed window's size in crop coordinates: as tall as the recording and narrower for a
/// canvas narrower than it, as wide and shorter for a wider one, (1, 1) for one of its shape.
pub fn base_size(project: &Project, source_width: i32, source_height: i32) -> (f64, f64) {
    let content = (f64::from(source_width) * project.crop.width)
        / cpp::max(1.0, f64::from(source_height) * project.crop.height);
    let frame = frame_aspect(project, source_width, source_height);
    if !(content > 0.0) || !(frame > 0.0) {
        return (1.0, 1.0);
    }
    if frame < content { (frame / content, 1.0) } else { (1.0, content / frame) }
}

/// `window`, from a track built for unzoomed windows of `built` size, reshaped for `now`: the
/// same centre and zoom, inside the crop. The editor draws this between an aspect or padding
/// change and the motion track's rebuild, so the frame never shows a stretched window.
pub fn refit(window: ReframeWindow, built: (f64, f64), now: (f64, f64)) -> ReframeWindow {
    if built == now {
        return window;
    }
    let resize = |size: f64, from: f64, to: f64| cpp::clamp(size * to / cpp::max(0.0001, from), 0.0001, 1.0);
    let w = resize(window.w, built.0, now.0);
    let h = resize(window.h, built.1, now.1);
    let center = window.center();
    let cx = clamp_center(center.x, w);
    let cy = clamp_center(center.y, h);
    ReframeWindow { x: cx - w / 2.0, y: cy - h / 2.0, w, h }
}

/// A window before the motion track has one (reframing just turned on): the unzoomed `base`
/// size shrunk by `zoom_scale`, centred on `cursor` (crop coordinates) and inside the crop.
pub fn provisional(cursor: PointF, zoom_scale: f64, base: (f64, f64)) -> ReframeWindow {
    let scale = if zoom_scale.is_finite() { cpp::max(1.0, zoom_scale) } else { 1.0 };
    let (w, h) = (cpp::min(1.0, base.0 / scale), cpp::min(1.0, base.1 / scale));
    let cx = clamp_center(if cursor.x.is_finite() { cursor.x } else { 0.5 }, w);
    let cy = clamp_center(if cursor.y.is_finite() { cursor.y } else { 0.5 }, h);
    ReframeWindow { x: cx - w / 2.0, y: cy - h / 2.0, w, h }
}

/// A point normalised to the recording, normalised to `crop` instead.
pub fn to_crop(point: PointF, crop: &RectF) -> PointF {
    PointF::new(
        (point.x - crop.x) / cpp::max(0.0001, crop.width),
        (point.y - crop.y) / cpp::max(0.0001, crop.height),
    )
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
    /// The unzoomed window size it was built for, so [`refit`] can reshape it until a rebuild.
    base: (f64, f64),
    samples: Vec<ReframeWindow>,
}

impl ReframeTrack {
    /// Simulates the window from one [`Focus`] per step at `sample_rate`, the first at time 0.
    /// `base` is the unzoomed window's size ([`base_size`]); the centre moves on `spring`.
    pub fn build(duration: f64, sample_rate: f64, focus: &[Focus], base: (f64, f64), spring: &Spring) -> ReframeTrack {
        let mut track =
            ReframeTrack { duration: cpp::max(0.0, duration), sample_rate, base, samples: Vec::with_capacity(focus.len()) };
        let dt = 1.0 / sample_rate;
        let (mut goal_x, mut goal_y) = (0.5, 0.5);
        let (mut x, mut y, mut vx, mut vy) = (0.5, 0.5, 0.0, 0.0);
        for (i, f) in focus.iter().enumerate() {
            let scale = if f.zoom_scale.is_finite() { cpp::max(1.0, f.zoom_scale) } else { 1.0 };
            let w = cpp::min(1.0, base.0 / scale);
            let h = cpp::min(1.0, base.1 / scale);
            let zoomed = f.zoom.is_some();
            let cursor = PointF::new(cpp::clamp(f.cursor.x, 0.0, 1.0), cpp::clamp(f.cursor.y, 0.0, 1.0));
            match f.zoom {
                Some(zoom) => {
                    goal_x = cpp::clamp(zoom.center.x, 0.0, 1.0);
                    goal_y = cpp::clamp(zoom.center.y, 0.0, 1.0);
                    if zoom.follow_cursor {
                        // The least move from the group's middle that keeps the cursor clear of
                        // the window's sides, for a group wider than the window.
                        goal_x = follow(goal_x, cursor.x, (0.5 - ZOOM_MARGIN) * w);
                        goal_y = follow(goal_y, cursor.y, (0.5 - ZOOM_MARGIN) * h);
                    }
                }
                None => {
                    if i == 0 {
                        (goal_x, goal_y) = (cursor.x, cursor.y);
                    }
                    goal_x = follow(goal_x, cursor.x, DEAD_ZONE * w / 2.0);
                    goal_y = follow(goal_y, cursor.y, DEAD_ZONE * h / 2.0);
                }
            }
            // While zoomed the window centres on the point itself, only kept inside the recording:
            // an edge snap would move a click near the edge off-centre.
            let aim_x = if zoomed { clamp_center(goal_x, w) } else { place(goal_x, w) };
            let aim_y = if zoomed { clamp_center(goal_y, h) } else { place(goal_y, h) };
            if i == 0 {
                (x, y) = (aim_x, aim_y);
            } else {
                integrate(&mut x, &mut vx, aim_x, spring, dt);
                integrate(&mut y, &mut vy, aim_y, spring, dt);
            }
            // The centre's spring can lag a window shrinking into an automatic zoom (or a cursor
            // moving within it): the cursor stays in the window regardless.
            let (mut cx, mut cy) = (x, y);
            if f.zoom.is_some_and(|zoom| zoom.follow_cursor) {
                cx = follow(cx, cursor.x, (0.5 - ZOOM_MARGIN / 2.0) * w);
                cy = follow(cy, cursor.y, (0.5 - ZOOM_MARGIN / 2.0) * h);
            }
            // The window may have grown with a zoom out faster than the centre moved.
            let cx = if w >= 1.0 { 0.5 } else { cpp::clamp(cx, w / 2.0, 1.0 - w / 2.0) };
            let cy = if h >= 1.0 { 0.5 } else { cpp::clamp(cy, h / 2.0, 1.0 - h / 2.0) };
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

    /// The unzoomed window size the track was built for.
    pub fn base(&self) -> (f64, f64) {
        self.base
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
        // Padding and a border shrink the frame but keep the canvas's shape, so they never
        // change the window.
        project.background.type_ = "gradient".into();
        project.frame.padding = 0.1;
        project.frame.border.enabled = true;
        project.frame.border.width = 6.0;
        for aspect in ["9:16", "1:1", "auto"] {
            project.aspect = aspect.into();
            let mut bare = project.clone();
            bare.background.type_ = "none".into();
            assert_eq!(base_size(&project, 1920, 1080), base_size(&bare, 1920, 1080), "{aspect}");
        }
    }

    // Regression: the window took the padded area's shape, so a canvas of the recording's own
    // shape was still cropped (1920x1080 at auto gave (1, 0.9)) and panned.
    #[test]
    fn a_canvas_of_the_recordings_shape_has_nothing_to_reframe() {
        let mut project = Project::defaults("reframe", 1.0);
        project.reframe = "follow".into();
        assert!(project.background.type_ != "none" && project.frame.padding > 0.0);
        for (width, height, aspect) in [(1920, 1080, "auto"), (1920, 1080, "16:9"), (1080, 1920, "9:16"), (1080, 1080, "1:1")] {
            project.aspect = aspect.into();
            assert_eq!(base_size(&project, width, height), (1.0, 1.0), "{width}x{height} {aspect}");
            assert!(!active(&project, width, height), "{width}x{height} {aspect}");
        }
        // A cropped recording is matched by its crop's shape.
        project.aspect = "auto".into();
        project.crop = RectF::new(0.25, 0.0, 0.5, 1.0);
        assert!(!active(&project, 1920, 1080));
        project.crop = RectF::new(0.0, 0.0, 1.0, 1.0);
        // Another shape does reframe: 9:20 into 9:16 pans up and down by what the shapes differ.
        project.aspect = "9:16".into();
        assert!(active(&project, 1080, 2400));
        let (w, h) = base_size(&project, 1080, 2400);
        assert!(w == 1.0 && (h - 0.45 / 0.5625).abs() < 1e-12, "{w} {h}");
        assert!(active(&project, 1920, 1080));
        project.reframe.clear();
        assert!(!active(&project, 1920, 1080));

        // No window at all is built, so the composition draws exactly as without reframing.
        project.reframe = "follow".into();
        project.aspect = "auto".into();
        project.zooms = vec![ZoomSegment { id: "z1".into(), start: 0.2, end: 0.8, ..ZoomSegment::default() }];
        let events = [InputEvent { kind: InputKind::Move, time: 0.0, position: PointF::new(1800.0, 100.0), ..InputEvent::default() }];
        let track = MotionTrack::build(1.0, 1920, 1080, &events, &project, 60.0);
        assert_eq!(track.reframe(0.5), None);
        project.reframe.clear();
        assert_eq!(track, MotionTrack::build(1.0, 1920, 1080, &events, &project, 60.0));
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
            let zoom = ZoomFocus { center: PointF::new(0.7, 0.4), follow_cursor: false };
            focus.push(Focus { cursor: PointF::new(0.2, 0.5), zoom_scale: scale, zoom: Some(zoom) });
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
                let zoom = (i == 4).then_some(ZoomFocus { center: PointF::new(0.95, 0.9), follow_cursor: true });
                focus.push(Focus { cursor: PointF::new(x, y), zoom_scale: if i == 4 { 2.5 } else { 1.0 }, zoom });
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

    fn event(kind: InputKind, time: f64, x: f64, y: f64) -> InputEvent {
        InputEvent { kind, time, position: PointF::new(x, y), ..InputEvent::default() }
    }

    fn portrait_project(duration: f64, zoom: ZoomSegment) -> Project {
        let mut project = Project::defaults("reframe", duration);
        project.aspect = "9:16".into();
        project.reframe = "follow".into();
        project.zooms = vec![zoom];
        project
    }

    fn holds(window: &ReframeWindow, x: f64, y: f64) -> bool {
        x >= window.x && x <= window.x + window.w && y >= window.y && y <= window.y + window.h
    }

    // Regression: while zoomed the window centred on the zoom's centre, which zoom_timeline keeps
    // 0.25..0.75 for a full-screen 2x zoom. The 9:16 window is about 0.16 wide then, so a click
    // at x 0.95, 0.85 or 0.05 was out of frame for the whole zoom.
    #[test]
    fn a_zoom_near_a_side_edge_frames_the_click() {
        for &click_x in &[0.05, 0.5, 0.85, 0.95] {
            let zoom = ZoomSegment { id: "z1".into(), start: 1.7, end: 5.0, level: 2.0, ..ZoomSegment::default() };
            let project = portrait_project(6.0, zoom);
            let (x, y) = (click_x * 1920.0, 545.0);
            let mut events = vec![event(InputKind::Move, 0.0, 960.0, 540.0)];
            for i in 1..=100 {
                let t = f64::from(i) / 100.0;
                events.push(event(InputKind::Move, t, 960.0 + (x - 960.0) * t, 540.0));
            }
            events.push(event(InputKind::Move, 1.5, x, y));
            events.push(event(InputKind::ButtonDown, 2.0, x, y));
            events.push(event(InputKind::ButtonUp, 2.05, x, y));
            let track = MotionTrack::build(6.0, 1920, 1080, &events, &project, 60.0);
            for i in 0..=((5.0 - 1.7) * RATE) as i32 {
                let t = 1.7 + f64::from(i) / RATE;
                let window = track.reframe(t).expect("reframed");
                assert!(inside(&window), "{click_x} at {t}: {window:?}");
                assert!(holds(&window, click_x, y / 1080.0), "click {click_x} out of frame at {t}: {window:?}");
            }
            // Settled, the click is as near the middle as the screen's edge allows.
            let window = track.reframe(4.5).unwrap();
            let expected = clamp_center(click_x, window.w);
            assert!((window.center().x - expected).abs() < 2e-3, "{click_x}: {window:?} vs {expected}");
            assert!(window.w < 0.17, "zoomed in: {window:?}");
        }
    }

    // A fixed target near an edge is framed as set, not where a full-screen zoom would clamp it.
    #[test]
    fn a_fixed_target_near_an_edge_is_framed() {
        let zoom = ZoomSegment {
            id: "z1".into(),
            start: 1.0,
            end: 4.0,
            level: 2.0,
            automatic_target: false,
            target: PointF::new(0.93, 0.5),
        };
        let project = portrait_project(4.0, zoom);
        let events = [event(InputKind::Move, 0.0, 300.0, 540.0)];
        let track = MotionTrack::build(4.0, 1920, 1080, &events, &project, 60.0);
        let window = track.reframe(3.5).unwrap();
        assert!(holds(&window, 0.93, 0.5), "{window:?}");
        assert!((window.center().x - clamp_center(0.93, window.w)).abs() < 2e-3, "{window:?}");
    }

    // An automatic zoom's group of positions may be wider than the window (up to 0.5 / level of
    // the screen): the window follows the cursor across it instead of losing it.
    #[test]
    fn a_zoom_wider_than_the_window_keeps_the_cursor_in_frame() {
        let zoom = ZoomSegment { id: "z1".into(), start: 0.5, end: 4.5, level: 2.0, ..ZoomSegment::default() };
        let project = portrait_project(5.0, zoom);
        // Back and forth across 0.3..0.54 of the width (one group at 2x) for the whole zoom.
        let mut events = Vec::new();
        for i in 0..=500 {
            let t = f64::from(i) / 100.0;
            let x = 0.42 + 0.12 * (t * 2.0).sin();
            events.push(event(InputKind::Move, t, x * 1920.0, 540.0));
        }
        let track = MotionTrack::build(5.0, 1920, 1080, &events, &project, 60.0);
        for i in 0..=(4 * RATE as i32) {
            let t = 0.5 + f64::from(i) / RATE;
            let window = track.reframe(t).unwrap();
            let cursor = track.sample(t);
            assert!(t < 1.5 || window.w < 0.17, "{t}: {window:?}");
            assert!(holds(&window, cursor.cursor_x, cursor.cursor_y), "cursor out of frame at {t}: {window:?} {cursor:?}");
        }
    }

    // Regression: after an aspect or padding change the editor drew the old track's window into
    // the new frame (stretched) until the rebuild finished. The window is refitted to the new
    // shape around the same centre, or made up when there is no track yet.
    #[test]
    fn refit_and_provisional_windows_have_the_new_shape() {
        let window = ReframeWindow { x: 0.6, y: 0.0, w: PORTRAIT.0, h: 1.0 };
        assert_eq!(refit(window, PORTRAIT, PORTRAIT), window);
        let square = refit(window, PORTRAIT, (0.5625, 1.0));
        assert!((square.w - 0.5625).abs() < 1e-12 && square.h == 1.0, "{square:?}");
        assert!(inside(&square) && (square.x + square.w - 1.0).abs() < 1e-12, "{square:?}");
        // Zoomed windows keep their zoom.
        let zoomed = ReframeWindow { x: 0.4, y: 0.25, w: PORTRAIT.0 / 2.0, h: 0.5 };
        let refitted = refit(zoomed, PORTRAIT, (0.5625, 1.0));
        assert!((refitted.w - 0.28125).abs() < 1e-12 && refitted.h == 0.5);
        assert!((refitted.center().x - zoomed.center().x).abs() < 1e-12);
        let made_up = provisional(PointF::new(0.98, 0.5), 2.0, PORTRAIT);
        assert!((made_up.w - PORTRAIT.0 / 2.0).abs() < 1e-12 && made_up.h == 0.5 && inside(&made_up));
        assert!((made_up.x + made_up.w - 1.0).abs() < 1e-12);
    }
}
