//! Clip edits (`splitAtPlayhead`, `moveClip`, `trimClip`, `removeClip`, `deleteOutputRange`,
//! `mergeClip`, `resetClipTrims`, `setClipSpeed`, `setClipLayout`, `setClipFit`, `reorderClip`)
//! and inserting a video (`insertMedia`, `finishImport`).

use super::bundle::{import_insert, ImportedInsert, ProbeVideo, VideoInfo};
use super::{Document, Effect};
use crate::clip_timeline::ClipTimeline;
use crate::compat::cpp;
use crate::compat::qdir;
use crate::compat::qround::q_fuzzy_compare;
use crate::project::{allowed_clip_fits, allowed_clip_layouts, allowed_clip_speeds, Clip, ZoomSegment};

/// What `insertMedia` runs off the GUI thread: [`import_insert`].
#[derive(Clone)]
pub struct ImportJob {
    pub source: String,
    pub bundle_path: String,
    pub probe_video: ProbeVideo,
}

impl ImportJob {
    pub fn run(&self) -> ImportedInsert {
        import_insert(&self.source, &self.bundle_path, &*self.probe_video)
    }
}

/// `visibleSourceSpan` in `deleteOutputRange`: the first and last recording time of a zoom
/// that some recording clip still shows (infinities when none does).
fn visible_source_span(zoom: &ZoomSegment, clips: &[Clip]) -> (f64, f64) {
    let mut first = f64::INFINITY;
    let mut last = f64::NEG_INFINITY;
    for clip in clips {
        if clip.is_media() {
            continue;
        }
        let start = cpp::max(zoom.start, clip.in_);
        let end = cpp::min(zoom.end, clip.out);
        if end - start <= 1e-9 {
            continue;
        }
        first = cpp::min(first, start);
        last = cpp::max(last, end);
    }
    (first, last)
}

impl Document {
    /// `insertInfo(path)`: what `probeVideo` says of an inserted video, asked once.
    pub(crate) fn insert_info(&mut self, path: &str) -> VideoInfo {
        if let Some(info) = self.inserts.get(path) {
            return *info;
        }
        let info = (self.probe_video)(&qdir::file_path(&self.recording.bundle_path, path)).unwrap_or_default();
        self.inserts.insert(path.to_owned(), info);
        info
    }

    /// `clipBounds(index)`: how far a clip's in and out may move, between its recording
    /// neighbours, or within the inserted video (its out when the video did not probe).
    pub(crate) fn clip_bounds(&mut self, index: i32) -> (f64, f64) {
        let clip = self.project.clips[index as usize].clone();
        if !clip.is_media() {
            return self.timeline().recording_bounds(index, self.recording.source_duration);
        }
        let info = self.insert_info(&clip.media);
        (0.0, if info.is_valid() { info.duration } else { clip.out })
    }

    /// `splitAtPlayhead()`: the clip at the playhead in two, unless that is within 0.1 s of its
    /// ends; the right half gets a new id and the selection.
    pub fn split_at_playhead(&mut self) -> bool {
        let (index, start) = self.clip_for_output(self.output_position);
        if index < 0 {
            return false;
        }
        let i = index as usize;
        let clip = self.project.clips[i].clone();
        let source = clip.in_ + (self.output_position - start) * clip.speed;
        if source - clip.in_ < 0.1 || clip.out - source < 0.1 {
            return false;
        }
        self.snapshot("");
        let mut left = clip.clone();
        let mut right = clip;
        left.out = source;
        right.in_ = source;
        right.id = self.next_id("c");
        self.selected_clip_id = right.id.clone();
        self.project.clips[i] = left;
        self.project.clips.insert(i + 1, right);
        self.changed(false);
        self.emit(Effect::SelectionChanged);
        true
    }

    /// `moveClip(id, sourceIn)`: slides a clip's span within its bounds, keeping its length.
    /// No movement is still success, without an undo step.
    pub fn move_clip(&mut self, id: &str, source_in: f64) -> bool {
        let index = self.clip_index(id);
        if index < 0 {
            return false;
        }
        let i = index as usize;
        let length = self.project.clips[i].out - self.project.clips[i].in_;
        let (lower, upper) = self.clip_bounds(index);
        let upper = upper - length;
        let next_in = cpp::clamp(source_in, lower, cpp::max(lower, upper));
        if q_fuzzy_compare(next_in + 1.0, self.project.clips[i].in_ + 1.0) {
            return true;
        }
        let key = self.history.key_or(&format!("move-clip-{id}"));
        self.snapshot(&key);
        self.project.clips[i].in_ = next_in;
        self.project.clips[i].out = next_in + length;
        self.changed(false);
        true
    }

    /// `trimClip(id, newIn, newOut)`: both ends clamped to the clip's bounds; under 0.1 s is
    /// refused. Trims of one clip coalesce (key `trim-<id>`) whatever coalesced edit is open.
    pub fn trim_clip(&mut self, id: &str, new_in: f64, new_out: f64) -> bool {
        let index = self.clip_index(id);
        if index < 0 {
            return false;
        }
        let (lower, upper) = self.clip_bounds(index);
        let new_in = cpp::clamp(new_in, lower, upper);
        let new_out = cpp::clamp(new_out, lower, upper);
        if new_out - new_in < 0.1 {
            return false;
        }
        self.snapshot(&format!("trim-{id}"));
        let clip = &mut self.project.clips[index as usize];
        clip.in_ = new_in;
        clip.out = new_out;
        self.changed(false);
        true
    }

    /// `removeClip(id)`: never the last clip, nor the last recording clip; the clip now in its
    /// place is selected.
    pub fn remove_clip(&mut self, id: &str) -> bool {
        let index = self.clip_index(id);
        if index < 0 || self.project.clips.len() <= 1 {
            return false;
        }
        let i = index as usize;
        // The recording always keeps at least one clip.
        if !self.project.clips[i].is_media() && self.project.clips.iter().filter(|clip| !clip.is_media()).count() <= 1 {
            return false;
        }
        self.snapshot("");
        self.project.clips.remove(i);
        self.selected_clip_id = self.project.clips[i.min(self.project.clips.len() - 1)].id.clone();
        self.emit(Effect::Seek(cpp::min(self.output_position, self.duration())));
        self.changed(false);
        self.emit(Effect::SelectionChanged);
        true
    }

    /// `deleteOutputRange(from, to)`: cuts an output span out of the clips (the right part of a
    /// clip cut in the middle gets a new id), as long as some recording is left. Zooms that no
    /// longer show are removed and the others shrink to what still shows; the clip at the cut is
    /// selected and the playhead goes there. The new id is drawn before anything is checked, as
    /// the C++ drew its UUID.
    pub fn delete_output_range(&mut self, from: f64, to: f64) -> bool {
        if !from.is_finite() || !to.is_finite() {
            return false;
        }
        let (from, to) = if from > to { (to, from) } else { (from, to) };
        let from = cpp::clamp(from, 0.0, self.duration());
        let to = cpp::clamp(to, 0.0, self.duration());
        let mut edited = ClipTimeline::new(self.project.clips.clone());
        let remainder_id = self.next_id("c");
        if !edited.delete_range(from, to, &remainder_id) {
            return false;
        }
        if edited.clips().iter().all(Clip::is_media) {
            return false;
        }
        self.emit(Effect::Pause);
        self.snapshot("");
        let mut surviving = Vec::new();
        let mut zooms_changed = false;
        for zoom in &self.project.zooms {
            let mut zoom = zoom.clone();
            let before = visible_source_span(&zoom, &self.project.clips);
            let after = visible_source_span(&zoom, edited.clips());
            if before.0.is_finite() && !after.0.is_finite() {
                zooms_changed = true;
                continue;
            }
            if before.0.is_finite() && before != after {
                zoom.start = after.0;
                zoom.end = after.1;
                zooms_changed = true;
            }
            surviving.push(zoom);
        }
        self.project.clips = edited.clips().to_vec();
        if zooms_changed {
            self.project.zooms = surviving;
        }
        let at = cpp::min(from, self.duration());
        self.selected_clip_id = self.project.clips[self.clip_for_output(at).0 as usize].id.clone();
        self.selected_zoom_id.clear();
        self.selected_zoom_ids.clear();
        self.changed(zooms_changed);
        self.emit(Effect::Seek(cpp::min(from, self.duration())));
        self.emit(Effect::SelectionChanged);
        true
    }

    /// `mergeClip(id, direction)`: joins a recording clip with its neighbour (the earlier one
    /// takes the later one's out).
    pub fn merge_clip(&mut self, id: &str, direction: i32) -> bool {
        let index = self.clip_index(id);
        let other = index + if direction < 0 { -1 } else { 1 };
        if index < 0 || other < 0 || other as usize >= self.project.clips.len() {
            return false;
        }
        // Only pieces of the recording join; an inserted clip has nothing to join across.
        if self.project.clips[index as usize].is_media() || self.project.clips[other as usize].is_media() {
            return false;
        }
        self.snapshot("");
        let first = index.min(other) as usize;
        let second = index.max(other) as usize;
        self.project.clips[first].out = self.project.clips[second].out;
        self.project.clips.remove(second);
        self.selected_clip_id = self.project.clips[first].id.clone();
        self.changed(false);
        self.emit(Effect::SelectionChanged);
        true
    }

    /// `resetClipTrims(id)`: the clip spans its whole bounds again.
    pub fn reset_clip_trims(&mut self, id: &str) -> bool {
        let index = self.clip_index(id);
        if index < 0 {
            return false;
        }
        let (new_in, new_out) = self.clip_bounds(index);
        if new_out - new_in < 0.1 {
            return false;
        }
        self.snapshot("");
        let clip = &mut self.project.clips[index as usize];
        clip.in_ = new_in;
        clip.out = new_out;
        self.changed(false);
        true
    }

    /// `setClipSpeed(id, speed)`: one of the allowed speeds, exactly.
    pub fn set_clip_speed(&mut self, id: &str, speed: f64) -> bool {
        let index = self.clip_index(id);
        if index < 0 || !allowed_clip_speeds().contains(&speed) {
            return false;
        }
        self.snapshot("");
        self.project.clips[index as usize].speed = speed;
        self.changed(false);
        self.emit(Effect::Seek(cpp::min(self.output_position, self.duration())));
        true
    }

    /// `setClipLayout(id, layout)`: recording clips only; the same layout is success without an
    /// undo step. The camera is synced at once, since its tolerance depends on the layout.
    pub fn set_clip_layout(&mut self, id: &str, layout: &str) -> bool {
        let index = self.clip_index(id);
        if index < 0 || !allowed_clip_layouts().contains(&layout) || self.project.clips[index as usize].is_media() {
            return false;
        }
        if self.project.clips[index as usize].layout == layout {
            return true;
        }
        self.snapshot("");
        self.project.clips[index as usize].layout = layout.to_owned();
        self.changed(false);
        // The camera now has to keep up with a full-frame layout, or may fall back to a bubble.
        self.emit(Effect::SyncCamera { screen_source_time: self.source_position(), force: true });
        true
    }

    /// `setClipFit(id, fit)`: inserted clips only.
    pub fn set_clip_fit(&mut self, id: &str, fit: &str) -> bool {
        let index = self.clip_index(id);
        if index < 0 || !self.project.clips[index as usize].is_media() || !allowed_clip_fits().contains(&fit) {
            return false;
        }
        if self.project.clips[index as usize].fit == fit {
            return true;
        }
        self.snapshot("");
        self.project.clips[index as usize].fit = fit.to_owned();
        self.changed(false);
        true
    }

    /// `reorderClip(id, direction)`: moves an inserted clip one place earlier (-1) or later; the
    /// playhead goes to its new start.
    pub fn reorder_clip(&mut self, id: &str, direction: i32) -> bool {
        let index = self.clip_index(id);
        let other = index + if direction < 0 { -1 } else { 1 };
        // Recording clips keep their order; only an inserted clip moves past its neighbour.
        if index < 0 || other < 0 || other as usize >= self.project.clips.len() || !self.project.clips[index as usize].is_media() {
            return false;
        }
        self.emit(Effect::Pause);
        self.snapshot("");
        self.project.clips.swap(index as usize, other as usize);
        self.selected_clip_id = id.to_owned();
        self.changed(false);
        let start = self.timeline().output_time(other, self.project.clips[other as usize].in_);
        self.emit(Effect::Seek(start));
        self.emit(Effect::SelectionChanged);
        true
    }

    /// `insertMedia(url)` with the URL already a path (`isLocalFile() ? toLocalFile() :
    /// toString()`): pauses and starts the import, unless one is running.
    pub fn insert_media(&mut self, source: &str) {
        if self.importing || source.is_empty() {
            return;
        }
        self.emit(Effect::Pause);
        self.importing = true;
        self.import_error.clear();
        self.emit(Effect::ImportingChanged);
        self.emit(Effect::StartImport(source.to_owned()));
    }

    /// The work [`Effect::StartImport`] asks for.
    pub fn import_job(&self, source: &str) -> ImportJob {
        ImportJob {
            source: source.to_owned(),
            bundle_path: self.recording.bundle_path.clone(),
            probe_video: self.probe_video.clone(),
        }
    }

    pub fn clear_import_error(&mut self) {
        if self.import_error.is_empty() {
            return;
        }
        self.import_error.clear();
        self.emit(Effect::ImportingChanged);
    }

    /// `finishImport()`: the new clip goes at the playhead: before a clip it starts (within
    /// 0.1 s), after one it ends, or between the two halves of the clip it splits. It is
    /// selected and the playhead goes to its start.
    pub fn finish_import(&mut self, imported: ImportedInsert) {
        self.importing = false;
        if !imported.error.is_empty() {
            self.import_error = imported.error;
            self.emit(Effect::ImportingChanged);
            return;
        }
        self.inserts.insert(imported.path.clone(), imported.info);
        let at = cpp::clamp(self.output_position, 0.0, self.duration());
        let (index, clip_start) = self.clip_for_output(at);
        self.snapshot("");
        let mut insert_at = self.project.clips.len();
        if index >= 0 {
            let i = index as usize;
            let clip = self.project.clips[i].clone();
            let source = clip.in_ + (at - clip_start) * clip.speed;
            if source - clip.in_ < 0.1 {
                insert_at = i;
            } else {
                insert_at = i + 1;
                if clip.out - source >= 0.1 {
                    let mut left = clip.clone();
                    let mut right = clip;
                    left.out = source;
                    right.in_ = source;
                    right.id = self.next_id("c");
                    self.project.clips[i] = left;
                    self.project.clips.insert(i + 1, right);
                }
            }
        }
        let insert = Clip {
            id: self.next_id("m"),
            in_: 0.0,
            out: imported.info.duration,
            speed: 1.0,
            media: imported.path,
            ..Clip::default()
        };
        self.selected_clip_id = insert.id.clone();
        self.project.clips.insert(insert_at, insert);
        self.selected_zoom_id.clear();
        self.selected_zoom_ids.clear();
        self.changed(false);
        self.emit(Effect::Seek(self.timeline().output_time(insert_at as i32, 0.0)));
        self.emit(Effect::SelectionChanged);
        self.emit(Effect::ImportingChanged);
    }
}
