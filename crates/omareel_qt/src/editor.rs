//! The `editor` object QML edits a recording through, which it also reads as `comp`, the
//! preview's composition. It replaced src/ui/Editor.cpp and keeps its QML surface: the same
//! properties, invokables, signals and default arguments, and the C++ call sites the tests use.
//!
//! What the editor decides is `omareel_core::editor::Document`. This object holds the document
//! and the state that follows the players (the transport, the playhead clock, scrubbing, the
//! inserted video the media player holds), converts what the document returns into Qt values,
//! and performs the [`Effect`]s each call into the document queues, in their order. `EditorBase`
//! (crates/omareel_qt/cpp) owns the players, the timers and the exporter, and reports to the
//! overrides at the end of the bridge. The worker threads (the motion track, filmstrip and
//! camera frames, imports, frame times, wallpaper thumbnails) are Rust threads that hand their
//! results back with `qt_thread().queue`; dropping the object waits for them, as the C++
//! destructor waited for its futures.

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QPointF, QString, QStringList, QUrl, QVariant};
use omareel_core::compat::qdir;
use omareel_core::compat::qenv;
use omareel_core::compat::qjson::{self, Value};
use omareel_core::editor::{
    self, containing_folder, format_time, preview_stats_line, takes_local_file, trace_input_line, Document, Effect,
    ExportDialogSettings, NextClipKind, Player, Probes, Transport, ZoomTarget,
};
use omareel_core::media_probe;
use omareel_core::project::Project;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::export::ffi::ExportOptions;
use crate::export::{json_map, json_variant, map};
use crate::theme::to_qt;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        // QtShims.h first: it brings in QString whole before cxx-qt-lib's qstring.h (see
        // export.rs).
        include!("QtShims.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
        include!("cxx-qt-lib/qpointf.h");
        type QPointF = cxx_qt_lib::QPointF;
    }

    #[namespace = "Omareel"]
    unsafe extern "C++Qt" {
        include!("EditorBase.h");
        #[qobject]
        type EditorBase;
    }

    #[namespace = "omareel::exporter"]
    unsafe extern "C++" {
        include!("omareel_qt/src/export.cxxqt.h");
        type ExportOptions = crate::export::ffi::ExportOptions;
    }

    #[namespace = "Omareel::EditorShims"]
    unsafe extern "C++" {
        #[cxx_name = "projectValueJson"]
        fn project_value_json(value: &QVariant, take_local_file: bool) -> QString;
        #[cxx_name = "urlPath"]
        fn url_path(url: &QUrl) -> QString;
        #[cxx_name = "textUrlPath"]
        fn text_url_path(text: &QString) -> QString;
        #[cxx_name = "zoomTarget"]
        fn zoom_target(target: &QVariant, x: &mut f64, y: &mut f64) -> bool;
        #[cxx_name = "exportSettings"]
        fn export_settings(settings: &QMap_QString_QVariant, fps: &mut i32, height: &mut i32, quality: &mut QString);
        #[cxx_name = "softwareRendering"]
        fn software_rendering() -> bool;
        #[cxx_name = "copyToClipboard"]
        fn copy_to_clipboard(text: &QString);
        #[cxx_name = "openFolder"]
        fn open_folder(directory: &QString);
        #[cxx_name = "generateWallpaperThumbnail"]
        fn generate_wallpaper_thumbnail(wallpaper_path: &QString, thumbnail_path: &QString) -> bool;
    }

    #[namespace = "Omareel::ThemeFonts"]
    unsafe extern "C++" {
        include!("ThemeBase.h");
        #[cxx_name = "interfaceFamily"]
        fn interface_family() -> QString;
    }

    #[namespace = "Omareel"]
    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[base = EditorBase]
        #[qproperty(QMap_QString_QVariant, project, READ = project_map, NOTIFY = project_changed)]
        #[qproperty(QList_QVariant, clips, READ = clips, NOTIFY = project_changed)]
        #[qproperty(QList_QVariant, zooms, READ = zooms, NOTIFY = project_changed)]
        #[qproperty(QString, bundle_path, READ = bundle_path, CONSTANT)]
        #[qproperty(QString, bundle_name, READ = bundle_name, NOTIFY = project_changed)]
        #[qproperty(f64, duration, READ = duration, NOTIFY = duration_changed)]
        #[qproperty(f64, source_duration, READ = source_duration, CONSTANT)]
        #[qproperty(i32, source_width, READ = source_width, CONSTANT)]
        #[qproperty(i32, source_height, READ = source_height, CONSTANT)]
        #[qproperty(f64, capture_scale, READ = capture_scale, CONSTANT)]
        #[qproperty(f64, fps, READ = fps, CONSTANT)]
        #[qproperty(bool, has_audio, READ = has_audio, CONSTANT)]
        #[qproperty(bool, has_desktop_audio, READ = has_desktop_audio, CONSTANT)]
        #[qproperty(bool, has_microphone_audio, READ = has_microphone_audio, CONSTANT)]
        #[qproperty(bool, has_camera, READ = has_camera, CONSTANT)]
        #[qproperty(QList_QVariant, camera_thumbnails, READ = camera_thumbnails, NOTIFY = camera_thumbnails_changed)]
        #[qproperty(i32, video_thumbnail_revision, READ = video_thumbnail_revision, NOTIFY = video_thumbnail_revision_changed)]
        #[qproperty(bool, loading, READ = loading, NOTIFY = loading_changed)]
        // A video being copied into the bundle by insertMedia().
        #[qproperty(bool, importing, READ = importing, NOTIFY = importing_changed)]
        #[qproperty(QString, import_error, READ = import_error, NOTIFY = importing_changed)]
        #[qproperty(f64, position, READ = position, NOTIFY = position_changed)]
        #[qproperty(f64, playhead_position, READ = playhead_position, NOTIFY = playhead_position_changed)]
        #[qproperty(bool, playing, READ = playing, NOTIFY = playing_changed)]
        #[qproperty(bool, has_playback_range, READ = has_playback_range, NOTIFY = playback_range_changed)]
        #[qproperty(bool, can_undo, READ = can_undo, NOTIFY = history_changed)]
        #[qproperty(bool, can_redo, READ = can_redo, NOTIFY = history_changed)]
        #[qproperty(bool, dirty, READ = dirty, NOTIFY = dirty_changed)]
        #[qproperty(QString, selected_clip_id, READ = selected_clip_id, WRITE = set_selected_clip_id, NOTIFY = selection_changed)]
        #[qproperty(QString, selected_zoom_id, READ = selected_zoom_id, WRITE = set_selected_zoom_id, NOTIFY = selection_changed)]
        #[qproperty(QStringList, selected_zoom_ids, READ = selected_zoom_ids, WRITE = set_selected_zoom_ids, NOTIFY = selection_changed)]
        #[qproperty(bool, picking_zoom_target, READ = picking_zoom_target, WRITE = set_picking_zoom_target, NOTIFY = picking_zoom_target_changed)]
        #[qproperty(QStringList, preset_names, READ = preset_names, NOTIFY = presets_changed)]
        #[qproperty(QList_QVariant, wallpaper_groups, READ = wallpaper_groups, NOTIFY = wallpapers_changed)]
        #[qproperty(QList_QVariant, gradients, READ = gradients, CONSTANT)]
        #[qproperty(f64, export_progress, READ = export_progress, NOTIFY = export_progress_changed)]
        #[qproperty(bool, exporting, READ = exporting, NOTIFY = exporting_changed)]
        #[qproperty(QString, export_error, READ = export_error, NOTIFY = export_error_changed)]
        #[qproperty(f64, export_fps, READ = export_fps, NOTIFY = export_progress_changed)]
        #[qproperty(i32, export_eta_seconds, READ = export_eta_seconds, NOTIFY = export_progress_changed)]
        #[qproperty(QString, exported_path, READ = exported_path, NOTIFY = export_finished)]
        // Composition.qml contract; the editor is registered as both "editor" and "comp".
        #[qproperty(i32, output_width, READ = output_width, NOTIFY = output_size_changed)]
        #[qproperty(i32, output_height, READ = output_height, NOTIFY = output_size_changed)]
        #[qproperty(f64, time, READ = source_position, NOTIFY = composition_changed)]
        #[qproperty(QMap_QString_QVariant, zoom, READ = preview_zoom, NOTIFY = composition_changed)]
        #[qproperty(QMap_QString_QVariant, layout, READ = preview_layout, NOTIFY = composition_changed)]
        #[qproperty(QMap_QString_QVariant, media, READ = preview_media, NOTIFY = composition_changed)]
        #[qproperty(QMap_QString_QVariant, cursor, READ = preview_cursor, NOTIFY = composition_changed)]
        #[qproperty(QList_QVariant, ripples, READ = preview_ripples, NOTIFY = composition_changed)]
        #[qproperty(QList_QVariant, keystroke_pills, READ = preview_keystroke_pills, NOTIFY = composition_changed)]
        #[qproperty(bool, software_rendering, READ = software_rendering, CONSTANT)]
        #[qproperty(bool, camera_available, READ = has_camera, CONSTANT)]
        #[qproperty(bool, camera_visible, READ = camera_visible, NOTIFY = composition_changed)]
        #[qproperty(i32, camera_source_width, READ = camera_source_width, CONSTANT)]
        #[qproperty(i32, camera_source_height, READ = camera_source_height, CONSTANT)]
        #[qproperty(QString, camera_status, READ = camera_status, CONSTANT)]
        type Editor = super::EditorRust;

        #[qsignal]
        fn project_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn duration_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn position_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn playhead_position_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn playing_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn playback_range_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn video_thumbnail_revision_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn history_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn dirty_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn selection_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn picking_zoom_target_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn presets_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn wallpapers_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn camera_thumbnails_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn loading_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn importing_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn output_size_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn composition_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn export_progress_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn exporting_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn export_error_changed(self: Pin<&mut Editor>);
        #[qsignal]
        fn export_finished(self: Pin<&mut Editor>, path: QString);
        #[qsignal]
        fn autosaved(self: Pin<&mut Editor>, path: QString);
        #[qsignal]
        fn error_occurred(self: Pin<&mut Editor>, message: QString);

        fn is_valid(self: &Editor) -> bool;
        fn error_string(self: &Editor) -> QString;
        fn project_map(self: &Editor) -> QMap_QString_QVariant;
        fn clips(self: &Editor) -> QList_QVariant;
        fn zooms(self: &Editor) -> QList_QVariant;
        fn bundle_path(self: &Editor) -> QString;
        fn bundle_name(self: &Editor) -> QString;
        fn duration(self: &Editor) -> f64;
        fn source_duration(self: &Editor) -> f64;
        fn source_width(self: &Editor) -> i32;
        fn source_height(self: &Editor) -> i32;
        fn capture_scale(self: &Editor) -> f64;
        fn fps(self: &Editor) -> f64;
        fn has_audio(self: &Editor) -> bool;
        fn has_desktop_audio(self: &Editor) -> bool;
        fn has_microphone_audio(self: &Editor) -> bool;
        fn has_camera(self: &Editor) -> bool;
        fn camera_thumbnails(self: &Editor) -> QList_QVariant;
        fn video_thumbnail_revision(self: &Editor) -> i32;
        fn loading(self: &Editor) -> bool;
        fn importing(self: &Editor) -> bool;
        fn import_error(self: &Editor) -> QString;
        fn position(self: &Editor) -> f64;
        fn playhead_position(self: &Editor) -> f64;
        fn playing(self: &Editor) -> bool;
        fn has_playback_range(self: &Editor) -> bool;
        fn can_undo(self: &Editor) -> bool;
        fn can_redo(self: &Editor) -> bool;
        fn dirty(self: &Editor) -> bool;
        fn selected_clip_id(self: &Editor) -> QString;
        fn selected_zoom_id(self: &Editor) -> QString;
        fn selected_zoom_ids(self: &Editor) -> QStringList;
        fn picking_zoom_target(self: &Editor) -> bool;
        fn preset_names(self: &Editor) -> QStringList;
        fn wallpaper_groups(self: &Editor) -> QList_QVariant;
        fn gradients(self: &Editor) -> QList_QVariant;
        fn export_progress(self: &Editor) -> f64;
        fn exporting(self: &Editor) -> bool;
        fn export_error(self: &Editor) -> QString;
        fn export_fps(self: &Editor) -> f64;
        fn export_eta_seconds(self: &Editor) -> i32;
        fn exported_path(self: &Editor) -> QString;
        fn output_width(self: &Editor) -> i32;
        fn output_height(self: &Editor) -> i32;
        fn source_position(self: &Editor) -> f64;
        fn preview_zoom(self: &Editor) -> QMap_QString_QVariant;
        fn preview_layout(self: &Editor) -> QMap_QString_QVariant;
        fn preview_media(self: &Editor) -> QMap_QString_QVariant;
        fn preview_cursor(self: &Editor) -> QMap_QString_QVariant;
        fn preview_ripples(self: &Editor) -> QList_QVariant;
        fn preview_keystroke_pills(self: &Editor) -> QList_QVariant;
        fn software_rendering(self: &Editor) -> bool;
        fn camera_source_width(self: &Editor) -> i32;
        fn camera_source_height(self: &Editor) -> i32;
        fn camera_status(self: &Editor) -> QString;
        fn camera_visible(self: &Editor) -> bool;
        // Test hook (no UI use).
        fn screen_frame_times_ready_for_tests(self: &Editor) -> bool;

        fn set_selected_clip_id(self: Pin<&mut Editor>, id: &QString);
        fn set_selected_zoom_id(self: Pin<&mut Editor>, id: &QString);
        fn set_selected_zoom_ids(self: Pin<&mut Editor>, ids: &QStringList);
        fn set_picking_zoom_target(self: Pin<&mut Editor>, value: bool);

        #[qinvokable]
        fn set_project_value(self: Pin<&mut Editor>, path: &QString, value: &QVariant);
        #[qinvokable]
        #[cxx_name = "setProjectValue"]
        fn set_project_value_coalescing(self: Pin<&mut Editor>, path: &QString, value: &QVariant, coalesce: bool);
        #[qinvokable]
        fn apply_gradient_preset(self: Pin<&mut Editor>, index: i32);
        #[qinvokable]
        fn begin_coalesced_edit(self: Pin<&mut Editor>, key: &QString);
        #[qinvokable]
        fn end_coalesced_edit(self: Pin<&mut Editor>);
        #[qinvokable]
        fn undo(self: Pin<&mut Editor>);
        #[qinvokable]
        fn redo(self: Pin<&mut Editor>);
        #[qinvokable]
        fn save_now(self: Pin<&mut Editor>) -> bool;
        #[qinvokable]
        fn play_pause(self: Pin<&mut Editor>);
        #[qinvokable]
        fn play(self: Pin<&mut Editor>);
        #[qinvokable]
        fn pause(self: Pin<&mut Editor>);
        #[qinvokable]
        fn set_playback_range(self: Pin<&mut Editor>, from: f64, to: f64);
        #[qinvokable]
        fn clear_playback_range(self: Pin<&mut Editor>);
        #[qinvokable]
        fn video_thumbnail_url(self: Pin<&mut Editor>, source_time: f64) -> QString;
        #[qinvokable]
        fn video_thumbnail_url_for_clip(
            self: Pin<&mut Editor>,
            source_time: f64,
            source_start: f64,
            source_end: f64,
        ) -> QString;
        // Filmstrip frame for a clip at a source time: camera clips show the webcam.
        #[qinvokable]
        fn clip_thumbnail_url(
            self: Pin<&mut Editor>,
            clip_id: &QString,
            source_time: f64,
            source_start: f64,
            source_end: f64,
        ) -> QString;
        #[qinvokable]
        fn seek(self: Pin<&mut Editor>, output_time: f64);
        #[qinvokable]
        fn begin_scrub(self: Pin<&mut Editor>);
        #[qinvokable]
        fn scrub_to(self: Pin<&mut Editor>, output_time: f64);
        #[qinvokable]
        fn end_scrub(self: Pin<&mut Editor>);
        #[qinvokable]
        fn step_frames(self: Pin<&mut Editor>, frames: i32);
        #[qinvokable]
        fn seek_boundary(self: Pin<&mut Editor>, direction: i32);
        #[qinvokable]
        fn output_to_source(self: &Editor, output_time: f64) -> f64;
        #[qinvokable]
        fn source_to_output(self: &Editor, source_time: f64) -> f64;
        #[qinvokable]
        #[cxx_name = "sourceToOutput"]
        fn source_to_output_in_clip(self: &Editor, source_time: f64, preferred_clip: i32) -> f64;
        // x/y: output start/end of a source span's surviving part; -1 when all of it was cut.
        #[qinvokable]
        fn source_range_to_output(self: &Editor, source_start: f64, source_end: f64) -> QPointF;
        #[qinvokable]
        fn split_at_playhead(self: Pin<&mut Editor>) -> bool;
        #[qinvokable]
        fn move_clip(self: Pin<&mut Editor>, id: &QString, source_in: f64) -> bool;
        #[qinvokable]
        fn trim_clip(self: Pin<&mut Editor>, id: &QString, new_in: f64, new_out: f64) -> bool;
        #[qinvokable]
        fn remove_clip(self: Pin<&mut Editor>, id: &QString) -> bool;
        #[qinvokable]
        fn delete_output_range(self: Pin<&mut Editor>, from: f64, to: f64) -> bool;
        #[qinvokable]
        fn merge_clip(self: Pin<&mut Editor>, id: &QString, direction: i32) -> bool;
        #[qinvokable]
        fn reset_clip_trims(self: Pin<&mut Editor>, id: &QString) -> bool;
        #[qinvokable]
        fn set_clip_speed(self: Pin<&mut Editor>, id: &QString, speed: f64) -> bool;
        #[qinvokable]
        fn set_clip_layout(self: Pin<&mut Editor>, id: &QString, layout: &QString) -> bool;
        #[qinvokable]
        fn set_clip_fit(self: Pin<&mut Editor>, id: &QString, fit: &QString) -> bool;
        // Moves an inserted clip one place earlier (-1) or later (+1) on the timeline.
        #[qinvokable]
        fn reorder_clip(self: Pin<&mut Editor>, id: &QString, direction: i32) -> bool;
        // Copies a video into the bundle's media/ directory, then inserts it at the playhead,
        // splitting the clip there.
        #[qinvokable]
        fn insert_media(self: Pin<&mut Editor>, url: &QUrl);
        #[qinvokable]
        fn clear_import_error(self: Pin<&mut Editor>);
        #[qinvokable]
        fn add_zoom_at(self: Pin<&mut Editor>, output_time: f64) -> QString;
        #[qinvokable]
        #[cxx_name = "addZoomAt"]
        fn add_zoom_at_with_length(self: Pin<&mut Editor>, output_time: f64, length: f64) -> QString;
        #[qinvokable]
        fn move_zoom(self: Pin<&mut Editor>, id: &QString, source_start: f64) -> bool;
        #[qinvokable]
        fn resize_zoom(self: Pin<&mut Editor>, id: &QString, source_start: f64, source_end: f64) -> bool;
        #[qinvokable]
        fn set_zoom_level(self: Pin<&mut Editor>, id: &QString, level: f64) -> bool;
        #[qinvokable]
        #[cxx_name = "setZoomLevel"]
        fn set_zoom_level_coalescing(self: Pin<&mut Editor>, id: &QString, level: f64, coalesce: bool) -> bool;
        #[qinvokable]
        fn set_zoom_target(self: Pin<&mut Editor>, id: &QString, target: &QVariant) -> bool;
        #[qinvokable]
        fn set_zoom_target_from_preview(self: Pin<&mut Editor>, x: f64, y: f64) -> bool;
        #[qinvokable]
        fn remove_zoom(self: Pin<&mut Editor>, id: &QString) -> bool;
        #[qinvokable]
        fn select_zooms_in_output_range(self: Pin<&mut Editor>, output_start: f64, output_end: f64);
        #[qinvokable]
        fn move_selected_zooms(self: Pin<&mut Editor>, anchor_id: &QString, source_start: f64) -> bool;
        #[qinvokable]
        fn remove_selected_zooms(self: Pin<&mut Editor>) -> i32;
        #[qinvokable]
        fn regenerate_zooms(self: Pin<&mut Editor>);
        #[qinvokable]
        fn save_preset(self: Pin<&mut Editor>, name: &QString);
        #[qinvokable]
        fn load_preset(self: Pin<&mut Editor>, name: &QString);
        #[qinvokable]
        fn delete_preset(self: Pin<&mut Editor>, name: &QString);
        #[qinvokable]
        fn export_to(self: Pin<&mut Editor>, path: &QString);
        #[qinvokable]
        #[cxx_name = "exportTo"]
        fn export_to_with_settings(self: Pin<&mut Editor>, path: &QString, settings: &QMap_QString_QVariant);
        #[qinvokable]
        fn default_export_path(self: &Editor, format: &QString) -> QString;
        #[qinvokable]
        fn format_time(self: &Editor, seconds: f64) -> QString;
        #[qinvokable]
        fn open_containing_folder(self: &Editor, path: &QString);
        #[qinvokable]
        fn copy_path(self: &Editor, path: &QString);
        #[qinvokable]
        fn refresh_omarchy_theme(self: Pin<&mut Editor>);
        #[qinvokable]
        fn trace_input(self: &Editor, object_name: &QString, phase: &QString, x: f64, y: f64);

        #[cxx_override]
        fn player_should_play(self: &Editor, player: i32) -> bool;
        #[cxx_override]
        fn screen_rest_position(self: &Editor) -> i64;
        #[cxx_override]
        fn handle_frame_shown(self: Pin<&mut Editor>, player: i32, start_time_us: i64, live: bool);
        #[cxx_override]
        fn handle_media_frame_ready(self: Pin<&mut Editor>);
        #[cxx_override]
        fn handle_warming_changed(self: Pin<&mut Editor>);
        #[cxx_override]
        fn handle_media_status(self: Pin<&mut Editor>, player: i32, status: i32);
        #[cxx_override]
        fn handle_playback_state(self: Pin<&mut Editor>, player: i32);
        #[cxx_override]
        fn camera_output_attached(self: Pin<&mut Editor>);
        #[cxx_override]
        fn media_output_attached(self: Pin<&mut Editor>);
        #[cxx_override]
        fn timer_fired(self: Pin<&mut Editor>, timer: i32);
        #[cxx_override]
        fn called_later(self: Pin<&mut Editor>, call: i32);
        #[cxx_override]
        fn export_progressed(self: Pin<&mut Editor>, frame: i32, total: i32);
        #[cxx_override]
        fn export_succeeded(self: Pin<&mut Editor>, path: &QString);
        #[cxx_override]
        fn export_failed_with(self: Pin<&mut Editor>, message: &QString);
    }

    #[namespace = "Omareel"]
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[inherit]
        fn playback_load_screen(self: Pin<&mut Editor>, path: &QString);
        #[inherit]
        fn playback_load_camera(self: Pin<&mut Editor>, path: &QString);
        #[inherit]
        fn playback_load_media(self: Pin<&mut Editor>, path: &QString);
        #[inherit]
        fn playback_warm_up(self: Pin<&mut Editor>, player: i32);
        #[inherit]
        fn playback_cancel_warm_up(self: Pin<&mut Editor>, player: i32);
        #[inherit]
        fn playback_warming(self: &Editor, player: i32) -> bool;
        #[inherit]
        fn playback_seek(self: Pin<&mut Editor>, player: i32, milliseconds: i64, will_play: bool);
        #[inherit]
        fn playback_play(self: Pin<&mut Editor>, player: i32);
        #[inherit]
        fn playback_pause(self: Pin<&mut Editor>, player: i32);
        #[inherit]
        fn playback_set_rate(self: Pin<&mut Editor>, player: i32, rate: f64);
        #[inherit]
        fn playback_sync_camera(self: Pin<&mut Editor>, target_ms: i64, tolerance_ms: i64, force: bool, covered: bool);
        #[inherit]
        fn playback_set_audio(self: Pin<&mut Editor>, player: i32, volume: f64, muted: bool);
        #[inherit]
        fn playback_position(self: &Editor, player: i32) -> i64;
        #[inherit]
        fn playback_position_ready(self: &Editor, player: i32) -> bool;
        #[inherit]
        fn playback_state(self: &Editor, player: i32) -> i32;
        #[inherit]
        fn playback_media_frame_ready(self: &Editor) -> bool;
        #[inherit]
        fn start_editor_timer(self: Pin<&mut Editor>, timer: i32);
        #[inherit]
        fn stop_editor_timer(self: Pin<&mut Editor>, timer: i32);
        #[inherit]
        fn editor_timer_active(self: &Editor, timer: i32) -> bool;
        #[inherit]
        fn call_later(self: Pin<&mut Editor>, call: i32, milliseconds: i32);
        #[inherit]
        fn start_export(self: Pin<&mut Editor>, options: &ExportOptions);
    }

    impl cxx_qt::Threading for Editor {}
    impl cxx_qt::Constructor<(QString,), NewArguments = (QString,)> for Editor {}
}

// PlaybackDriver::Player.
const SCREEN: i32 = 0;
const CAMERA: i32 = 1;
const MEDIA: i32 = 2;

// QMediaPlayer::PlaybackState and MediaStatus.
const PLAYING_STATE: i32 = 1;
const PAUSED_STATE: i32 = 2;
const LOADED_MEDIA: i32 = 2;
const BUFFERED_MEDIA: i32 = 5;
const END_OF_MEDIA: i32 = 6;
const INVALID_MEDIA: i32 = 7;

// EditorBase::Timer and EditorBase::Call.
const AUTOSAVE_TIMER: i32 = 0;
const MOTION_TIMER: i32 = 1;
const SCRUB_TIMER: i32 = 2;
const PREVIEW_TIMER: i32 = 3;
const PREVIEW_STATS_TIMER: i32 = 4;
const FINISH_LOADING_CALL: i32 = 0;
const ADVANCE_CLIP_CALL: i32 = 1;

fn player_index(player: Player) -> i32 {
    match player {
        Player::Screen => SCREEN,
        Player::Camera => CAMERA,
        Player::Media => MEDIA,
    }
}

fn player_from_index(player: i32) -> Player {
    match player {
        CAMERA => Player::Camera,
        MEDIA => Player::Media,
        _ => Player::Screen,
    }
}

/// Writes a line to stderr in one piece, as `QTextStream(stderr) << line` did.
fn print_error(line: &str) {
    use std::io::Write;
    let _ = std::io::stderr().write_all(line.as_bytes());
}

fn qstring(text: &str) -> QString {
    QString::from(text)
}

fn string_list(items: &[String]) -> QStringList {
    items.iter().map(|item| qstring(item)).collect()
}

fn url_list(urls: &[String]) -> QList<QVariant> {
    urls.iter().map(|url| QVariant::from(&qstring(url))).collect()
}

/// Wallpaper thumbnails waiting for a worker, and how many workers run: they share
/// `QThreadPool::globalInstance()`'s limit, the ideal thread count, as QtConcurrent::run did.
#[derive(Default)]
struct WallpaperQueue {
    state: Mutex<(VecDeque<(String, String)>, usize)>,
}

/// The `Editor`'s Rust side: the document and the state that follows the players.
pub struct EditorRust {
    /// `QDir(bundlePath).absolutePath()`.
    bundle_path: String,
    /// The open recording; `None` when it could not be opened (`isValid()` is false).
    document: Option<Document>,
    error: String,
    transport: Transport,
    /// `m_playheadClock`; `None` while invalid.
    playhead_clock: Option<Instant>,
    /// The inserted video the media player holds.
    media_player_source: String,
    pending_scrub_position: f64,
    loading: bool,
    preview_stats_enabled: bool,
    preview_stats_frames: i32,
    preview_stats_cost_ns: i64,
    preview_stats_clock: Option<Instant>,
    camera_thumbnails: QList<QVariant>,
    export_timer: Option<Instant>,
    /// The converted `project` map and the document revision it was made at.
    project_map: RefCell<Option<(u64, QMap<QMapPair_QString_QVariant>)>>,
    workers: RefCell<Vec<JoinHandle<()>>>,
    wallpaper_jobs: Arc<WallpaperQueue>,
}

impl EditorRust {
    fn new(bundle_path: String) -> EditorRust {
        EditorRust {
            bundle_path,
            document: None,
            error: String::new(),
            transport: Transport::default(),
            playhead_clock: None,
            media_player_source: String::new(),
            pending_scrub_position: -1.0,
            loading: true,
            preview_stats_enabled: false,
            preview_stats_frames: 0,
            preview_stats_cost_ns: 0,
            preview_stats_clock: None,
            camera_thumbnails: QList::default(),
            export_timer: None,
            project_map: RefCell::new(None),
            workers: RefCell::new(Vec::new()),
            wallpaper_jobs: Arc::new(WallpaperQueue::default()),
        }
    }

    /// Runs `work` on a thread of its own, kept to be waited for when the editor goes. A thread
    /// that cannot be started runs it here instead.
    fn spawn(&self, work: impl FnOnce() + Send + 'static) {
        let mut workers = self.workers.borrow_mut();
        workers.retain(|worker| !worker.is_finished());
        let work = Arc::new(Mutex::new(Some(work)));
        let run = {
            let work = Arc::clone(&work);
            move || {
                if let Some(work) = work.lock().ok().and_then(|mut work| work.take()) {
                    work();
                }
            }
        };
        match std::thread::Builder::new().spawn(run) {
            Ok(worker) => workers.push(worker),
            Err(_) => {
                drop(workers);
                if let Some(work) = work.lock().ok().and_then(|mut work| work.take()) {
                    work();
                }
            }
        }
    }
}

impl Drop for EditorRust {
    /// `~Editor`: unsaved edits are written, then the workers are waited for. Nothing is
    /// emitted: the object is being destroyed. EditorBase cancels a running export after this.
    fn drop(&mut self) {
        if let Some(document) = self.document.as_mut() {
            if document.dirty() {
                document.save_now();
            }
        }
        for worker in self.workers.get_mut().drain(..) {
            let _ = worker.join();
        }
    }
}

impl cxx_qt::Constructor<(QString,)> for qobject::Editor {
    type BaseArguments = ();
    type InitializeArguments = ();
    type NewArguments = (QString,);

    fn route_arguments(arguments: (QString,)) -> (Self::NewArguments, Self::BaseArguments, Self::InitializeArguments) {
        (arguments, (), ())
    }

    fn new((bundle_path,): (QString,)) -> EditorRust {
        EditorRust::new(qdir::absolute_path(&bundle_path.to_string()))
    }

    fn initialize(self: Pin<&mut Self>, _: ()) {
        self.open();
    }
}

impl qobject::Editor {
    // ---------------------------------------------------------------------------------------
    // Opening the recording, and performing what the document asks for.

    /// The C++ constructor after its timers and connections, which EditorBase made.
    fn open(mut self: Pin<&mut Self>) {
        let probes = Probes::new(Arc::new(media_probe::probe_video));
        match Document::open(&self.bundle_path, &probes) {
            Ok(document) => self.as_mut().rust_mut().document = Some(document),
            Err(error) => {
                self.as_mut().rust_mut().error = error;
                return;
            }
        }
        // A new project starts its autosave as it loads.
        self.as_mut().drain();
        let Some(document) = self.document.as_ref() else { return };
        let video_path = qstring(document.video_path());
        let camera_path = document.has_camera().then(|| qstring(document.camera_video_path()));
        self.as_mut().playback_load_screen(&video_path);
        let stats = qenv::int_value_or_zero("OMAREEL_PREVIEW_STATS") == 1;
        self.as_mut().rust_mut().preview_stats_enabled = stats;
        if stats {
            print_error("preview stats enabled\n");
        }
        if let Some(camera_path) = camera_path {
            self.as_mut().playback_load_camera(&camera_path);
        }
        self.start_frame_times();
        self.as_mut().rebuild_motion();
        self.start_camera_thumbnail_build();
        self.as_mut().call_later(FINISH_LOADING_CALL, 1500);
    }

    /// Changes the document, then performs what it queued; `None` without a document.
    fn edit<R>(mut self: Pin<&mut Self>, change: impl FnOnce(&mut Document) -> R) -> Option<R> {
        let result = self.as_mut().rust_mut().document.as_mut().map(change)?;
        self.drain();
        Some(result)
    }

    /// Performs the effects the document queued, in order.
    fn drain(mut self: Pin<&mut Self>) {
        let Some(document) = self.document.as_ref() else { return };
        for effect in document.take_effects() {
            self.as_mut().perform(effect);
        }
    }

    fn perform(mut self: Pin<&mut Self>, effect: Effect) {
        match effect {
            Effect::SetScreenAudio { volume, muted } => self.as_mut().playback_set_audio(SCREEN, volume, muted),
            Effect::UpdateInsertAudio => self.as_mut().update_insert_audio(),
            Effect::DirtyChanged => self.as_mut().dirty_changed(),
            Effect::StartAutosaveTimer => self.as_mut().start_editor_timer(AUTOSAVE_TIMER),
            Effect::ProjectChanged => self.as_mut().project_changed(),
            Effect::DurationChanged => self.as_mut().duration_changed(),
            Effect::OutputSizeChanged => self.as_mut().output_size_changed(),
            Effect::StartMotionTimer => self.as_mut().start_editor_timer(MOTION_TIMER),
            Effect::CompositionChanged => self.as_mut().composition_changed(),
            Effect::HistoryChanged => self.as_mut().history_changed(),
            Effect::SelectionChanged => self.as_mut().selection_changed(),
            Effect::PickingZoomTargetChanged => self.as_mut().picking_zoom_target_changed(),
            Effect::PlaybackRangeChanged => self.as_mut().playback_range_changed(),
            Effect::PresetsChanged => self.as_mut().presets_changed(),
            Effect::WallpapersChanged => self.as_mut().wallpapers_changed(),
            Effect::ImportingChanged => self.as_mut().importing_changed(),
            Effect::VideoThumbnailRevisionChanged => self.as_mut().video_thumbnail_revision_changed(),
            Effect::ExportingChanged => self.as_mut().exporting_changed(),
            Effect::ExportProgressChanged => self.as_mut().export_progress_changed(),
            Effect::ExportErrorChanged => self.as_mut().export_error_changed(),
            Effect::ExportFinished(path) => self.as_mut().export_finished(qstring(&path)),
            Effect::ErrorOccurred(message) => self.as_mut().error_occurred(qstring(&message)),
            Effect::Autosaved(path) => self.as_mut().autosaved(qstring(&path)),
            Effect::Seek(output_time) => self.as_mut().seek(output_time),
            Effect::Pause => self.as_mut().pause(),
            Effect::SyncCamera { screen_source_time, force } => self.as_mut().sync_camera(screen_source_time, force),
            Effect::StartNextVideoThumbnail => self.as_mut().start_next_video_thumbnail(),
            Effect::StartImport(source) => self.start_import(&source),
            Effect::GenerateWallpaperThumbnail { path, thumbnail } => self.generate_wallpaper_thumbnail(path, thumbnail),
        }
    }

    // ---------------------------------------------------------------------------------------
    // Worker threads.

    /// `probeFrameTimes(screen.mp4)` off the GUI thread.
    fn start_frame_times(&self) {
        let Some(document) = self.document.as_ref() else { return };
        let video_path = document.video_path().to_owned();
        let thread = self.qt_thread();
        self.spawn(move || {
            let times = media_probe::probe_frame_times(&video_path);
            let _ = thread.queue(move |mut editor| {
                if let Some(document) = editor.as_mut().rust_mut().document.as_mut() {
                    document.set_screen_frame_times(times);
                }
            });
        });
    }

    /// `rebuildMotion()`: builds the motion track off the GUI thread, unless a build runs.
    fn rebuild_motion(mut self: Pin<&mut Self>) {
        let Some(job) = self.as_mut().rust_mut().document.as_mut().and_then(Document::motion_job) else { return };
        let thread = self.qt_thread();
        self.spawn(move || {
            let track = job.run();
            let _ = thread.queue(move |editor| {
                editor.edit(|document| document.apply_motion(track));
            });
        });
    }

    /// `startCameraThumbnailBuild()`.
    fn start_camera_thumbnail_build(&self) {
        let Some(job) = self.document.as_ref().and_then(Document::camera_thumbnail_job) else { return };
        let thread = self.qt_thread();
        self.spawn(move || {
            let urls = job.run();
            let _ = thread.queue(move |mut editor| {
                editor.as_mut().rust_mut().camera_thumbnails = url_list(&urls);
                editor.camera_thumbnails_changed();
            });
        });
    }

    /// `startNextVideoThumbnail()`: the next queued filmstrip frame, unless playing or scrubbing
    /// or a frame is being built.
    fn start_next_video_thumbnail(mut self: Pin<&mut Self>) {
        let (playing, scrubbing) = (self.transport.playing, self.transport.scrubbing);
        let Some(job) = self
            .as_mut()
            .rust_mut()
            .document
            .as_mut()
            .and_then(|document| document.next_video_thumbnail_job(playing, scrubbing))
        else {
            return;
        };
        let thread = self.qt_thread();
        self.spawn(move || {
            let succeeded = job.run();
            let _ = thread.queue(move |editor| {
                editor.edit(|document| document.finish_video_thumbnail(succeeded));
            });
        });
    }

    /// The import [`Effect::StartImport`] asks for, then `finishImport()`.
    fn start_import(&self, source: &str) {
        let Some(job) = self.document.as_ref().map(|document| document.import_job(source)) else { return };
        let thread = self.qt_thread();
        self.spawn(move || {
            let imported = job.run();
            let _ = thread.queue(move |editor| {
                editor.edit(|document| document.finish_import(imported));
            });
        });
    }

    /// A wallpaper picker thumbnail, generated off the GUI thread.
    fn generate_wallpaper_thumbnail(&self, path: String, thumbnail: String) {
        let queue = Arc::clone(&self.wallpaper_jobs);
        let start_worker = {
            let Ok(mut state) = queue.state.lock() else { return };
            state.0.push_back((path, thumbnail));
            let limit = std::thread::available_parallelism().map_or(1, usize::from);
            let start = state.1 < limit;
            if start {
                state.1 += 1;
            }
            start
        };
        if !start_worker {
            return;
        }
        let thread = self.qt_thread();
        self.spawn(move || {
            loop {
                let job = match queue.state.lock() {
                    Ok(mut state) => {
                        let job = state.0.pop_front();
                        if job.is_none() {
                            state.1 -= 1;
                        }
                        job
                    }
                    Err(_) => None,
                };
                let Some((path, thumbnail)) = job else { break };
                qobject::generate_wallpaper_thumbnail(&qstring(&path), &qstring(&thumbnail));
                let _ = thread.queue(move |editor| {
                    editor.edit(|document| document.wallpaper_thumbnail_ready(&thumbnail));
                });
            }
        });
    }

    // ---------------------------------------------------------------------------------------
    // Properties.

    fn is_valid(&self) -> bool {
        self.document.is_some()
    }

    fn error_string(&self) -> QString {
        qstring(&self.error)
    }

    /// `projectMap()`: the project's JSON as a map, with `renderTheme`, the resolved background
    /// image and the timeline's clip extras; kept until the document changes.
    fn project_map(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(document) = self.document.as_ref() else { return QMap::default() };
        let revision = document.project_map_revision();
        if let Some((cached, map)) = self.project_map.borrow().as_ref() {
            if *cached == revision {
                return map.clone();
            }
        }
        let mut result = json_map(&document.project_map());
        // A Theme built here used to set the application font as it loaded; the composition's
        // text without a theme of its own falls back to that font.
        crate::theme::set_application_font(&qobject::interface_family(), crate::theme::body_text_size());
        let (background, foreground) = document.render_theme();
        result.insert(
            qstring("renderTheme"),
            QVariant::from(&map([
                ("background", QVariant::from(&to_qt(&background))),
                ("foreground", QVariant::from(&to_qt(&foreground))),
            ])),
        );
        *self.project_map.borrow_mut() = Some((revision, result.clone()));
        result
    }

    fn project_list(&self, key: &str) -> QList<QVariant> {
        self.project_map()
            .get(&qstring(key))
            .map(|value| value.value_or_default::<QList<QVariant>>())
            .unwrap_or_default()
    }

    fn clips(&self) -> QList<QVariant> {
        self.project_list("clips")
    }

    fn zooms(&self) -> QList<QVariant> {
        self.project_list("zooms")
    }

    fn bundle_path(&self) -> QString {
        qstring(&self.bundle_path)
    }

    fn bundle_name(&self) -> QString {
        match self.document.as_ref() {
            Some(document) => qstring(document.bundle_name()),
            None => qstring(&Project::default().name),
        }
    }

    fn duration(&self) -> f64 {
        self.document.as_ref().map_or(0.0, Document::duration)
    }

    fn source_duration(&self) -> f64 {
        self.document.as_ref().map_or(0.0, Document::source_duration)
    }

    fn source_width(&self) -> i32 {
        self.document.as_ref().map_or(0, Document::source_width)
    }

    fn source_height(&self) -> i32 {
        self.document.as_ref().map_or(0, Document::source_height)
    }

    fn capture_scale(&self) -> f64 {
        self.document.as_ref().map_or(1.0, Document::capture_scale)
    }

    fn fps(&self) -> f64 {
        self.document.as_ref().map_or(60.0, Document::fps)
    }

    fn has_audio(&self) -> bool {
        self.document.as_ref().is_some_and(Document::has_audio)
    }

    fn has_desktop_audio(&self) -> bool {
        self.document.as_ref().is_some_and(Document::has_desktop_audio)
    }

    fn has_microphone_audio(&self) -> bool {
        self.document.as_ref().is_some_and(Document::has_microphone_audio)
    }

    fn has_camera(&self) -> bool {
        self.document.as_ref().is_some_and(Document::has_camera)
    }

    fn camera_thumbnails(&self) -> QList<QVariant> {
        self.camera_thumbnails.clone()
    }

    fn video_thumbnail_revision(&self) -> i32 {
        self.document.as_ref().map_or(0, Document::video_thumbnail_revision)
    }

    fn loading(&self) -> bool {
        self.loading
    }

    fn importing(&self) -> bool {
        self.document.as_ref().is_some_and(Document::importing)
    }

    fn import_error(&self) -> QString {
        self.document.as_ref().map_or_else(QString::default, |document| qstring(document.import_error()))
    }

    fn position(&self) -> f64 {
        self.document.as_ref().map_or(0.0, Document::output_position)
    }

    fn playhead_position(&self) -> f64 {
        self.transport.playhead_position
    }

    fn playing(&self) -> bool {
        self.transport.playing
    }

    fn has_playback_range(&self) -> bool {
        self.document.as_ref().is_some_and(Document::has_playback_range)
    }

    fn can_undo(&self) -> bool {
        self.document.as_ref().is_some_and(Document::can_undo)
    }

    fn can_redo(&self) -> bool {
        self.document.as_ref().is_some_and(Document::can_redo)
    }

    fn dirty(&self) -> bool {
        self.document.as_ref().is_some_and(Document::dirty)
    }

    fn selected_clip_id(&self) -> QString {
        self.document.as_ref().map_or_else(QString::default, |document| qstring(document.selected_clip_id()))
    }

    fn selected_zoom_id(&self) -> QString {
        self.document.as_ref().map_or_else(QString::default, |document| qstring(document.selected_zoom_id()))
    }

    fn selected_zoom_ids(&self) -> QStringList {
        self.document.as_ref().map_or_else(QStringList::default, |document| string_list(document.selected_zoom_ids()))
    }

    fn picking_zoom_target(&self) -> bool {
        self.document.as_ref().is_some_and(Document::picking_zoom_target)
    }

    fn preset_names(&self) -> QStringList {
        self.document.as_ref().map_or_else(QStringList::default, |document| string_list(&document.preset_names()))
    }

    /// `wallpaperGroups`: every theme's wallpapers; the picker thumbnails that are missing start
    /// generating as the list is read.
    fn wallpaper_groups(&self) -> QList<QVariant> {
        let Some(document) = self.document.as_ref() else { return QList::default() };
        let groups = document.wallpaper_groups();
        let mut others = Vec::new();
        for effect in document.take_effects() {
            if let Effect::GenerateWallpaperThumbnail { path, thumbnail } = effect {
                self.generate_wallpaper_thumbnail(path, thumbnail);
            } else {
                others.push(effect);
            }
        }
        // A getter can't perform the rest; they stay queued for the next call that drains.
        document.requeue_effects(others);
        groups
            .iter()
            .map(|group| {
                let wallpapers: QList<QVariant> = group
                    .wallpapers
                    .iter()
                    .map(|wallpaper| {
                        QVariant::from(&map([
                            ("name", QVariant::from(&qstring(&wallpaper.name))),
                            ("file", QVariant::from(&qstring(&wallpaper.file))),
                            ("path", QVariant::from(&qstring(&wallpaper.path))),
                            ("url", QVariant::from(&qstring(&wallpaper.url))),
                            ("thumbnailUrl", QVariant::from(&qstring(&wallpaper.thumbnail_url))),
                        ]))
                    })
                    .collect();
                QVariant::from(&map([
                    ("name", QVariant::from(&qstring(&group.name))),
                    ("label", QVariant::from(&qstring(&group.label))),
                    ("current", QVariant::from(&group.current)),
                    ("wallpapers", QVariant::from(&wallpapers)),
                ]))
            })
            .collect()
    }

    fn gradients(&self) -> QList<QVariant> {
        match self.document.as_ref() {
            Some(document) => document.gradients().iter().map(json_variant).collect(),
            None => editor::gradient_presets().iter().map(json_variant).collect(),
        }
    }

    fn export_progress(&self) -> f64 {
        self.document.as_ref().map_or(0.0, |document| document.export_state().progress)
    }

    fn exporting(&self) -> bool {
        self.document.as_ref().is_some_and(|document| document.export_state().exporting)
    }

    fn export_error(&self) -> QString {
        self.document.as_ref().map_or_else(QString::default, |document| qstring(&document.export_state().error))
    }

    fn export_fps(&self) -> f64 {
        self.document.as_ref().map_or(0.0, |document| document.export_state().fps)
    }

    fn export_eta_seconds(&self) -> i32 {
        self.document.as_ref().map_or(0, |document| document.export_state().eta_seconds)
    }

    fn exported_path(&self) -> QString {
        self.document
            .as_ref()
            .map_or_else(QString::default, |document| qstring(&document.export_state().exported_path))
    }

    fn output_width(&self) -> i32 {
        self.document.as_ref().map_or(2, Document::output_width)
    }

    fn output_height(&self) -> i32 {
        720
    }

    fn source_position(&self) -> f64 {
        self.document.as_ref().map_or(0.0, Document::source_position)
    }

    fn preview_zoom(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(document) = self.document.as_ref() else { return QMap::default() };
        let zoom = document.preview_zoom();
        map([
            ("scale", QVariant::from(&zoom.scale)),
            ("cx", QVariant::from(&zoom.cx)),
            ("cy", QVariant::from(&zoom.cy)),
            ("velocity", QVariant::from(&zoom.velocity)),
        ])
    }

    fn preview_layout(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(document) = self.document.as_ref() else { return QMap::default() };
        let layout = document.preview_layout();
        map([("cameraFull", QVariant::from(&layout.camera_full)), ("bubble", QVariant::from(&layout.bubble))])
    }

    fn preview_media(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(document) = self.document.as_ref() else { return QMap::default() };
        let media = document.preview_media(self.playback_media_frame_ready(), &self.media_player_source);
        map([
            ("active", QVariant::from(&media.active)),
            ("ready", QVariant::from(&media.ready)),
            ("fit", QVariant::from(&qstring(&media.fit))),
            ("width", QVariant::from(&media.width)),
            ("height", QVariant::from(&media.height)),
            ("missing", QVariant::from(&media.missing)),
        ])
    }

    fn preview_cursor(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(document) = self.document.as_ref() else { return QMap::default() };
        let cursor = document.preview_cursor();
        map([
            ("x", QVariant::from(&cursor.x)),
            ("y", QVariant::from(&cursor.y)),
            ("visible", QVariant::from(&cursor.visible)),
            ("scale", QVariant::from(&cursor.scale)),
            ("opacity", QVariant::from(&cursor.opacity)),
            ("rotation", QVariant::from(&cursor.rotation)),
        ])
    }

    fn preview_ripples(&self) -> QList<QVariant> {
        let Some(document) = self.document.as_ref() else { return QList::default() };
        document
            .preview_ripples()
            .iter()
            .map(|ripple| {
                QVariant::from(&map([
                    ("x", QVariant::from(&ripple.x)),
                    ("y", QVariant::from(&ripple.y)),
                    ("progress", QVariant::from(&ripple.progress)),
                ]))
            })
            .collect()
    }

    fn preview_keystroke_pills(&self) -> QList<QVariant> {
        let Some(document) = self.document.as_ref() else { return QList::default() };
        document
            .preview_keystroke_pills()
            .iter()
            .map(|pill| {
                QVariant::from(&map([
                    ("keys", QVariant::from(&string_list(&pill.keys))),
                    ("text", QVariant::from(&qstring(&pill.text))),
                    ("opacity", QVariant::from(&pill.opacity)),
                ]))
            })
            .collect()
    }

    fn software_rendering(&self) -> bool {
        qobject::software_rendering()
    }

    fn camera_source_width(&self) -> i32 {
        self.document.as_ref().map_or(0, Document::camera_source_width)
    }

    fn camera_source_height(&self) -> i32 {
        self.document.as_ref().map_or(0, Document::camera_source_height)
    }

    fn camera_status(&self) -> QString {
        match self.document.as_ref() {
            Some(document) => qstring(&document.camera_status()),
            None => qstring("No webcam in this bundle"),
        }
    }

    fn camera_visible(&self) -> bool {
        self.document.as_ref().is_some_and(Document::camera_visible)
    }

    fn screen_frame_times_ready_for_tests(&self) -> bool {
        self.document.as_ref().is_some_and(Document::screen_frame_times_ready)
    }

    fn set_selected_clip_id(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.edit(|document| document.set_selected_clip_id(&id));
    }

    fn set_selected_zoom_id(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.edit(|document| document.set_selected_zoom_id(&id));
    }

    fn set_selected_zoom_ids(self: Pin<&mut Self>, ids: &QStringList) {
        let ids: Vec<String> = ids.iter().map(QString::to_string).collect();
        self.edit(|document| document.set_selected_zoom_ids(&ids));
    }

    fn set_picking_zoom_target(self: Pin<&mut Self>, value: bool) {
        self.edit(|document| document.set_picking_zoom_target(value));
    }

    // ---------------------------------------------------------------------------------------
    // Editing.

    fn set_project_value(self: Pin<&mut Self>, path: &QString, value: &QVariant) {
        self.set_project_value_coalescing(path, value, false);
    }

    /// `setProjectValue(path, value, coalesce)`: `value` goes through `QJsonValue::fromVariant`
    /// as it did in the C++ (a local file URL as its path for the background image paths).
    fn set_project_value_coalescing(self: Pin<&mut Self>, path: &QString, value: &QVariant, coalesce: bool) {
        let path = path.to_string();
        let json = qobject::project_value_json(value, takes_local_file(&path)).to_string();
        let value = match qjson::from_json(json.as_bytes()) {
            Ok(Value::Array(mut items)) if items.len() == 1 => items.remove(0),
            _ => Value::Null,
        };
        self.edit(|document| document.set_project_value(&path, value, coalesce));
    }

    fn apply_gradient_preset(self: Pin<&mut Self>, index: i32) {
        self.edit(|document| document.apply_gradient_preset(index));
    }

    fn begin_coalesced_edit(self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        self.edit(|document| document.begin_coalesced_edit(&key));
    }

    fn end_coalesced_edit(self: Pin<&mut Self>) {
        self.edit(Document::end_coalesced_edit);
    }

    fn undo(self: Pin<&mut Self>) {
        self.edit(Document::undo);
    }

    fn redo(self: Pin<&mut Self>) {
        self.edit(Document::redo);
    }

    fn save_now(self: Pin<&mut Self>) -> bool {
        self.edit(Document::save_now).unwrap_or(false)
    }

    fn set_playback_range(self: Pin<&mut Self>, from: f64, to: f64) {
        self.edit(|document| document.set_playback_range(from, to));
    }

    fn clear_playback_range(self: Pin<&mut Self>) {
        self.edit(Document::clear_playback_range);
    }

    fn video_thumbnail_url(self: Pin<&mut Self>, source_time: f64) -> QString {
        let url = self.edit(|document| document.video_thumbnail_url(source_time));
        qstring(&url.unwrap_or_default())
    }

    fn video_thumbnail_url_for_clip(self: Pin<&mut Self>, source_time: f64, source_start: f64, source_end: f64) -> QString {
        let url = self.edit(|document| document.video_thumbnail_url_for_clip(source_time, source_start, source_end));
        qstring(&url.unwrap_or_default())
    }

    fn clip_thumbnail_url(
        self: Pin<&mut Self>,
        clip_id: &QString,
        source_time: f64,
        source_start: f64,
        source_end: f64,
    ) -> QString {
        let clip_id = clip_id.to_string();
        let url = self.edit(|document| document.clip_thumbnail_url(&clip_id, source_time, source_start, source_end));
        qstring(&url.unwrap_or_default())
    }

    fn output_to_source(&self, output_time: f64) -> f64 {
        self.document.as_ref().map_or(0.0, |document| document.output_to_source(output_time))
    }

    fn source_to_output(&self, source_time: f64) -> f64 {
        self.source_to_output_in_clip(source_time, -1)
    }

    fn source_to_output_in_clip(&self, source_time: f64, preferred_clip: i32) -> f64 {
        self.document.as_ref().map_or(-1.0, |document| document.source_to_output(source_time, preferred_clip))
    }

    fn source_range_to_output(&self, source_start: f64, source_end: f64) -> QPointF {
        let (start, end) = self
            .document
            .as_ref()
            .map_or((-1.0, -1.0), |document| document.source_range_to_output(source_start, source_end));
        QPointF::new(start, end)
    }

    fn split_at_playhead(self: Pin<&mut Self>) -> bool {
        self.edit(Document::split_at_playhead).unwrap_or(false)
    }

    fn move_clip(self: Pin<&mut Self>, id: &QString, source_in: f64) -> bool {
        let id = id.to_string();
        self.edit(|document| document.move_clip(&id, source_in)).unwrap_or(false)
    }

    fn trim_clip(self: Pin<&mut Self>, id: &QString, new_in: f64, new_out: f64) -> bool {
        let id = id.to_string();
        self.edit(|document| document.trim_clip(&id, new_in, new_out)).unwrap_or(false)
    }

    fn remove_clip(self: Pin<&mut Self>, id: &QString) -> bool {
        let id = id.to_string();
        self.edit(|document| document.remove_clip(&id)).unwrap_or(false)
    }

    /// `deleteOutputRange(from, to)`, pausing before the cut as the C++ did.
    fn delete_output_range(mut self: Pin<&mut Self>, from: f64, to: f64) -> bool {
        let Some(cut) = self.as_mut().edit(|document| document.prepare_delete_output_range(from, to)).flatten() else {
            return false;
        };
        self.as_mut().pause();
        self.edit(|document| document.apply_delete_output_range(cut));
        true
    }

    fn merge_clip(self: Pin<&mut Self>, id: &QString, direction: i32) -> bool {
        let id = id.to_string();
        self.edit(|document| document.merge_clip(&id, direction)).unwrap_or(false)
    }

    fn reset_clip_trims(self: Pin<&mut Self>, id: &QString) -> bool {
        let id = id.to_string();
        self.edit(|document| document.reset_clip_trims(&id)).unwrap_or(false)
    }

    fn set_clip_speed(self: Pin<&mut Self>, id: &QString, speed: f64) -> bool {
        let id = id.to_string();
        self.edit(|document| document.set_clip_speed(&id, speed)).unwrap_or(false)
    }

    fn set_clip_layout(self: Pin<&mut Self>, id: &QString, layout: &QString) -> bool {
        let (id, layout) = (id.to_string(), layout.to_string());
        self.edit(|document| document.set_clip_layout(&id, &layout)).unwrap_or(false)
    }

    fn set_clip_fit(self: Pin<&mut Self>, id: &QString, fit: &QString) -> bool {
        let (id, fit) = (id.to_string(), fit.to_string());
        self.edit(|document| document.set_clip_fit(&id, &fit)).unwrap_or(false)
    }

    /// `reorderClip(id, direction)`, pausing before the move as the C++ did.
    fn reorder_clip(mut self: Pin<&mut Self>, id: &QString, direction: i32) -> bool {
        let id = id.to_string();
        if !self.document.as_ref().is_some_and(|document| document.can_reorder_clip(&id, direction)) {
            return false;
        }
        self.as_mut().pause();
        self.edit(|document| document.apply_reorder_clip(&id, direction));
        true
    }

    /// `insertMedia(url)`, pausing before the import starts as the C++ did.
    fn insert_media(mut self: Pin<&mut Self>, url: &QUrl) {
        let source = qobject::url_path(url).to_string();
        if !self.document.as_ref().is_some_and(|document| document.can_insert_media(&source)) {
            return;
        }
        self.as_mut().pause();
        self.edit(|document| document.start_import(&source));
    }

    fn clear_import_error(self: Pin<&mut Self>) {
        self.edit(Document::clear_import_error);
    }

    fn add_zoom_at(self: Pin<&mut Self>, output_time: f64) -> QString {
        self.add_zoom_at_with_length(output_time, 2.0)
    }

    fn add_zoom_at_with_length(self: Pin<&mut Self>, output_time: f64, length: f64) -> QString {
        qstring(&self.edit(|document| document.add_zoom_at(output_time, length)).unwrap_or_default())
    }

    fn move_zoom(self: Pin<&mut Self>, id: &QString, source_start: f64) -> bool {
        let id = id.to_string();
        self.edit(|document| document.move_zoom(&id, source_start)).unwrap_or(false)
    }

    fn resize_zoom(self: Pin<&mut Self>, id: &QString, source_start: f64, source_end: f64) -> bool {
        let id = id.to_string();
        self.edit(|document| document.resize_zoom(&id, source_start, source_end)).unwrap_or(false)
    }

    fn set_zoom_level(self: Pin<&mut Self>, id: &QString, level: f64) -> bool {
        self.set_zoom_level_coalescing(id, level, false)
    }

    fn set_zoom_level_coalescing(self: Pin<&mut Self>, id: &QString, level: f64, coalesce: bool) -> bool {
        let id = id.to_string();
        self.edit(|document| document.set_zoom_level(&id, level, coalesce)).unwrap_or(false)
    }

    fn set_zoom_target(self: Pin<&mut Self>, id: &QString, target: &QVariant) -> bool {
        let id = id.to_string();
        let (mut x, mut y) = (0.5, 0.5);
        let target = if qobject::zoom_target(target, &mut x, &mut y) { ZoomTarget::Auto } else { ZoomTarget::Point(x, y) };
        self.edit(|document| document.set_zoom_target(&id, target)).unwrap_or(false)
    }

    fn set_zoom_target_from_preview(self: Pin<&mut Self>, x: f64, y: f64) -> bool {
        self.edit(|document| document.set_zoom_target_from_preview(x, y)).unwrap_or(false)
    }

    fn remove_zoom(self: Pin<&mut Self>, id: &QString) -> bool {
        let id = id.to_string();
        self.edit(|document| document.remove_zoom(&id)).unwrap_or(false)
    }

    fn select_zooms_in_output_range(self: Pin<&mut Self>, output_start: f64, output_end: f64) {
        self.edit(|document| document.select_zooms_in_output_range(output_start, output_end));
    }

    fn move_selected_zooms(self: Pin<&mut Self>, anchor_id: &QString, source_start: f64) -> bool {
        let anchor_id = anchor_id.to_string();
        self.edit(|document| document.move_selected_zooms(&anchor_id, source_start)).unwrap_or(false)
    }

    fn remove_selected_zooms(self: Pin<&mut Self>) -> i32 {
        self.edit(Document::remove_selected_zooms).unwrap_or(0)
    }

    fn regenerate_zooms(self: Pin<&mut Self>) {
        self.edit(Document::regenerate_zooms);
    }

    fn save_preset(self: Pin<&mut Self>, name: &QString) {
        let name = name.to_string();
        self.edit(|document| document.save_preset(&name));
    }

    fn load_preset(self: Pin<&mut Self>, name: &QString) {
        let name = name.to_string();
        self.edit(|document| document.load_preset(&name));
    }

    fn delete_preset(self: Pin<&mut Self>, name: &QString) {
        let name = name.to_string();
        self.edit(|document| document.delete_preset(&name));
    }

    fn refresh_omarchy_theme(self: Pin<&mut Self>) {
        self.edit(Document::refresh_omarchy_theme);
    }

    // ---------------------------------------------------------------------------------------
    // Exporting and the editor's other text.

    fn export_to(self: Pin<&mut Self>, path: &QString) {
        self.export_to_with_settings(path, &QMap::default());
    }

    /// `exportTo(path, settings)`: saves, then starts an exporter with the dialog's settings.
    fn export_to_with_settings(mut self: Pin<&mut Self>, path: &QString, settings: &QMap<QMapPair_QString_QVariant>) {
        if self.document.as_ref().is_none_or(|document| document.export_state().exporting) {
            return;
        }
        self.as_mut().save_now();
        let path = qobject::text_url_path(path).to_string();
        let (mut fps, mut height, mut quality) = (0, 0, QString::default());
        qobject::export_settings(settings, &mut fps, &mut height, &mut quality);
        let settings = ExportDialogSettings { fps, height, quality: quality.to_string() };
        let request = {
            let mut rust = self.as_mut().rust_mut();
            let Some(document) = rust.document.as_mut() else { return };
            document.begin_saved_export(&path, &settings)
        };
        if request.is_some() {
            self.as_mut().rust_mut().export_timer = Some(Instant::now());
        }
        self.as_mut().drain();
        if let Some(request) = request {
            self.start_export(&ExportOptions {
                bundle_path: qstring(&request.bundle_path),
                output_path: qstring(&request.output_path),
                fps: request.fps,
                width: request.width,
                quality: qstring(&request.quality),
                gif_fps: request.gif_fps,
                gif_width: request.gif_width,
                timing: false,
            });
        }
    }

    fn default_export_path(&self, format: &QString) -> QString {
        let format = format.to_string();
        self.document.as_ref().map_or_else(QString::default, |document| qstring(&document.default_export_path(&format)))
    }

    fn format_time(&self, seconds: f64) -> QString {
        qstring(&format_time(seconds))
    }

    fn open_containing_folder(&self, path: &QString) {
        if let Some(directory) = containing_folder(&path.to_string()) {
            qobject::open_folder(&qstring(&directory));
        }
    }

    fn copy_path(&self, path: &QString) {
        qobject::copy_to_clipboard(path);
    }

    fn trace_input(&self, object_name: &QString, phase: &QString, x: f64, y: f64) {
        if std::env::var_os("OMAREEL_INPUT_TRACE").is_none_or(|value| value != "1") {
            return;
        }
        print_error(&trace_input_line(&object_name.to_string(), &phase.to_string(), x, y));
    }

    fn export_progressed(mut self: Pin<&mut Self>, frame: i32, total: i32) {
        let elapsed_ms = self.export_timer.map_or(0, |timer| timer.elapsed().as_millis() as i64);
        self.as_mut().edit(|document| document.export_progressed(frame, total, elapsed_ms));
    }

    fn export_succeeded(self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        self.edit(|document| document.export_finished(&path));
    }

    fn export_failed_with(self: Pin<&mut Self>, message: &QString) {
        let message = message.to_string();
        self.edit(|document| document.export_failed(&message));
    }

    // ---------------------------------------------------------------------------------------
    // The transport: Editor.cpp's playback code around the document's decisions.

    fn play_pause(self: Pin<&mut Self>) {
        if self.transport.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    fn play(mut self: Pin<&mut Self>) {
        self.as_mut().end_scrub();
        if let Some(target) = self.document.as_ref().and_then(Document::play_start_seek) {
            self.as_mut().seek(target);
        }
        // The opening preview, or a seek from a stalled player, may still be warming up; its
        // pause on the first decoded frame must not stop playback asked for in the meantime.
        self.as_mut().playback_cancel_warm_up(SCREEN);
        self.as_mut().playback_cancel_warm_up(MEDIA);
        let Some(active) = self.document.as_ref().map(|document| document.clip_for_output(document.output_position()).0)
        else {
            return;
        };
        self.as_mut().rust_mut().transport.active_clip = active;
        self.as_mut().set_transport_playing(true);
        if self.clip_is_media(self.transport.active_clip) {
            self.as_mut().playback_play(MEDIA);
            self.as_mut().playback_pause(SCREEN);
        } else {
            self.as_mut().playback_play(SCREEN);
            self.as_mut().playback_pause(MEDIA);
            let source = self.source_position();
            self.as_mut().sync_camera(source, true);
        }
        self.as_mut().update_insert_audio();
        self.prepare_next_clip();
    }

    fn pause(mut self: Pin<&mut Self>) {
        self.as_mut().set_transport_playing(false);
        self.as_mut().playback_pause(SCREEN);
        self.as_mut().playback_pause(CAMERA);
        self.playback_pause(MEDIA);
    }

    fn step_frames(mut self: Pin<&mut Self>, frames: i32) {
        self.as_mut().pause();
        if let Some(target) = self.document.as_ref().map(|document| document.step_target(frames)) {
            self.seek(target);
        }
    }

    fn seek_boundary(self: Pin<&mut Self>, direction: i32) {
        if let Some(target) = self.document.as_ref().map(|document| document.boundary_target(direction)) {
            self.seek(target);
        }
    }

    fn clips_empty(&self) -> bool {
        self.document.as_ref().is_none_or(|document| document.project().clips.is_empty())
    }

    fn clip_is_media(&self, index: i32) -> bool {
        self.document.as_ref().is_some_and(|document| document.clip_is_media(index))
    }

    /// `Editor::seek(outputTime)`.
    fn seek(mut self: Pin<&mut Self>, output_time: f64) {
        if self.clips_empty() {
            return;
        }
        self.as_mut().stop_editor_timer(SCRUB_TIMER);
        self.as_mut().rust_mut().pending_scrub_position = -1.0;
        self.as_mut().rust_mut().transport.scrubbing = false;
        let Some(target) = self.document.as_ref().map(|document| document.clamp_playback_position(output_time)) else {
            return;
        };
        self.as_mut().set_output_position(target);
        let position = self.position();
        self.as_mut().seek_media(position);
        self.start_next_video_thumbnail();
    }

    fn begin_scrub(mut self: Pin<&mut Self>) {
        if self.clips_empty() {
            return;
        }
        self.as_mut().pause();
        self.as_mut().rust_mut().transport.scrubbing = true;
    }

    fn scrub_to(mut self: Pin<&mut Self>, output_time: f64) {
        if self.clips_empty() {
            return;
        }
        if !self.transport.scrubbing {
            self.as_mut().begin_scrub();
        }
        let Some(target) = self.document.as_ref().map(|document| document.clamp_playback_position(output_time)) else {
            return;
        };
        self.as_mut().set_output_position(target);
        let position = self.position();
        self.as_mut().rust_mut().pending_scrub_position = position;
        if !self.editor_timer_active(SCRUB_TIMER) {
            self.start_editor_timer(SCRUB_TIMER);
        }
    }

    fn end_scrub(mut self: Pin<&mut Self>) {
        if !self.transport.scrubbing {
            return;
        }
        self.as_mut().rust_mut().transport.scrubbing = false;
        self.as_mut().stop_editor_timer(SCRUB_TIMER);
        self.as_mut().flush_scrub_seek();
        self.start_next_video_thumbnail();
    }

    fn flush_scrub_seek(mut self: Pin<&mut Self>) {
        if self.pending_scrub_position < 0.0 {
            return;
        }
        let target = self.pending_scrub_position;
        self.as_mut().rust_mut().pending_scrub_position = -1.0;
        self.seek_media(target);
    }

    /// `setOutputPosition(outputTime)`.
    fn set_output_position(mut self: Pin<&mut Self>, output_time: f64) {
        let Some(active) = self.as_mut().rust_mut().document.as_mut().map(|document| document.set_output_position(output_time))
        else {
            return;
        };
        self.as_mut().rust_mut().transport.active_clip = active;
        let position = self.position();
        self.as_mut().reset_playhead_clock(position);
        self.as_mut().position_changed();
        self.composition_changed();
    }

    fn set_playhead_position(mut self: Pin<&mut Self>, output_time: f64) {
        let current = self.transport.playhead_position;
        let Some(position) = self.document.as_ref().and_then(|document| document.playhead_update(current, output_time))
        else {
            return;
        };
        self.as_mut().rust_mut().transport.playhead_position = position;
        self.playhead_position_changed();
    }

    fn reset_playhead_clock(mut self: Pin<&mut Self>, output_time: f64) {
        self.as_mut().rust_mut().transport.playhead_clock_position = output_time;
        self.as_mut().rust_mut().playhead_clock = Some(Instant::now());
        self.set_playhead_position(output_time);
    }

    fn update_playhead_position(self: Pin<&mut Self>) {
        let Some(clock) = self.playhead_clock else { return };
        if !self.transport.playing || self.transport.scrubbing || self.clips_empty() {
            return;
        }
        let elapsed_ms = clock.elapsed().as_millis() as i64;
        let transport = self.transport;
        if let Some(target) = self.document.as_ref().and_then(|document| document.playhead_advance(&transport, elapsed_ms)) {
            self.set_playhead_position(target);
        }
    }

    /// `seekMedia(outputTime)`: shows the clip there in its player.
    fn seek_media(mut self: Pin<&mut Self>, output_time: f64) {
        let Some(seek) = self.document.as_ref().and_then(|document| document.media_seek(output_time)) else { return };
        self.as_mut().rust_mut().transport.internal_seek = true;
        // While playing, the clip's player starts before the other stops, so the picture never
        // waits on a paused pair.
        if let Some(media) = &seek.media {
            self.as_mut().load_insert(media);
            self.as_mut().playback_set_rate(MEDIA, seek.speed);
            let playing = self.transport.playing;
            self.as_mut().playback_seek(MEDIA, seek.target_ms, playing);
            if self.transport.playing {
                self.as_mut().playback_play(MEDIA);
            }
            self.as_mut().playback_pause(SCREEN);
            self.as_mut().playback_pause(CAMERA);
        } else {
            self.as_mut().playback_set_rate(SCREEN, seek.speed);
            let playing = self.transport.playing;
            self.as_mut().playback_seek(SCREEN, seek.target_ms, playing);
            if self.transport.playing {
                self.as_mut().playback_play(SCREEN);
            }
            self.as_mut().playback_pause(MEDIA);
            self.as_mut().sync_camera(seek.source, true);
        }
        self.as_mut().update_insert_audio();
        self.as_mut().rust_mut().transport.internal_seek = false;
        self.prepare_next_clip();
    }

    /// `loadInsert(path)`: the media player opens an inserted video.
    fn load_insert(mut self: Pin<&mut Self>, path: &str) {
        if self.media_player_source == path {
            return;
        }
        self.as_mut().rust_mut().media_player_source = path.to_owned();
        let file = qstring(&qdir::file_path(&self.bundle_path, path));
        self.as_mut().playback_load_media(&file);
        self.composition_changed();
    }

    /// `prepareNextClip()`: readies the next clip's player when the cut changes players.
    fn prepare_next_clip(mut self: Pin<&mut Self>) {
        let active = self.transport.active_clip;
        let Some(next) = self.document.as_ref().and_then(|document| document.next_clip(active)) else { return };
        match &next.kind {
            NextClipKind::IntoMedia(media) => {
                // The inserted clip after this one opens and waits on its first frame, so the cut
                // to it is immediate.
                if self.media_player_source == *media
                    && self.playback_media_frame_ready()
                    && self.playback_state(MEDIA) == PAUSED_STATE
                    && (self.playback_position(MEDIA) - next.target_ms).abs() <= 1
                {
                    return;
                }
                self.as_mut().load_insert(media);
                self.as_mut().playback_set_rate(MEDIA, next.speed);
                self.playback_seek(MEDIA, next.target_ms, false);
            }
            NextClipKind::IntoRecording => {
                // Likewise the screen waits where the recording resumes.
                if self.playback_state(SCREEN) != PLAYING_STATE {
                    self.as_mut().playback_set_rate(SCREEN, next.speed);
                    self.playback_seek(SCREEN, next.target_ms, false);
                }
            }
        }
    }

    /// `advanceClip()`: on to the next clip, or the end.
    fn advance_clip(mut self: Pin<&mut Self>) {
        if !self.transport.playing {
            return;
        }
        let active = self.transport.active_clip;
        let Some(document) = self.document.as_ref() else { return };
        match document.advance_target(active) {
            Some(target) => self.seek(target),
            None => {
                let duration = document.duration();
                self.as_mut().pause();
                self.set_output_position(duration);
            }
        }
    }

    /// `updateInsertAudio()`: silent while the inserted video decodes ahead of its clip or warms
    /// up on a paused frame.
    fn update_insert_audio(mut self: Pin<&mut Self>) {
        let warming = self.playback_warming(MEDIA);
        let active = self.transport.active_clip;
        let Some((volume, muted)) = self.document.as_ref().map(|document| document.insert_audio(active, warming)) else {
            return;
        };
        self.as_mut().playback_set_audio(MEDIA, volume, muted);
    }

    /// `setTransportPlaying(value)`: what Space controls; the players follow it as clips change
    /// hands.
    fn set_transport_playing(mut self: Pin<&mut Self>, value: bool) {
        if self.transport.playing == value {
            return;
        }
        self.as_mut().rust_mut().transport.playing = value;
        let position = self.position();
        if value {
            self.as_mut().reset_playhead_clock(position);
            self.as_mut().start_editor_timer(PREVIEW_TIMER);
            if self.preview_stats_enabled {
                let mut rust = self.as_mut().rust_mut();
                rust.preview_stats_frames = 0;
                rust.preview_stats_cost_ns = 0;
                rust.preview_stats_clock = Some(Instant::now());
                self.as_mut().start_editor_timer(PREVIEW_STATS_TIMER);
            }
        } else {
            self.as_mut().stop_editor_timer(PREVIEW_TIMER);
            self.as_mut().stop_editor_timer(PREVIEW_STATS_TIMER);
            self.as_mut().rust_mut().playhead_clock = None;
            self.as_mut().set_playhead_position(position);
        }
        self.as_mut().update_insert_audio();
        self.as_mut().playing_changed();
        if !value {
            self.start_next_video_thumbnail();
        }
    }

    /// `syncCamera(screenSourceTime, force)`.
    fn sync_camera(self: Pin<&mut Self>, screen_source_time: f64, force: bool) {
        let Some(sync) = self.document.as_ref().and_then(|document| document.camera_sync(screen_source_time)) else {
            return;
        };
        self.playback_sync_camera(sync.target_ms, sync.tolerance_ms, force, sync.covered);
    }

    /// `handlePlayerPosition(milliseconds)`.
    fn handle_player_position(mut self: Pin<&mut Self>, milliseconds: i64) {
        let transport = self.transport;
        let Some(step) = self.document.as_ref().and_then(|document| document.player_position(&transport, milliseconds))
        else {
            return;
        };
        if let Some(end) = step.stop_at_range_end {
            self.as_mut().pause();
            self.seek(end);
            return;
        }
        if let Some(source) = step.sync_camera {
            self.as_mut().sync_camera(source, false);
        }
        if step.advance {
            self.advance_clip();
            return;
        }
        if let Some(output) = step.position {
            if let Some(document) = self.as_mut().rust_mut().document.as_mut() {
                document.follow_output_position(output);
            }
            self.as_mut().position_changed();
            self.composition_changed();
        }
    }

    /// The preview timer's tick: follow the active player, then move the playhead.
    fn preview_tick(mut self: Pin<&mut Self>) {
        // While an inserted clip's file is still opening, its player has no position yet.
        let player = self.document.as_ref().map_or(SCREEN, |document| player_index(document.player_for(self.transport.active_clip)));
        if self.playback_position_ready(player) {
            let position = self.playback_position(player);
            self.as_mut().handle_player_position(position);
        }
        self.update_playhead_position();
    }

    /// `handlePreviewStats()`.
    fn handle_preview_stats(mut self: Pin<&mut Self>) {
        let elapsed_ms = self.preview_stats_clock.map_or(0, |clock| clock.elapsed().as_millis() as i64);
        let Some(line) = preview_stats_line(self.preview_stats_frames, self.preview_stats_cost_ns, elapsed_ms, self.fps())
        else {
            return;
        };
        print_error(&line);
        let mut rust = self.as_mut().rust_mut();
        rust.preview_stats_frames = 0;
        rust.preview_stats_cost_ns = 0;
        rust.preview_stats_clock = Some(Instant::now());
    }

    fn finish_loading(mut self: Pin<&mut Self>) {
        if qenv::int_value_or_zero("OMAREEL_SCREENSHOT_LOADING") == 1 || !self.loading {
            return;
        }
        self.as_mut().rust_mut().loading = false;
        self.loading_changed();
    }

    // ---------------------------------------------------------------------------------------
    // What EditorBase reports.

    fn player_should_play(&self, player: i32) -> bool {
        self.document
            .as_ref()
            .is_some_and(|document| document.player_should_play(player_from_index(player), &self.transport))
    }

    fn screen_rest_position(&self) -> i64 {
        self.document.as_ref().map_or(0, Document::screen_rest_ms)
    }

    /// `handleFrameShown(player, startTimeUs, live)`: counts the screen's frames for the preview
    /// stats, and anchors the playhead clock to a frame the active clip's player shows while
    /// playing.
    fn handle_frame_shown(mut self: Pin<&mut Self>, player: i32, start_time_us: i64, live: bool) {
        let player = player_from_index(player);
        let cost = Instant::now();
        if player == Player::Screen {
            self.as_mut().rust_mut().preview_stats_frames += 1;
        }
        if player != Player::Camera {
            let transport = self.transport;
            let anchor =
                self.document.as_ref().and_then(|document| document.frame_anchor(player, &transport, start_time_us, live));
            if let Some(anchor) = anchor {
                let mut rust = self.as_mut().rust_mut();
                rust.transport.playhead_clock_position = anchor;
                rust.playhead_clock = Some(Instant::now());
            }
        }
        if player == Player::Screen {
            self.as_mut().rust_mut().preview_stats_cost_ns += cost.elapsed().as_nanos() as i64;
        }
    }

    fn handle_media_frame_ready(self: Pin<&mut Self>) {
        self.composition_changed();
    }

    fn handle_warming_changed(self: Pin<&mut Self>) {
        self.update_insert_audio();
    }

    fn handle_media_status(mut self: Pin<&mut Self>, player: i32, status: i32) {
        if self.document.is_none() {
            return;
        }
        let active = self.transport.active_clip;
        if player == SCREEN {
            if [LOADED_MEDIA, BUFFERED_MEDIA, END_OF_MEDIA, INVALID_MEDIA].contains(&status) {
                self.as_mut().call_later(FINISH_LOADING_CALL, 0);
            }
            // The video can end a little before the clip's out point; the clip is over either way.
            if status == END_OF_MEDIA && self.transport.playing && !self.clip_is_media(active) {
                self.call_later(ADVANCE_CLIP_CALL, 0);
            }
        } else if player == MEDIA && status == END_OF_MEDIA && self.transport.playing && self.clip_is_media(active) {
            self.call_later(ADVANCE_CLIP_CALL, 0);
        }
    }

    fn handle_playback_state(self: Pin<&mut Self>, player: i32) {
        if self.preview_stats_enabled && player == SCREEN {
            print_error(&format!("preview state={}\n", self.playback_state(SCREEN)));
        }
    }

    fn camera_output_attached(mut self: Pin<&mut Self>) {
        let source = self.source_position();
        self.as_mut().sync_camera(source, true);
        if self.has_camera() {
            self.playback_warm_up(CAMERA);
        }
    }

    fn media_output_attached(self: Pin<&mut Self>) {
        if self.clip_is_media(self.transport.active_clip) {
            let position = self.position();
            self.seek_media(position);
        }
    }

    fn timer_fired(self: Pin<&mut Self>, timer: i32) {
        match timer {
            AUTOSAVE_TIMER => {
                self.save_now();
            }
            MOTION_TIMER => self.rebuild_motion(),
            SCRUB_TIMER => self.flush_scrub_seek(),
            PREVIEW_TIMER => self.preview_tick(),
            PREVIEW_STATS_TIMER => self.handle_preview_stats(),
            _ => {}
        }
    }

    fn called_later(self: Pin<&mut Self>, call: i32) {
        match call {
            FINISH_LOADING_CALL => self.finish_loading(),
            ADVANCE_CLIP_CALL => self.advance_clip(),
            _ => {}
        }
    }
}
