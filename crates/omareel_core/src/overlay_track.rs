//! Which overlays show at a moment of the output, and how strongly. The editor's preview and
//! the exporter both ask this, with the same times, so a blur region covers exactly the same
//! frames in both.

use crate::compat::qjson::{Object, Value};
use crate::export::redact::block_fraction;
use crate::project::Overlay;

/// The overlays in effect at `output_time` (whose recording time is `source_time`), each with
/// its opacity (0 at the very start of a fade-in), in the project's order (later items draw on
/// top). Screen-anchored items go by `source_time` and only while the recording shows
/// (`recording_shown` is false during an inserted video); frame-anchored items go by
/// `output_time`.
pub fn sample(overlays: &[Overlay], source_time: f64, output_time: f64, recording_shown: bool) -> Vec<(usize, f64)> {
    overlays
        .iter()
        .enumerate()
        .filter_map(|(index, overlay)| {
            let time = if overlay.is_screen_anchored() { source_time } else { output_time };
            let shown = recording_shown || !overlay.is_screen_anchored();
            (shown && time >= overlay.start && time < overlay.end).then(|| (index, overlay.opacity_at(time)))
        })
        .collect()
}

/// [`sample`] as the maps the composition draws: each item's JSON with its `opacity`, and for
/// a blur region its cell size `block` as a fraction of the recording's height
/// ([`block_fraction`]).
pub fn sample_json(overlays: &[Overlay], source_time: f64, output_time: f64, recording_shown: bool) -> Vec<Object> {
    sample(overlays, source_time, output_time, recording_shown)
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

/// The blur regions over the recording at `source_time`, which the exporter bakes into the
/// recording's pixels.
pub fn redactions_at(overlays: &[Overlay], source_time: f64) -> Vec<&Overlay> {
    overlays.iter().filter(|overlay| overlay.kind == "blur" && overlay.opacity_at(source_time) > 0.0).collect()
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
        assert_eq!(sample(&items, 3.0, 0.5, true).len(), 2);
        assert_eq!(sample(&items, 3.0, 0.5, false), vec![(1, 1.0)]);
        assert!(sample(&items, 5.0, 2.0, true).is_empty());
        // At the start of a fade-in it is in effect, unseen.
        assert_eq!(sample(&items, 2.0, 1.0, true), vec![(0, 0.0)]);
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
        assert_eq!(redactions_at(&[blur.clone()], 0.99).len(), 0);
        assert_eq!(redactions_at(&[blur], 1.0).len(), 1);
    }
}
