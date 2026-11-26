//! Reading and editing a recording without the editor's window, for AI agents and scripts:
//! `omareel describe` ([`describe`]) summarises the edited timeline as JSON, and `omareel apply`
//! ([`apply`]) runs a list of edit operations through the editor's own [`Document`], so every
//! edit is checked the way the editor checks it and `project.json` stays valid. An editor that
//! has the bundle open picks the new `project.json` up as one undoable change
//! ([`Document::reload_if_changed_on_disk`]).
//!
//! Times: "source" seconds are times in `screen.mp4`; "output" seconds are times in the edited
//! video, after cuts, speed changes and inserted clips.
//!
//! [`Document`]: crate::editor::Document
//! [`Document::reload_if_changed_on_disk`]: crate::editor::Document::reload_if_changed_on_disk

pub mod apply;
pub mod describe;
pub mod skill;
#[cfg(test)]
mod tests;

use crate::editor::{Document, Probes};
use crate::project::Clip;
use std::sync::Arc;

/// Rounds to the millisecond, for numbers meant to be read.
pub(crate) fn ms(t: f64) -> f64 {
    if t.is_finite() { (t * 1000.0).round() / 1000.0 } else { t }
}

/// Opens a bundle the way the editor does, without starting any of its background work or
/// writing anything (a project.json that needs migrating is migrated in memory).
pub fn open_document(bundle: &str) -> Result<Document, String> {
    if !std::path::Path::new(bundle).is_dir() {
        return Err(format!("bundle does not exist: {bundle}"));
    }
    let probes = Probes::new(Arc::new(crate::media_probe::probe_video));
    let document = Document::open_read_only(bundle, &probes)?;
    // Nothing runs the effects the editor would perform (players, timers).
    document.take_effects();
    Ok(document)
}

/// The output spans in which the recording's `[start, end)` plays, in output order. A span cut
/// out of the timeline has none; a span shown by two clips has two.
pub fn source_span_outputs(clips: &[Clip], start: f64, end: f64) -> Vec<(f64, f64)> {
    let mut spans = Vec::new();
    let mut cursor = 0.0;
    for clip in clips {
        let length = (clip.out - clip.in_) / clip.speed;
        if !clip.is_media() {
            let from = start.max(clip.in_);
            let to = end.min(clip.out);
            if to > from {
                spans.push((cursor + (from - clip.in_) / clip.speed, cursor + (to - clip.in_) / clip.speed));
            }
        }
        cursor += length;
    }
    spans
}

/// The output times at which the recording's source time `time` plays (none when it was cut).
pub fn source_time_outputs(clips: &[Clip], time: f64) -> Vec<f64> {
    let mut times = Vec::new();
    let mut cursor = 0.0;
    for clip in clips {
        let length = (clip.out - clip.in_) / clip.speed;
        if !clip.is_media() && time >= clip.in_ && time < clip.out {
            times.push(cursor + (time - clip.in_) / clip.speed);
        }
        cursor += length;
    }
    times
}

#[cfg(test)]
mod mapping_tests {
    use super::*;

    fn clip(id: &str, in_: f64, out: f64, speed: f64) -> Clip {
        Clip { id: id.into(), in_, out, speed, ..Clip::default() }
    }

    #[test]
    fn maps_source_spans_through_cuts_and_speed() {
        // 0–4 at 1×, a cut 4–6, 6–10 at 2×, an inserted 3 s video, 10–12 at 1×.
        let clips = vec![
            clip("a", 0.0, 4.0, 1.0),
            clip("b", 6.0, 10.0, 2.0),
            Clip { id: "m".into(), in_: 0.0, out: 3.0, media: "media/x.mp4".into(), ..Clip::default() },
            clip("c", 10.0, 12.0, 1.0),
        ];
        assert_eq!(source_span_outputs(&clips, 3.0, 7.0), vec![(3.0, 4.0), (4.0, 4.5)]);
        assert_eq!(source_span_outputs(&clips, 4.5, 5.5), vec![]);
        assert_eq!(source_span_outputs(&clips, 9.0, 11.0), vec![(5.5, 6.0), (9.0, 10.0)]);
        assert_eq!(source_time_outputs(&clips, 5.0), Vec::<f64>::new());
        assert_eq!(source_time_outputs(&clips, 8.0), vec![5.0]);
        assert_eq!(source_time_outputs(&clips, 10.0), vec![9.0]);
    }
}
