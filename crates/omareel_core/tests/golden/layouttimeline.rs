//! layouttimeline: `layout_timeline::sample_layout` against `Omareel::sampleLayout` (README
//! "layouttimeline"). Defined on x86_64 only: the transition eases through glibc's `pow`.

use crate::cliptimeline::clips_from_json;
use crate::support::*;
use omareel_core::compat::cpp;
use omareel_core::layout_timeline::{DEFAULT_TRANSITION, sample_layout};

/// A call's arguments; `None` is the call without a transition (the default, 0.5).
struct Variant {
    camera: bool,
    bubble: bool,
    transition: Option<f64>,
}

#[test]
fn sample_txt() {
    let timelines = read_json("layouttimeline/cases.json").get("timelines").to_object();
    let mut variants = vec![Variant { camera: false, bubble: true, transition: None }];
    for bubble in [false, true] {
        variants.push(Variant { camera: true, bubble, transition: None });
        for transition in [0.0, 0.25, 1.5] {
            variants.push(Variant { camera: true, bubble, transition: Some(transition) });
        }
    }
    let mut out = Text::new();
    for (name, clips) in timelines.iter() {
        let clips = clips_from_json(clips);
        // Clip starts as sampleLayout accumulates them: lengths are clamped at 0.
        let mut starts = vec![0.0];
        for clip in &clips {
            let last = *starts.last().expect("a start");
            starts.push(last + cpp::max(0.0, (clip.out - clip.in_) / cpp::max(0.000001, clip.speed)));
        }
        let end = *starts.last().expect("a start");
        for v in &variants {
            let transition = v.transition.unwrap_or(DEFAULT_TRANSITION);
            out.section(&format!(
                "{name} cameraAvailable={} bubbleEnabled={} transition={}",
                boolean(v.camera),
                boolean(v.bubble),
                v.transition.map_or_else(|| "default".to_owned(), num)
            ));
            let mut times = uniform(20, -10, (20.0 * cpp::min(end, 12.0)).ceil() as i32 + 10);
            for &s in &starts {
                let eased = s + transition;
                times.extend([before(s), s, after(s), before(eased), eased, after(eased)]);
            }
            let mut times = sorted_unique(times);
            times.extend([f64::NEG_INFINITY, f64::INFINITY, f64::NAN]);
            for t in times {
                let sample = sample_layout(&clips, t, v.camera, v.bubble, transition);
                out.row([num(t), num(sample.camera_full), num(sample.bubble)]);
            }
        }
    }
    assert_golden("layouttimeline/sample.txt", &out.into_string());
}
