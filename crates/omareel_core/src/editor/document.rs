//! The document itself: the project, its history and selection, the output position and
//! playback range, and the operations on them that are not about clips, zooms or presets.

use super::bundle::{aspect_ratio, load_bundle, Bundle, CameraRecording, Probes, ProbeVideo, Recording, VideoInfo};
use super::history::History;
use super::ids::random_uuid;
use super::thumbnails::{Thumbnails, WallpaperCache};
use super::Effect;
use crate::clip_timeline::ClipTimeline;
use crate::compat::cpp;
use crate::compat::qdir;
use crate::compat::qjson::{self, Object, Value};
use crate::compat::qnumber;
use crate::compat::qround::q_round;
use crate::compat::qstandardpaths::{self, Location};
use crate::input_log::{InputEvent, InputLog};
use crate::keystroke_track::KeystrokeTrack;
use crate::motion_track::MotionTrack;
use crate::project::{Clip, Project};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// `:/omareel/assets/gradients.json`, the gradient presets, compiled in from the same file the
/// Qt resource holds.
const GRADIENTS_JSON: &str = include_str!("../../../../assets/gradients.json");

/// The gradient presets: each a list of colour names (`QJsonArray::toVariantList()` of the file;
/// empty if it does not parse).
pub fn gradient_presets() -> Vec<Value> {
    match qjson::from_json(GRADIENTS_JSON.as_bytes()) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// Whether `setProjectValue(path, ...)` takes a URL for this path: for `background.image` and
/// `background.wallpaper` the Qt side passes `value.toUrl().toLocalFile()` instead of the value
/// when `value.toUrl().isLocalFile()`, before `QJsonValue::fromVariant`.
pub fn takes_local_file(path: &str) -> bool {
    path == "background.image" || path == "background.wallpaper"
}

/// Editor.cpp's `setNested`: `value` at the dotted path, each level above it read with
/// `toObject()` (so anything but an object there becomes one), and `Undefined` removing the key.
pub fn set_nested(mut object: Object, parts: &[&str], index: usize, value: Value) -> Object {
    if index + 1 == parts.len() {
        object.insert(parts[index], value);
    } else {
        let child = object.value(parts[index]).to_object();
        object.insert(parts[index], set_nested(child, parts, index + 1, value));
    }
    object
}

/// `QVariant::toString()` of a JSON value as `toVariantList()` made it.
pub(crate) fn variant_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Double(d) => qnumber::number_f64(*d, 'g', qnumber::FLOATING_POINT_SHORTEST),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        _ => String::new(),
    }
}

/// What `rebuildMotion` hands its worker: everything `MotionTrack::build` reads.
#[derive(Clone, Debug)]
pub struct MotionJob {
    pub duration: f64,
    pub width: i32,
    pub height: i32,
    pub events: Vec<InputEvent>,
    pub project: Project,
    pub capture_fps: f64,
}

impl MotionJob {
    pub fn run(&self) -> MotionTrack {
        MotionTrack::build(self.duration, self.width, self.height, &self.events, &self.project, self.capture_fps)
    }
}

/// `QStandardPaths` locations the document uses; `None` reads them from the environment on
/// each use, as Qt does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Locations {
    /// `ConfigLocation`, where presets live (`omareel/presets`).
    pub config: Option<String>,
    /// `CacheLocation`, where filmstrips live.
    pub cache: Option<String>,
}

/// An open recording being edited (the state of `Editor` that is not Qt plumbing).
pub struct Document {
    pub(crate) recording: Recording,
    pub(crate) project_path: String,
    pub(crate) video_path: String,
    pub(crate) camera_video_path: String,
    pub(crate) project: Project,
    pub(crate) input: InputLog,
    pub(crate) keystroke_track: KeystrokeTrack,
    pub(crate) inserts: HashMap<String, VideoInfo>,
    pub(crate) probe_video: ProbeVideo,
    pub(crate) history: History,
    pub(crate) dirty: bool,
    pub(crate) selected_clip_id: String,
    pub(crate) selected_zoom_id: String,
    pub(crate) selected_zoom_ids: Vec<String>,
    pub(crate) picking_zoom_target: bool,
    pub(crate) output_position: f64,
    pub(crate) playback_range_start: f64,
    pub(crate) playback_range_end: f64,
    pub(crate) screen_frame_times: Vec<f64>,
    pub(crate) motion: Option<Arc<MotionTrack>>,
    pub(crate) motion_generation: i32,
    pub(crate) running_motion_generation: i32,
    pub(crate) motion_running: bool,
    pub(crate) importing: bool,
    pub(crate) import_error: String,
    pub(crate) thumbnails: Thumbnails,
    pub(crate) wallpapers: RefCell<WallpaperCache>,
    pub(crate) project_map_revision: u64,
    pub(crate) gradients: Vec<Value>,
    pub(crate) export: super::exporting::ExportState,
    pub(crate) config_location: Option<String>,
    pub(crate) cache_location: Option<String>,
    pub(crate) new_id: Box<dyn FnMut() -> String + Send>,
    pub(crate) effects: RefCell<Vec<Effect>>,
}

impl Document {
    /// `Editor(bundlePath)` up to `m_valid = true`: [`load_bundle`], the document, and the
    /// screen's thumbnail source. A new project queues [`Effect::StartAutosaveTimer`]. The Qt
    /// side then loads the players, starts [`Document::motion_job`],
    /// [`Document::camera_thumbnail_job`] and `probeFrameTimes` (into
    /// [`Document::set_screen_frame_times`]), and the 1.5 s loading timer.
    pub fn open(bundle_path: &str, probes: &Probes) -> Result<Document, String> {
        Document::open_with(bundle_path, probes, Locations::default())
    }

    /// [`Document::open`] with the standard locations given (for tests).
    pub fn open_with(bundle_path: &str, probes: &Probes, locations: Locations) -> Result<Document, String> {
        let Bundle { recording, project, input, inserts, new_project } = load_bundle(bundle_path, probes)?;
        let mut document = Document::new(recording, project, input, inserts, probes.video.clone());
        document.config_location = locations.config;
        document.cache_location = locations.cache;
        if new_project {
            document.dirty = true;
            document.emit(Effect::StartAutosaveTimer);
        }
        document.thumbnails.ensure_source(&document.thumbnail_context(), "screen");
        Ok(document)
    }

    /// A document over what [`load_bundle`] read (or a test's own parts): clean, nothing
    /// selected, at output time 0, no playback range, the motion track not built yet (its first
    /// generation due).
    pub fn new(
        recording: Recording,
        project: Project,
        input: InputLog,
        inserts: HashMap<String, VideoInfo>,
        probe_video: ProbeVideo,
    ) -> Document {
        let bundle_path = recording.bundle_path.clone();
        let keystroke_track = KeystrokeTrack::build(input.events(), &project.keystrokes);
        Document {
            project_path: qdir::file_path(&bundle_path, "project.json"),
            video_path: qdir::file_path(&bundle_path, "screen.mp4"),
            camera_video_path: qdir::file_path(&bundle_path, "camera.mp4"),
            recording,
            project,
            input,
            keystroke_track,
            inserts,
            probe_video,
            history: History::default(),
            dirty: false,
            selected_clip_id: String::new(),
            selected_zoom_id: String::new(),
            selected_zoom_ids: Vec::new(),
            picking_zoom_target: false,
            output_position: 0.0,
            playback_range_start: -1.0,
            playback_range_end: -1.0,
            screen_frame_times: Vec::new(),
            motion: None,
            motion_generation: 1,
            running_motion_generation: 0,
            motion_running: false,
            importing: false,
            import_error: String::new(),
            thumbnails: Thumbnails::default(),
            wallpapers: RefCell::new(WallpaperCache::default()),
            project_map_revision: 0,
            gradients: gradient_presets(),
            export: Default::default(),
            config_location: None,
            cache_location: None,
            new_id: Box::new(random_uuid),
            effects: RefCell::new(Vec::new()),
        }
    }

    /// The Qt work queued since the last call, in order.
    pub fn take_effects(&self) -> Vec<Effect> {
        self.effects.take()
    }

    pub(crate) fn emit(&self, effect: Effect) {
        self.effects.borrow_mut().push(effect);
    }

    /// Replaces `QUuid::createUuid().toString(QUuid::WithoutBraces)` for the ids of split and
    /// inserted clips and new zooms.
    pub fn set_id_generator(&mut self, generator: Box<dyn FnMut() -> String + Send>) {
        self.new_id = generator;
    }

    pub(crate) fn next_id(&mut self, prefix: &str) -> String {
        format!("{prefix}-{}", (self.new_id)())
    }

    /// `QStandardPaths::ConfigLocation` (read on each use when `None`), where presets live.
    pub fn set_config_location(&mut self, location: Option<String>) {
        self.config_location = location;
    }

    /// `QStandardPaths::CacheLocation` (read on each use when `None`), where filmstrips live.
    /// Thumbnail sources already made keep their directories.
    pub fn set_cache_location(&mut self, location: Option<String>) {
        self.cache_location = location;
    }

    pub(crate) fn config_location(&self) -> String {
        self.config_location.clone().unwrap_or_else(|| qstandardpaths::writable_location(Location::Config))
    }

    pub(crate) fn cache_location(&self) -> String {
        self.cache_location.clone().unwrap_or_else(|| qstandardpaths::writable_location(Location::Cache))
    }

    // ---------------------------------------------------------------------------------------
    // What the C++ exposed as properties.

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    pub fn bundle_path(&self) -> &str {
        &self.recording.bundle_path
    }

    pub fn project_path(&self) -> &str {
        &self.project_path
    }

    pub fn video_path(&self) -> &str {
        &self.video_path
    }

    pub fn camera_video_path(&self) -> &str {
        &self.camera_video_path
    }

    pub fn bundle_name(&self) -> &str {
        &self.project.name
    }

    pub fn input(&self) -> &InputLog {
        &self.input
    }

    pub fn source_duration(&self) -> f64 {
        self.recording.source_duration
    }

    pub fn source_width(&self) -> i32 {
        self.recording.source_width
    }

    pub fn source_height(&self) -> i32 {
        self.recording.source_height
    }

    pub fn capture_scale(&self) -> f64 {
        self.recording.capture_scale
    }

    pub fn fps(&self) -> f64 {
        self.recording.fps
    }

    pub fn has_audio(&self) -> bool {
        self.recording.has_audio
    }

    pub fn has_desktop_audio(&self) -> bool {
        self.recording.has_desktop_audio
    }

    pub fn has_microphone_audio(&self) -> bool {
        self.recording.has_microphone_audio
    }

    pub fn has_camera(&self) -> bool {
        self.recording.camera.is_some()
    }

    /// The camera's facts, all 0 without a camera (as the C++ members stayed).
    pub(crate) fn camera(&self) -> CameraRecording {
        self.recording.camera.unwrap_or_default()
    }

    pub fn camera_source_width(&self) -> i32 {
        self.camera().width
    }

    pub fn camera_source_height(&self) -> i32 {
        self.camera().height
    }

    pub fn inserts(&self) -> &HashMap<String, VideoInfo> {
        &self.inserts
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    pub fn selected_clip_id(&self) -> &str {
        &self.selected_clip_id
    }

    pub fn selected_zoom_id(&self) -> &str {
        &self.selected_zoom_id
    }

    pub fn selected_zoom_ids(&self) -> &[String] {
        &self.selected_zoom_ids
    }

    pub fn picking_zoom_target(&self) -> bool {
        self.picking_zoom_target
    }

    pub fn importing(&self) -> bool {
        self.importing
    }

    pub fn import_error(&self) -> &str {
        &self.import_error
    }

    /// The gradient presets (`gradients`).
    pub fn gradients(&self) -> &[Value] {
        &self.gradients
    }

    /// Changes whenever the C++ invalidated its `projectMap` cache (every edit, and
    /// `refreshOmarchyTheme`): the Qt side keeps its converted [`Document::project_map`] until
    /// this moves.
    pub fn project_map_revision(&self) -> u64 {
        self.project_map_revision
    }

    /// The motion track once built.
    pub fn motion(&self) -> Option<&MotionTrack> {
        self.motion.as_deref()
    }

    pub fn duration(&self) -> f64 {
        self.timeline().total_duration()
    }

    pub(crate) fn timeline(&self) -> ClipTimeline {
        ClipTimeline::new(self.project.clips.clone())
    }

    /// `outputWidth`: 720 px high at the project's aspect, rounded up to even, at least 2.
    pub fn output_width(&self) -> i32 {
        let width = q_round(720.0 * aspect_ratio(&self.project, self.recording.source_width, self.recording.source_height));
        cpp::max(2, width.wrapping_add(width % 2))
    }

    pub fn output_height(&self) -> i32 {
        720
    }

    // ---------------------------------------------------------------------------------------
    // Lookups and time mapping.

    pub fn clip_index(&self, id: &str) -> i32 {
        self.project.clips.iter().position(|clip| clip.id == id).map_or(-1, |i| i as i32)
    }

    pub fn zoom_index(&self, id: &str) -> i32 {
        self.project.zooms.iter().position(|zoom| zoom.id == id).map_or(-1, |i| i as i32)
    }

    pub(crate) fn clip(&self, index: i32) -> Option<&Clip> {
        usize::try_from(index).ok().and_then(|i| self.project.clips.get(i))
    }

    /// `clipForOutput(output, &clipOutputStart)`: the clip playing at an output time (the last
    /// one past the end) and its output start; -1 and 0 without clips. Unlike
    /// `ClipTimeline::clipAt` it divides by the speed as it is.
    pub fn clip_for_output(&self, output: f64) -> (i32, f64) {
        let clips = &self.project.clips;
        let mut start = 0.0;
        for (i, clip) in clips.iter().enumerate() {
            let length = (clip.out - clip.in_) / clip.speed;
            if output < start + length || i == clips.len() - 1 {
                return (i as i32, start);
            }
            start += length;
        }
        (-1, 0.0)
    }

    pub fn clip_is_media(&self, index: i32) -> bool {
        self.clip(index).is_some_and(Clip::is_media)
    }

    /// `outputToSource`: the recording time at an output time clamped to the timeline (during an
    /// inserted video, the recording time it interrupts).
    pub fn output_to_source(&self, output_time: f64) -> f64 {
        self.timeline().recording_source_time(cpp::clamp(output_time, 0.0, self.duration()))
    }

    /// `sourceToOutput(sourceTime, preferredClip)`: through `preferred_clip` when it is a clip
    /// (its own time, which for an inserted clip is the video's), else through the first
    /// recording clip holding the time; -1 when none does.
    pub fn source_to_output(&self, source_time: f64, preferred_clip: i32) -> f64 {
        let timeline = self.timeline();
        if self.clip(preferred_clip).is_some() {
            return timeline.output_time(preferred_clip, source_time);
        }
        for (i, clip) in self.project.clips.iter().enumerate() {
            if !clip.is_media() && source_time >= clip.in_ && source_time <= clip.out {
                return timeline.output_time(i as i32, source_time);
            }
        }
        -1.0
    }

    /// `sourceRangeToOutput`: output start and end of a recording span's surviving part, -1 and
    /// -1 when all of it was cut.
    pub fn source_range_to_output(&self, source_start: f64, source_end: f64) -> (f64, f64) {
        self.timeline().output_range(source_start, source_end)
    }

    // ---------------------------------------------------------------------------------------
    // Output position and playback range.

    /// `position`.
    pub fn output_position(&self) -> f64 {
        self.output_position
    }

    /// The position part of `setOutputPosition`: clamped to the timeline; returns the clip there
    /// (`m_activeClip`). The Qt side then resets the playhead clock and emits `positionChanged`
    /// and `compositionChanged`.
    pub fn set_output_position(&mut self, output_time: f64) -> i32 {
        self.output_position = cpp::clamp(output_time, 0.0, self.duration());
        self.clip_for_output(self.output_position).0
    }

    /// `m_outputPosition = output` as `handlePlayerPosition` sets it, unclamped.
    pub fn follow_output_position(&mut self, output_time: f64) {
        self.output_position = output_time;
    }

    /// `sourcePosition()` (the composition's `time`).
    pub fn source_position(&self) -> f64 {
        self.output_to_source(self.output_position)
    }

    pub fn has_playback_range(&self) -> bool {
        self.playback_range_end > self.playback_range_start
    }

    /// Start and end of the playback range; -1 and -1 without one.
    pub fn playback_range(&self) -> (f64, f64) {
        (self.playback_range_start, self.playback_range_end)
    }

    /// `setPlaybackRange(from, to)`: ordered and clamped to the timeline; one under a millisecond
    /// clears it.
    pub fn set_playback_range(&mut self, from: f64, to: f64) {
        if !from.is_finite() || !to.is_finite() {
            return;
        }
        let (from, to) = if from > to { (to, from) } else { (from, to) };
        let from = cpp::clamp(from, 0.0, self.duration());
        let to = cpp::clamp(to, 0.0, self.duration());
        if to - from <= 0.001 {
            self.clear_playback_range();
            return;
        }
        if self.playback_range_start == from && self.playback_range_end == to {
            return;
        }
        self.playback_range_start = from;
        self.playback_range_end = to;
        self.emit(Effect::PlaybackRangeChanged);
    }

    pub fn clear_playback_range(&mut self) {
        if !self.has_playback_range() {
            return;
        }
        self.playback_range_start = -1.0;
        self.playback_range_end = -1.0;
        self.emit(Effect::PlaybackRangeChanged);
    }

    /// `clampPlaybackPosition`: inside the timeline, and inside the playback range up to a
    /// millisecond before its end.
    pub fn clamp_playback_position(&self, output_time: f64) -> f64 {
        let output_time = cpp::clamp(output_time, 0.0, self.duration());
        if !self.has_playback_range() {
            return output_time;
        }
        cpp::clamp(
            output_time,
            self.playback_range_start,
            cpp::max(self.playback_range_start, self.playback_range_end - 0.001),
        )
    }

    /// `m_screenFrameTimes`, once `probeFrameTimes(screen.mp4)` has answered (sorted).
    pub fn set_screen_frame_times(&mut self, times: Vec<f64>) {
        self.screen_frame_times = times;
    }

    pub fn screen_frame_times_ready(&self) -> bool {
        !self.screen_frame_times.is_empty()
    }

    /// `nextScreenFrameTime`: the recording's next frame after `source_time` (by more than half a
    /// millisecond); its last frame lasts to the end. Until the frame times are known, every frame
    /// is taken to be the last before a change.
    pub fn next_screen_frame_time(&self, source_time: f64) -> f64 {
        next_screen_frame_time(&self.screen_frame_times, source_time, self.recording.source_duration)
    }

    // ---------------------------------------------------------------------------------------
    // History.

    /// `snapshot(coalesceKey)`.
    pub(crate) fn snapshot(&mut self, coalesce_key: &str) {
        let current = self.project.to_json();
        if self.history.snapshot(coalesce_key, current) {
            self.emit(Effect::HistoryChanged);
        }
    }

    /// `changed(motion)`: the project changed. The keystroke pills are rebuilt here; the rest is
    /// queued.
    pub(crate) fn changed(&mut self, motion: bool) {
        self.project_map_revision += 1;
        self.keystroke_track = KeystrokeTrack::build(self.input.events(), &self.project.keystrokes);
        let (volume, muted) = self.screen_audio();
        self.emit(Effect::SetScreenAudio { volume, muted });
        self.emit(Effect::UpdateInsertAudio);
        if !self.dirty {
            self.dirty = true;
            self.emit(Effect::DirtyChanged);
        }
        self.emit(Effect::StartAutosaveTimer);
        self.emit(Effect::ProjectChanged);
        self.emit(Effect::DurationChanged);
        self.emit(Effect::OutputSizeChanged);
        if motion {
            self.motion_generation = self.motion_generation.wrapping_add(1);
            self.emit(Effect::StartMotionTimer);
        }
        self.emit(Effect::CompositionChanged);
    }

    pub fn begin_coalesced_edit(&mut self, key: &str) {
        self.history.begin_coalesced_edit(key);
    }

    pub fn end_coalesced_edit(&mut self) {
        self.history.end_coalesced_edit();
    }

    /// `restore(json)`: the project from an undo step, a selected clip that is gone replaced by
    /// the clip at the playhead, zoom selections that are gone dropped, and a seek to where the
    /// playhead now falls.
    fn restore(&mut self, json: &Object) {
        self.project = Project::from_json(json);
        if !self.selected_clip_id.is_empty() && self.clip_index(&self.selected_clip_id) < 0 {
            self.selected_clip_id = if self.project.clips.is_empty() {
                String::new()
            } else {
                let at = cpp::min(self.output_position, self.duration());
                self.project.clips[self.clip_for_output(at).0 as usize].id.clone()
            };
        }
        let retained: Vec<String> =
            self.selected_zoom_ids.iter().filter(|id| self.zoom_index(id) >= 0).cloned().collect();
        self.selected_zoom_ids = retained;
        if self.zoom_index(&self.selected_zoom_id) < 0 {
            self.selected_zoom_id = self.selected_zoom_ids.first().cloned().unwrap_or_default();
        }
        self.changed(true);
        if !self.project.clips.is_empty() {
            self.emit(Effect::Seek(cpp::min(self.output_position, self.duration())));
        }
        self.emit(Effect::SelectionChanged);
    }

    pub fn undo(&mut self) {
        let Some(value) = self.history.undo.pop() else { return };
        self.history.redo.push(self.project.to_json());
        self.end_coalesced_edit();
        self.restore(&value);
        self.emit(Effect::HistoryChanged);
    }

    /// Unlike `undo`, leaves a coalesced edit open.
    pub fn redo(&mut self) {
        let Some(value) = self.history.redo.pop() else { return };
        self.history.undo.push(self.project.to_json());
        self.restore(&value);
        self.emit(Effect::HistoryChanged);
    }

    /// `saveNow()`: writes `project.json`; a failure is reported with `errorOccurred`.
    pub fn save_now(&mut self) -> bool {
        if let Err(error) = self.project.save(&self.project_path) {
            self.emit(Effect::ErrorOccurred(error));
            return false;
        }
        if self.dirty {
            self.dirty = false;
            self.emit(Effect::DirtyChanged);
        }
        self.emit(Effect::Autosaved(self.project_path.clone()));
        true
    }

    // ---------------------------------------------------------------------------------------
    // Editing the project as JSON.

    /// `setProjectValue(path, value, coalesce)`: `value` at the dotted path of the project's
    /// JSON, kept only if the project read back from it differs. A coalescing edit uses the open
    /// coalesced edit's key, or the path. `value` is `QJsonValue::fromVariant` of what QML sent
    /// (after the Qt side's [`takes_local_file`] rule): booleans, numbers, strings, `null`, colours
    /// as `QVariant(QColor).toString()` (`#rrggbb`, or `#aarrggbb` when not opaque), and
    /// `Undefined` for an unset variant, which removes the key.
    pub fn set_project_value(&mut self, path: &str, value: Value, coalesce: bool) {
        let parts: Vec<&str> = path.split('.').filter(|part| !part.is_empty()).collect();
        if parts.is_empty() {
            return;
        }
        let old = self.project.to_json();
        let next = set_nested(old.clone(), &parts, 0, value);
        let parsed = Project::from_json(&next);
        if parsed.to_json() == old {
            return;
        }
        let key = if coalesce { self.history.key_or(path) } else { String::new() };
        self.snapshot(&key);
        self.project = parsed;
        self.changed(true);
    }

    /// `applyGradientPreset(index)`: the preset's colours as evenly spaced stops, and the
    /// background switched to the gradient.
    pub fn apply_gradient_preset(&mut self, index: i32) {
        let Some(preset) = usize::try_from(index).ok().and_then(|i| self.gradients.get(i)) else { return };
        let colours = preset.to_array();
        if colours.len() < 2 {
            return;
        }
        let last = cpp::max(1, colours.len() as i64 - 1) as f64;
        let stops: Vec<Value> = colours
            .iter()
            .enumerate()
            .map(|(i, colour)| Value::array(vec![variant_to_string(colour).into(), (i as f64 / last).into()]))
            .collect();
        let old = self.project.to_json();
        let next = set_nested(old.clone(), &["background", "type"], 0, "gradient".into());
        let next = set_nested(next, &["background", "gradient", "stops"], 0, Value::array(stops));
        let parsed = Project::from_json(&next);
        if parsed.to_json() == old {
            return;
        }
        self.snapshot("");
        self.project = parsed;
        self.changed(true);
    }

    // ---------------------------------------------------------------------------------------
    // Selection.

    /// `setSelectedClipId`: selecting a clip deselects zooms and stops picking a zoom target.
    pub fn set_selected_clip_id(&mut self, id: &str) {
        if id != self.selected_clip_id || !self.selected_zoom_id.is_empty() {
            self.set_picking_zoom_target(false);
            self.selected_clip_id = id.to_owned();
            self.selected_zoom_id.clear();
            self.selected_zoom_ids.clear();
            self.emit(Effect::SelectionChanged);
        }
    }

    /// `setSelectedZoomId`: one zoom (or none), and no clip.
    pub fn set_selected_zoom_id(&mut self, id: &str) {
        if id != self.selected_zoom_id || !self.selected_clip_id.is_empty() {
            self.set_picking_zoom_target(false);
            self.selected_zoom_id = id.to_owned();
            self.selected_zoom_ids = if id.is_empty() { Vec::new() } else { vec![id.to_owned()] };
            self.selected_clip_id.clear();
            self.emit(Effect::SelectionChanged);
        }
    }

    /// `setSelectedZoomIds`: the ids that name zooms, each once; the first is the primary one.
    pub fn set_selected_zoom_ids(&mut self, ids: &[String]) {
        let mut valid: Vec<String> = Vec::new();
        for id in ids {
            if self.zoom_index(id) >= 0 && !valid.contains(id) {
                valid.push(id.clone());
            }
        }
        let primary = valid.first().cloned().unwrap_or_default();
        if valid == self.selected_zoom_ids
            && primary == self.selected_zoom_id
            && (valid.is_empty() || self.selected_clip_id.is_empty())
        {
            return;
        }
        self.set_picking_zoom_target(false);
        let any = !valid.is_empty();
        self.selected_zoom_ids = valid;
        self.selected_zoom_id = primary;
        if any {
            self.selected_clip_id.clear();
        }
        self.emit(Effect::SelectionChanged);
    }

    /// `setPickingZoomTarget`: only while a zoom is selected.
    pub fn set_picking_zoom_target(&mut self, value: bool) {
        let value = value && !self.selected_zoom_id.is_empty();
        if value != self.picking_zoom_target {
            self.picking_zoom_target = value;
            self.emit(Effect::PickingZoomTargetChanged);
        }
    }

    // ---------------------------------------------------------------------------------------
    // The motion track.

    /// `rebuildMotion()`: the build to run off the GUI thread, unless one is running; give its
    /// result to [`Document::apply_motion`].
    pub fn motion_job(&mut self) -> Option<MotionJob> {
        if self.motion_running {
            return None;
        }
        self.motion_running = true;
        self.running_motion_generation = self.motion_generation;
        Some(MotionJob {
            duration: self.recording.source_duration,
            width: self.recording.source_width,
            height: self.recording.source_height,
            events: self.input.events().to_vec(),
            project: self.project.clone(),
            capture_fps: self.recording.fps,
        })
    }

    /// `applyMotionResult()`: the new track, and another build if the project changed meanwhile.
    pub fn apply_motion(&mut self, track: MotionTrack) {
        self.motion = Some(Arc::new(track));
        self.motion_running = false;
        self.emit(Effect::CompositionChanged);
        if self.running_motion_generation != self.motion_generation {
            self.emit(Effect::StartMotionTimer);
        }
    }

    /// `refreshOmarchyTheme()`: the theme or wallpapers changed on disk.
    pub fn refresh_omarchy_theme(&mut self) {
        self.wallpapers.borrow_mut().valid = false;
        self.project_map_revision += 1;
        self.emit(Effect::WallpapersChanged);
        self.emit(Effect::ProjectChanged);
    }
}

// The Qt side keeps the document in its QObject and hands the jobs to worker threads.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Document>();
    assert_send::<MotionJob>();
    assert_send::<super::ImportJob>();
    assert_send::<super::ThumbnailJob>();
    assert_send::<super::CameraThumbnailJob>();
};

/// `nextScreenFrameTime` over a sorted list of frame times.
pub fn next_screen_frame_time(frame_times: &[f64], source_time: f64, source_duration: f64) -> f64 {
    if frame_times.is_empty() {
        return source_time;
    }
    // std::upper_bound as libstdc++ searches: the first time greater than source_time + 0.0005.
    let key = source_time + 0.0005;
    let (mut first, mut len) = (0, frame_times.len());
    while len > 0 {
        let half = len / 2;
        if key < frame_times[first + half] {
            len = half;
        } else {
            first += half + 1;
            len -= half + 1;
        }
    }
    match frame_times.get(first) {
        Some(&time) => time,
        None => cpp::max(source_time, source_duration),
    }
}
