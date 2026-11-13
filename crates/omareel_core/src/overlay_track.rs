//! Which overlays show at a moment of the output, and how strongly. The editor's preview and
//! the exporter both ask this, with the same times.
//!
//! A blur region is a privacy feature, so it errs early and late: both pad its span (in the
//! recording's time) by a margin, because the frame on screen is never exactly the one the
//! timeline names. The exporter's decoder picks the nearest recording frame on its own output
//! grid, up to a frame or two (times the clip's speed) either way, and a recording frame shows
//! the screen until the next one; the preview's video presents frames on its own, ahead of the
//! overlay update that follows them. See [`export_blur_pad`] and [`PREVIEW_BLUR_PAD`].

use crate::compat::qjson::{Object, Value};
use crate::export::redact::block_fraction;
use crate::project::Overlay;

/// How much earlier and later than its span the preview shows a blur region, in seconds of
/// output (times the clip's speed for the recording's time): three frames at 60 Hz.
pub const PREVIEW_BLUR_PAD: f64 = 0.05;

/// How much earlier and later than its span the exporter blurs, in seconds of the recording:
/// a frame of the recording (which may straddle the span's start) and two of the export
/// times the clip's `speed` (the decoder's pick of the nearest frame on a grid that starts
/// up to a frame after the clip does). The recording's frame interval is taken as 1/60 s
/// at the least and 1/10 s at most, whatever its stated rate.
pub fn export_blur_pad(recording_fps: f64, export_fps: f64, speed: f64) -> f64 {
    let interval = if recording_fps > 0.0 { (1.0 / recording_fps).clamp(1.0 / 60.0, 0.1) } else { 0.1 };
    let speed = if speed > 0.0 { speed } else { 1.0 };
    interval + 2.0 * speed / export_fps.max(1.0) + 1e-6
}

/// The overlays in effect at `output_time` (whose recording time is `source_time`), each with
/// its opacity (0 at the very start of a fade-in), in the project's order (later items draw on
/// top). Screen-anchored items go by `source_time` and only while the recording shows
/// (`recording_shown` is false during an inserted video); frame-anchored items go by
/// `output_time`. Blur items show from `blur_pad` seconds (of the recording) before their
/// span to as long after it, always fully.
pub fn sample(overlays: &[Overlay], source_time: f64, output_time: f64, recording_shown: bool, blur_pad: f64) -> Vec<(usize, f64)> {
    sample_by(overlays, source_time, output_time, recording_shown, &|overlay| {
        recording_shown && blur_near(overlay, source_time, blur_pad)
    })
}

/// [`sample`] with blur items in effect when `blur_shown` says (the preview looks either side
/// of the playhead, across cuts).
pub fn sample_by(
    overlays: &[Overlay],
    source_time: f64,
    output_time: f64,
    recording_shown: bool,
    blur_shown: &dyn Fn(&Overlay) -> bool,
) -> Vec<(usize, f64)> {
    overlays
        .iter()
        .enumerate()
        .filter_map(|(index, overlay)| {
            if overlay.kind == "blur" {
                return blur_shown(overlay).then_some((index, 1.0));
            }
            let time = if overlay.is_screen_anchored() { source_time } else { output_time };
            let shown = recording_shown || !overlay.is_screen_anchored();
            (shown && time >= overlay.start && time < overlay.end).then(|| (index, overlay.opacity_at(time)))
        })
        .collect()
}

/// [`sample`] as the maps the composition draws: each item's JSON with its `opacity`, and for
/// a blur region its cell size `block` as a fraction of the recording's height
/// ([`block_fraction`]).
pub fn sample_json(overlays: &[Overlay], source_time: f64, output_time: f64, recording_shown: bool, blur_pad: f64) -> Vec<Object> {
    to_json(overlays, sample(overlays, source_time, output_time, recording_shown, blur_pad))
}

/// The JSON of `sampled` items ([`sample_json`]).
pub fn to_json(overlays: &[Overlay], sampled: Vec<(usize, f64)>) -> Vec<Object> {
    sampled
        .into_iter()
        .map(|(index, opacity)| {
            let overlay = &overlays[index];
            let mut json = overlay.to_json();
            json.insert("opacity", Value::from(opacity));
            if overlay.kind == "blur" {
                json.insert("block", Value::from(block_fraction(overlay.amount, overlay.rect.height)));
            }
            json
        })
        .collect()
}

/// Whether a blur item's span, widened by `pad` either way, takes in `source_time`.
pub fn blur_near(overlay: &Overlay, source_time: f64, pad: f64) -> bool {
    source_time >= overlay.start - pad && source_time < overlay.end + pad
}

/// The blur regions the exporter bakes into a recording frame that the timeline puts at
/// `source_time`: those whose span, widened by `pad` ([`export_blur_pad`]) either way, takes
/// it in.
pub fn redactions_near(overlays: &[Overlay], source_time: f64, pad: f64) -> Vec<&Overlay> {
    overlays.iter().filter(|overlay| overlay.kind == "blur" && blur_near(overlay, source_time, pad)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay(kind: &str, start: f64, end: f64) -> Overlay {
        Overlay { id: kind.into(), start, end, ..Overlay::new_of_kind(kind).unwrap() }
    }

    #[test]
    fn screen_items_follow_source_time_and_hide_during_inserts() {
        let items = vec![overlay("box", 2.0, 4.0), overlay("text", 0.0, 1.0)];
        assert_eq!(sample(&items, 3.0, 0.5, true, 0.0).len(), 2);
        assert_eq!(sample(&items, 3.0, 0.5, false, 0.0), vec![(1, 1.0)]);
        assert!(sample(&items, 5.0, 2.0, true, 0.0).is_empty());
        // At the start of a fade-in it is in effect, unseen.
        assert_eq!(sample(&items, 2.0, 1.0, true, 0.0), vec![(0, 0.0)]);
    }

    #[test]
    fn fades_ramp_but_blur_never_fades() {
        let mut text = overlay("text", 1.0, 3.0);
        text.fade_in = 0.5;
        text.fade_out = 0.5;
        assert!((text.opacity_at(1.25) - 0.5).abs() < 1e-9);
        assert!((text.opacity_at(2.0) - 1.0).abs() < 1e-9);
        assert!((text.opacity_at(2.75) - 0.5).abs() < 1e-9);
        assert_eq!(text.opacity_at(3.0), 0.0);
        let blur = overlay("blur", 1.0, 3.0);
        assert_eq!(blur.opacity_at(1.0), 1.0);
        assert_eq!(blur.opacity_at(2.999), 1.0);
        assert_eq!(redactions_near(&[blur.clone()], 0.99, 0.0).len(), 0);
        assert_eq!(redactions_near(&[blur], 1.0, 0.0).len(), 1);
    }

    #[test]
    fn blur_is_padded_both_ways() {
        let items = vec![overlay("blur", 2.0, 4.0), overlay("text", 2.0, 4.0)];
        // The preview's pad: the blur shows early and late, fully; the text doesn't.
        assert_eq!(sample(&items, 1.96, 0.0, true, 0.05), vec![(0, 1.0)]);
        assert_eq!(sample(&items, 4.04, 0.0, true, 0.05), vec![(0, 1.0)]);
        assert!(sample(&items, 1.94, 0.0, true, 0.05).is_empty());
        // Hidden during an inserted video all the same.
        assert!(sample(&items, 3.0, 0.0, false, 0.05).is_empty());
        let json = sample_json(&items, 3.0, 3.0, true, 0.0);
        assert!(json[0].value("block").to_double(0.0) >= 0.04);
        assert!(!json[1].contains("block"));
        // The exporter's: a recording frame and two export frames at the clip's speed.
        let pad = export_blur_pad(60.0, 60.0, 1.0);
        assert!((pad - 0.05).abs() < 1e-5);
        assert!((export_blur_pad(60.0, 30.0, 2.0) - (1.0 / 60.0 + 4.0 / 30.0)).abs() < 1e-5);
        assert!((export_blur_pad(0.0, 60.0, 1.0) - (0.1 + 2.0 / 60.0)).abs() < 1e-5);
        assert!((export_blur_pad(1000.0, 60.0, 1.0) - 0.05).abs() < 1e-5);
        assert_eq!(redactions_near(&items, 2.0 - pad + 1e-4, pad).len(), 1);
        assert_eq!(redactions_near(&items, 4.0 + pad - 1e-4, pad).len(), 1);
        assert!(redactions_near(&items, 4.0 + pad + 1e-4, pad).is_empty());
    }
}
