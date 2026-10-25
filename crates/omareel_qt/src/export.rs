//! The exporter's logic for C++ (`omareel_core::export`), and the media probe the editor uses.
//!
//! `Exporter` (src/render/Exporter.cpp) keeps the Qt side: the renderer, the event loop and the
//! signals. It drives an [`ExportJob`] through the calls below in the order `Exporter::run` had
//! them, and copies the composition state into the `QVariantMap`s Composition.qml reads. Nothing
//! here calls back into C++ or runs the event loop, so no Rust frame is on the stack while Qt
//! processes events; `CancelFlag::cancel` only sets an atomic and may be called from anywhere.

use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use omareel_core::compat::qjson::Value;
use omareel_core::export::{self, FrameImages, Image};
use omareel_core::media_probe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        // QtShims.h first: it brings in QString whole before cxx-qt-lib's qstring.h, whose
        // QStringList include otherwise makes GCC 16 warn about an incomplete QChar.
        include!("QtShims.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
        type QList_f64 = cxx_qt_lib::QList<f64>;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        #[namespace = "Omareel::Shims"]
        #[cxx_name = "nullVariant"]
        fn null_variant() -> QVariant;
    }

    /// What an inserted video holds, as ffmpeg will decode it (`MediaProbe.h`'s `VideoInfo`).
    /// `width` and `height` are the display size, rotation and sample aspect applied; `duration`
    /// is the video stream's, the container's when the stream has none.
    #[namespace = "Omareel"]
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct VideoInfo {
        width: i32,
        height: i32,
        duration: f64,
        fps: f64,
        audio: bool,
    }

    #[namespace = "Omareel"]
    extern "Rust" {
        #[cxx_name = "isValid"]
        fn is_valid(self: &VideoInfo) -> bool;
        /// `probeVideo(path)`: a default `VideoInfo` (30 fps) when the file can't be probed.
        #[cxx_name = "probeVideo"]
        fn probe_video(path: &QString) -> VideoInfo;
        /// `probeVideo(path, &error)`: `error` says why the file can't be used, or is cleared.
        #[cxx_name = "probeVideo"]
        fn probe_video_with_error(path: &QString, error: &mut QString) -> VideoInfo;
        /// When each frame of the first video stream starts, in seconds and in order; empty when
        /// the file can't be read.
        #[cxx_name = "probeFrameTimes"]
        fn probe_frame_times(path: &QString) -> QList_f64;
    }

    /// `ExportOptions`.
    #[namespace = "omareel::exporter"]
    struct ExportOptions {
        bundle_path: QString,
        output_path: QString,
        fps: i32,
        width: i32,
        quality: QString,
        gif_fps: i32,
        gif_width: i32,
        timing: bool,
    }

    #[namespace = "omareel::exporter"]
    struct EvenSize {
        width: i32,
        height: i32,
    }

    /// The scalar members of `CompositionState`; the maps and lists have their own calls.
    #[namespace = "omareel::exporter"]
    struct Composition {
        output_width: i32,
        output_height: i32,
        source_width: i32,
        source_height: i32,
        capture_scale: f64,
        camera_available: bool,
        camera_visible: bool,
        camera_source_width: i32,
        camera_source_height: i32,
        time: f64,
    }

    #[namespace = "omareel::exporter"]
    extern "Rust" {
        /// The even size an encoder needs: odd dimensions grow by one.
        #[cxx_name = "paddedEvenSize"]
        fn padded_even_size(width: i32, height: i32) -> EvenSize;

        /// The exporter's cancel flag. `cancel` may be called from any thread at any time.
        type CancelFlag;
        #[cxx_name = "newCancelFlag"]
        fn new_cancel_flag() -> Box<CancelFlag>;
        fn cancel(self: &CancelFlag);
        fn reset(self: &CancelFlag);

        /// A decoded frame: tightly packed RGBA rows, `width` * 4 bytes each.
        type DecodedImage;
        #[cxx_name = "isNull"]
        fn is_null(self: &DecodedImage) -> bool;
        fn width(self: &DecodedImage) -> i32;
        fn height(self: &DecodedImage) -> i32;
        fn pixels(self: &DecodedImage) -> &[u8];

        type ExportJob;
        #[cxx_name = "newExportJob"]
        fn new_export_job() -> Box<ExportJob>;
        /// Loads the bundle and works out the output; false with `error` set when it can't be
        /// exported. Cancelling `cancel` stops the job.
        fn prepare(self: &mut ExportJob, options: &ExportOptions, cancel: &CancelFlag, error: &mut QString) -> bool;
        #[cxx_name = "totalFrames"]
        fn total_frames(self: &ExportJob) -> i32;
        fn composition(self: &ExportJob) -> Composition;
        /// `project.toJson().toVariantMap()`.
        #[cxx_name = "projectMap"]
        fn project_map(self: &ExportJob) -> QMap_QString_QVariant;
        /// The local file the background draws, empty for none.
        #[cxx_name = "backgroundImage"]
        fn background_image(self: &ExportJob) -> QString;
        #[cxx_name = "zoomMap"]
        fn zoom_map(self: &ExportJob) -> QMap_QString_QVariant;
        #[cxx_name = "layoutMap"]
        fn layout_map(self: &ExportJob) -> QMap_QString_QVariant;
        #[cxx_name = "mediaMap"]
        fn media_map(self: &ExportJob) -> QMap_QString_QVariant;
        #[cxx_name = "cursorMap"]
        fn cursor_map(self: &ExportJob) -> QMap_QString_QVariant;
        fn ripples(self: &ExportJob) -> QList_QVariant;
        #[cxx_name = "keystrokePills"]
        fn keystroke_pills(self: &ExportJob) -> QList_QVariant;
        /// Once the renderer is ready: temporary files, the encoder and the decoder.
        fn start(self: &mut ExportJob, software_rendering: bool, error: &mut QString) -> bool;
        /// Waits for frame `frame` and updates the composition; false with `error` set to end.
        #[cxx_name = "nextFrame"]
        fn next_frame(self: &mut ExportJob, frame: i32, error: &mut QString) -> bool;
        /// The current frame's pictures; a null one leaves that layer as it was.
        #[cxx_name = "screenImage"]
        fn screen_image(self: &ExportJob) -> Box<DecodedImage>;
        #[cxx_name = "cameraImage"]
        fn camera_image(self: &ExportJob) -> Box<DecodedImage>;
        #[cxx_name = "insertImage"]
        fn insert_image(self: &ExportJob) -> Box<DecodedImage>;
        /// Whether the encoder can take a frame, waiting up to `timeout_ms`.
        #[cxx_name = "waitEncodeSpace"]
        fn wait_encode_space(self: &ExportJob, timeout_ms: u64) -> bool;
        /// Hands over a rendered frame; the error that ends the export, or empty.
        fn encode(self: &mut ExportJob, bytes: QByteArray) -> QString;
        #[cxx_name = "finishDecoder"]
        fn finish_decoder(self: &mut ExportJob);
        #[cxx_name = "closeEncoder"]
        fn close_encoder(self: &mut ExportJob, abort: bool);
        #[cxx_name = "encoderDone"]
        fn encoder_done(self: &ExportJob) -> bool;
        #[cxx_name = "waitEncoderDone"]
        fn wait_encoder_done(self: &ExportJob, timeout_ms: u64);
        #[cxx_name = "joinEncoder"]
        fn join_encoder(self: &mut ExportJob);
        /// Writes the output file once the encoder is done; false with `error` set.
        fn finish(self: &mut ExportJob, error: &mut QString) -> bool;
        #[cxx_name = "timingReport"]
        fn timing_report(self: &ExportJob, upload_ns: i64, render_ns: i64, readback_ns: i64) -> QString;
    }
}

use ffi::{Composition, EvenSize, ExportOptions, VideoInfo};

fn video_info(info: &media_probe::VideoInfo) -> VideoInfo {
    VideoInfo { width: info.width, height: info.height, duration: info.duration, fps: info.fps, audio: info.audio }
}

impl VideoInfo {
    fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.duration > 0.0
    }
}

fn probe_video(path: &QString) -> VideoInfo {
    video_info(&media_probe::probe_video(&path.to_string()).unwrap_or_default())
}

fn probe_video_with_error(path: &QString, error: &mut QString) -> VideoInfo {
    match media_probe::probe_video(&path.to_string()) {
        Ok(info) => {
            *error = QString::default();
            video_info(&info)
        }
        Err(message) => {
            *error = QString::from(&message);
            video_info(&media_probe::VideoInfo::default())
        }
    }
}

fn probe_frame_times(path: &QString) -> QList<f64> {
    media_probe::probe_frame_times(&path.to_string()).into_iter().collect()
}

fn padded_even_size(width: i32, height: i32) -> EvenSize {
    let (width, height) = export::plan::padded_even_size(width, height);
    EvenSize { width, height }
}

pub struct CancelFlag(Arc<AtomicBool>);

fn new_cancel_flag() -> Box<CancelFlag> {
    Box::new(CancelFlag(Arc::new(AtomicBool::new(false))))
}

impl CancelFlag {
    fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub struct DecodedImage(Option<Image>);

impl DecodedImage {
    fn is_null(&self) -> bool {
        self.0.is_none()
    }

    fn width(&self) -> i32 {
        self.0.as_ref().map_or(0, |image| image.width)
    }

    fn height(&self) -> i32 {
        self.0.as_ref().map_or(0, |image| image.height)
    }

    fn pixels(&self) -> &[u8] {
        self.0.as_ref().map_or(&[], |image| image.pixels.as_slice())
    }
}

/// A rendered frame on its way to the encode thread. `QByteArray` is reentrant and its data is
/// shared with an atomic count, so the one handle can move to another thread and drop there.
struct SendBytes(QByteArray);

// SAFETY: see above; nothing else holds this handle once it is moved in.
unsafe impl Send for SendBytes {}

impl AsRef<[u8]> for SendBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// `omareel_core::export::ExportJob` once `prepare` succeeded, and the pictures of the frame
/// `next_frame` last gave.
pub struct ExportJob {
    job: Option<export::ExportJob>,
    frame: FrameImages,
}

fn new_export_job() -> Box<ExportJob> {
    Box::new(ExportJob { job: None, frame: FrameImages::default() })
}

fn qstring(text: &str) -> QString {
    QString::from(text)
}

/// `QJsonValue::toVariant`: integers as qlonglong, doubles, null as a null `QVariant` QML reads
/// as `null`, arrays as `QVariantList` and objects as `QVariantMap`.
fn json_variant(value: &Value) -> QVariant {
    match value {
        Value::Undefined => QVariant::default(),
        Value::Null => ffi::null_variant(),
        Value::Bool(b) => QVariant::from(b),
        Value::Int(n) => QVariant::from(n),
        Value::Double(d) => QVariant::from(d),
        Value::String(s) => QVariant::from(&qstring(s)),
        Value::Array(items) => QVariant::from(&items.iter().map(json_variant).collect::<QList<QVariant>>()),
        Value::Object(object) => QVariant::from(&json_map(object)),
    }
}

fn json_map(object: &omareel_core::compat::qjson::Object) -> QMap<QMapPair_QString_QVariant> {
    object.iter().map(|(key, value)| (qstring(key), json_variant(value))).collect()
}

fn map<const N: usize>(entries: [(&str, QVariant); N]) -> QMap<QMapPair_QString_QVariant> {
    entries.into_iter().map(|(key, value)| (qstring(key), value)).collect()
}

impl ExportJob {
    fn state(&self) -> Option<&export::composition::CompositionState> {
        self.job.as_ref().map(export::ExportJob::state)
    }

    fn prepare(&mut self, options: &ExportOptions, cancel: &CancelFlag, error: &mut QString) -> bool {
        let options = export::ExportOptions {
            bundle_path: options.bundle_path.to_string(),
            output_path: options.output_path.to_string(),
            fps: options.fps,
            width: options.width,
            quality: options.quality.to_string(),
            gif_fps: options.gif_fps,
            gif_width: options.gif_width,
            timing: options.timing,
        };
        match export::ExportJob::prepare(&options, Arc::clone(&cancel.0)) {
            Ok(job) => {
                self.job = Some(job);
                true
            }
            Err(message) => {
                *error = qstring(&message);
                false
            }
        }
    }

    fn total_frames(&self) -> i32 {
        self.job.as_ref().map_or(0, export::ExportJob::total_frames)
    }

    fn composition(&self) -> Composition {
        let Some(s) = self.state() else {
            return Composition {
                output_width: 0,
                output_height: 0,
                source_width: 0,
                source_height: 0,
                capture_scale: 1.0,
                camera_available: false,
                camera_visible: true,
                camera_source_width: 0,
                camera_source_height: 0,
                time: 0.0,
            };
        };
        Composition {
            output_width: s.output_width,
            output_height: s.output_height,
            source_width: s.source_width,
            source_height: s.source_height,
            capture_scale: s.capture_scale,
            camera_available: s.camera_available,
            camera_visible: s.camera_visible,
            camera_source_width: s.camera_source_width,
            camera_source_height: s.camera_source_height,
            time: s.time,
        }
    }

    fn project_map(&self) -> QMap<QMapPair_QString_QVariant> {
        self.job.as_ref().map(|job| json_map(&job.project().to_json())).unwrap_or_default()
    }

    fn background_image(&self) -> QString {
        self.job.as_ref().map(|job| qstring(&job.background_image())).unwrap_or_default()
    }

    fn zoom_map(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(s) = self.state() else { return QMap::default() };
        let z = &s.zoom;
        map([
            ("scale", QVariant::from(&z.scale)),
            ("cx", QVariant::from(&z.cx)),
            ("cy", QVariant::from(&z.cy)),
            ("velocity", QVariant::from(&z.velocity)),
        ])
    }

    fn layout_map(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(s) = self.state() else { return QMap::default() };
        map([("cameraFull", QVariant::from(&s.layout.camera_full)), ("bubble", QVariant::from(&s.layout.bubble))])
    }

    fn media_map(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(s) = self.state() else { return QMap::default() };
        let m = &s.media;
        map([
            ("active", QVariant::from(&m.active)),
            ("ready", QVariant::from(&m.ready)),
            ("fit", QVariant::from(&qstring(&m.fit))),
            ("width", QVariant::from(&m.width)),
            ("height", QVariant::from(&m.height)),
        ])
    }

    fn cursor_map(&self) -> QMap<QMapPair_QString_QVariant> {
        let Some(s) = self.state() else { return QMap::default() };
        let c = &s.cursor;
        map([
            ("x", QVariant::from(&c.x)),
            ("y", QVariant::from(&c.y)),
            ("visible", QVariant::from(&c.visible)),
            ("scale", QVariant::from(&c.scale)),
            ("opacity", QVariant::from(&c.opacity)),
            ("rotation", QVariant::from(&c.rotation)),
        ])
    }

    fn ripples(&self) -> QList<QVariant> {
        let Some(s) = self.state() else { return QList::default() };
        s.ripples
            .iter()
            .map(|r| {
                QVariant::from(&map([
                    ("x", QVariant::from(&r.x)),
                    ("y", QVariant::from(&r.y)),
                    ("progress", QVariant::from(&r.progress)),
                ]))
            })
            .collect()
    }

    fn keystroke_pills(&self) -> QList<QVariant> {
        let Some(s) = self.state() else { return QList::default() };
        s.keystroke_pills
            .iter()
            .map(|pill| {
                let keys: QStringList = pill.keys.iter().map(|key| qstring(key)).collect();
                QVariant::from(&map([
                    ("keys", QVariant::from(&keys)),
                    ("text", QVariant::from(&qstring(&pill.text))),
                    ("opacity", QVariant::from(&pill.opacity)),
                ]))
            })
            .collect()
    }

    fn start(&mut self, software_rendering: bool, error: &mut QString) -> bool {
        let Some(job) = self.job.as_mut() else { return false };
        match job.start(software_rendering) {
            Ok(()) => true,
            Err(message) => {
                *error = qstring(&message);
                false
            }
        }
    }

    fn next_frame(&mut self, frame: i32, error: &mut QString) -> bool {
        let Some(job) = self.job.as_mut() else { return false };
        match job.next_frame(frame) {
            Ok(images) => {
                self.frame = images;
                true
            }
            Err(message) => {
                self.frame = FrameImages::default();
                *error = qstring(&message);
                false
            }
        }
    }

    fn screen_image(&self) -> Box<DecodedImage> {
        Box::new(DecodedImage(self.frame.screen.clone()))
    }

    fn camera_image(&self) -> Box<DecodedImage> {
        Box::new(DecodedImage(self.frame.camera.clone()))
    }

    fn insert_image(&self) -> Box<DecodedImage> {
        Box::new(DecodedImage(self.frame.insert.clone()))
    }

    fn wait_encode_space(&self, timeout_ms: u64) -> bool {
        self.job.as_ref().is_none_or(|job| job.wait_encode_space(timeout_ms))
    }

    fn encode(&mut self, bytes: QByteArray) -> QString {
        let Some(job) = self.job.as_mut() else { return QString::default() };
        match job.encode(Box::new(SendBytes(bytes))) {
            Ok(()) => QString::default(),
            Err(message) => qstring(&message),
        }
    }

    fn finish_decoder(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.finish_decoder();
        }
    }

    fn close_encoder(&mut self, abort: bool) {
        if let Some(job) = self.job.as_mut() {
            job.close_encoder(abort);
        }
    }

    fn encoder_done(&self) -> bool {
        self.job.as_ref().is_none_or(export::ExportJob::encoder_done)
    }

    fn wait_encoder_done(&self, timeout_ms: u64) {
        if let Some(job) = self.job.as_ref() {
            job.wait_encoder_done(timeout_ms);
        }
    }

    fn join_encoder(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.join_encoder();
        }
    }

    fn finish(&mut self, error: &mut QString) -> bool {
        let Some(job) = self.job.as_mut() else { return false };
        match job.finish() {
            Ok(()) => true,
            Err(message) => {
                *error = qstring(&message);
                false
            }
        }
    }

    fn timing_report(&self, upload_ns: i64, render_ns: i64, readback_ns: i64) -> QString {
        self.job.as_ref().map(|job| qstring(&job.timing_report(upload_ns, render_ns, readback_ns))).unwrap_or_default()
    }
}
