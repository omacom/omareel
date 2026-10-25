//! The editor's document: everything the C++ `Editor` (src/ui/Editor.cpp at the tag
//! `rust-migration-base`; function names and line numbers here refer to it) decided that is not
//! Qt plumbing. The CXX-Qt `Editor` (crates/omareel_qt/src/editor.rs, QML's `editor` and `comp`)
//! holds a [`Document`]; it converts what the document returns into `QVariant`s, owns the threads,
//! and drives the players and timers through its C++ base, `EditorBase`.
//!
//! # How the Qt side uses it
//!
//! Every operation changes the document at once and queues the Qt work it implies, in the
//! order the C++ did it, as [`Effect`]s: signals to emit, timers to start, and the Qt-side
//! steps `Editor` called (its `seek`, `pause`, `syncCamera`, `updateInsertAudio`). After each
//! call into the document the Qt side drains [`Document::take_effects`] and performs them in
//! order. QML handlers run by those signals may call back into the editor; by then the
//! document is in its final state, while the C++ answered them part-way through the operation.
//! Where that showed in what the C++ emitted (the pause before `deleteOutputRange`,
//! `reorderClip` and `insertMedia`), the operation has a split form the Qt side pauses inside
//! (see [`Effect::Pause`]).
//!
//! What stays on the Qt side, and why:
//! - The players and their state (`PlaybackDriver`, `m_transportPlaying`, `m_activeClip`, the
//!   playhead and its `QElapsedTimer`, scrubbing, `m_internalSeek`, `m_mediaPlayerSource`):
//!   they follow QMediaPlayer's signals. The decisions inside `seekMedia`, `prepareNextClip`,
//!   `advanceClip`, `updatePlayheadPosition`, `handleFrameShown`, `handlePlayerPosition`,
//!   `syncCamera` and `updateInsertAudio` are here as functions of that state ([`playback`]).
//!   The output position and the playback range are in the document, because edits read them.
//! - Timers (autosave 500 ms, motion 100 ms, scrub 35 ms, preview 16 ms, loading 1.5 s) and the
//!   worker threads: the document hands out jobs ([`MotionJob`], [`ImportJob`],
//!   [`ThumbnailJob`], [`CameraThumbnailJob`]) that run anywhere and takes their results back.
//! - `QVariant` conversions: values from QML arrive as [`qjson::Value`](crate::compat::qjson)
//!   made by `QJsonValue::fromVariant`, after the Qt side applies the URL rule of
//!   [`takes_local_file`]; QUrls from QML (`insertMedia`, `exportTo`) are turned into paths
//!   there (`isLocalFile() ? toLocalFile() : toString()`), and the `QVariantMap` settings of
//!   `exportTo` and the target of `setZoomTarget` are read with `QVariant`'s rules (see
//!   [`ExportDialogSettings`] and [`ZoomTarget`]).
//! - `ThemeFonts::setApplicationFont` in `projectMap`, `QQuickWindow::graphicsApi` in
//!   `softwareRendering`, the clipboard, `xdg-open`, `OmarchyPaths::generateWallpaperThumbnail`
//!   (QImage), and the `OMAREEL_*` environment switches. `probeVideo` comes in as
//!   [`Probes::video`] (normally [`crate::media_probe::probe_video`]), and the Qt side runs
//!   [`crate::media_probe::probe_frame_times`] for [`Document::set_screen_frame_times`].

mod bundle;
mod clips;
mod composition;
mod document;
mod exporting;
mod history;
mod ids;
pub mod playback;
mod presets;
#[cfg(test)]
mod tests;
mod project_map;
mod thumbnails;
mod zooms;

pub use bundle::*;
pub use clips::{ImportJob, OutputCut};
pub use composition::*;
pub use document::*;
pub use exporting::*;
pub use ids::random_uuid;
pub use playback::{CameraSync, MediaSeek, NextClip, NextClipKind, Player, PlayerPosition, Transport};
pub use presets::clean_preset_name;
pub use thumbnails::*;
pub use zooms::ZoomTarget;

/// Qt work an operation implies, in the order `Editor` did it. Names follow the C++: a
/// `...Changed` is the signal of that name.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    // Editor::changed(), in its order.
    /// `m_playback.setAudio(PlaybackDriver::Screen, volume, muted)`.
    SetScreenAudio { volume: f64, muted: bool },
    /// `updateInsertAudio()`: [`Document::insert_audio`] for the active clip and the media
    /// player's warm-up.
    UpdateInsertAudio,
    DirtyChanged,
    /// `m_autosaveTimer.start()` (500 ms, single shot, then [`Document::save_now`]).
    StartAutosaveTimer,
    ProjectChanged,
    DurationChanged,
    OutputSizeChanged,
    /// `m_motionTimer.start()` (100 ms, single shot, then [`Document::motion_job`]).
    StartMotionTimer,
    CompositionChanged,

    HistoryChanged,
    SelectionChanged,
    PickingZoomTargetChanged,
    PlaybackRangeChanged,
    PresetsChanged,
    WallpapersChanged,
    ImportingChanged,
    VideoThumbnailRevisionChanged,
    ExportingChanged,
    ExportProgressChanged,
    ExportErrorChanged,
    /// `exportFinished(path)`.
    ExportFinished(String),
    /// `errorOccurred(message)`.
    ErrorOccurred(String),
    /// `autosaved(path)`.
    Autosaved(String),

    /// `Editor::seek(outputTime)`: unless there are no clips, stop the scrub timer and scrubbing,
    /// [`Document::set_output_position`] to [`Document::clamp_playback_position`] of it (resetting
    /// the playhead clock, `positionChanged`, `compositionChanged`), `seekMedia` with
    /// [`Document::media_seek`], and start the next video thumbnail.
    Seek(f64),
    /// `Editor::pause()`. `deleteOutputRange`, `reorderClip` and `insertMedia` paused before they
    /// changed anything, so a handler of `playingChanged` saw the document as it was; the
    /// operations that queue this effect first are for callers that do not mind. The Qt side
    /// uses their split forms ([`Document::prepare_delete_output_range`] and
    /// [`Document::apply_delete_output_range`], [`Document::can_reorder_clip`] and
    /// [`Document::apply_reorder_clip`], [`Document::can_insert_media`] and
    /// [`Document::start_import`]) and pauses in between, where the C++ did.
    Pause,
    /// `syncCamera(screenSourceTime, force)` with [`Document::camera_sync`].
    SyncCamera { screen_source_time: f64, force: bool },
    /// `startNextVideoThumbnail()`: [`Document::next_video_thumbnail_job`] unless playing or
    /// scrubbing.
    StartNextVideoThumbnail,
    /// Run [`Document::import_job`] for this source off the GUI thread, then
    /// [`Document::finish_import`].
    StartImport(String),
    /// `OmarchyPaths::generateWallpaperThumbnail(path, thumbnail)` off the GUI thread, then
    /// [`Document::wallpaper_thumbnail_ready`].
    GenerateWallpaperThumbnail { path: String, thumbnail: String },
}
