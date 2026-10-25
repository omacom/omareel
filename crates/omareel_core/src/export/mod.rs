//! Exporting a recording bundle to MP4 or GIF (`Exporter::run`): everything but the rendering.
//!
//! [`ExportJob`] loads the bundle and works out the output ([`ExportJob::prepare`]), then, once
//! the caller's renderer is ready, starts ffmpeg ([`ExportJob::start`]): a decode thread turns the
//! recording, the webcam video and inserted clips into RGBA frames three frames ahead, and for
//! MP4 an encode thread feeds an ffmpeg encoder two frames behind. The caller runs the frame
//! loop: [`next_frame`](ExportJob::next_frame) gives each frame's pictures and updates
//! [`state`](ExportJob::state), the caller renders them and hands the pixels to
//! [`encode`](ExportJob::encode), waiting with [`wait_encode_space`](ExportJob::wait_encode_space)
//! where the C++ ran the event loop. [`finish`](ExportJob::finish) writes the file.
//!
//! The order of everything, the ffmpeg arguments, the error messages and the files are the
//! C++'s. Cancelling only sets the shared flag, so it is safe from any thread at any time; the
//! threads notice it where the C++ threads did.

pub mod composition;
pub mod decoder;
pub mod encoder_pipe;
pub mod plan;

use crate::clip_timeline::ClipTimeline;
use crate::compat::qdir::{self, FileInfo};
use crate::compat::qnumber::{self, number_f64};
use crate::compat::qprocess::Process;
use crate::compat::qround::q_round;
use crate::compat::qtemporaryfile::TemporaryFile;
use crate::compat::{cpp, qfile, qjson, qstandardpaths, qstring};
use crate::camera_timeline::map_camera_time;
use crate::input_log::InputLog;
use crate::keystroke_track::KeystrokeTrack;
use crate::media_probe::{self, VideoInfo};
use crate::motion_track::MotionTrack;
use crate::omarchy_paths;
use crate::project::{Clip, Project};
use crate::zoom_timeline;
use composition::{CompositionState, Sources};
use decoder::FfmpegDecoder;
use encoder_pipe::{FINISH_TIMEOUT_MS, STALL_TIMEOUT_MS, finish_encoder_pipe, write_encoder_frame};
use plan::MediaInfo;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The click sound mixed in for `cursor.clickSound` "soft" (the app's `:/omareel/assets/...`).
const CLICK_SOUND: &[u8] = include_bytes!("../../../../assets/sounds/click.wav");

/// `ExportOptions`: zero and empty mean "as the project says".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportOptions {
    pub bundle_path: String,
    pub output_path: String,
    pub fps: i32,
    pub width: i32,
    pub quality: String,
    pub gif_fps: i32,
    pub gif_width: i32,
    pub timing: bool,
}

/// A decoded frame: tightly packed RGBA rows, shared by the frames that repeat it.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: i32,
    pub height: i32,
    pub pixels: Arc<Vec<u8>>,
}

/// The pictures of one frame for the composition; a missing one leaves that layer as it was.
#[derive(Clone, Debug, Default)]
pub struct FrameImages {
    pub screen: Option<Image>,
    pub camera: Option<Image>,
    pub insert: Option<Image>,
}

/// One rendered frame for the encoder, owned by whoever made it.
pub type FrameBytes = Box<dyn AsRef<[u8]> + Send>;

/// What the decode thread hands over per frame. `clip` says which of `image` (the recording)
/// and `insert_image` the frame shows; -1 with `error` when decoding stopped.
struct DecodedPacket {
    image: Option<Image>,
    camera_image: Option<Image>,
    error: String,
    clip: i32,
    insert_image: Option<Image>,
}

impl DecodedPacket {
    fn failed(error: String) -> DecodedPacket {
        DecodedPacket { image: None, camera_image: None, error, clip: -1, insert_image: None }
    }
}

impl Default for DecodedPacket {
    fn default() -> DecodedPacket {
        DecodedPacket::failed(String::new())
    }
}

struct DecodeQueue {
    packets: VecDeque<DecodedPacket>,
    done: bool,
}

struct DecodeShared {
    queue: Mutex<DecodeQueue>,
    ready: Condvar,
    space: Condvar,
    stop: AtomicBool,
}

struct EncodeQueue {
    frames: VecDeque<FrameBytes>,
    started: bool,
    start_ok: bool,
    closing: bool,
    error: String,
}

struct EncodeShared {
    queue: Mutex<EncodeQueue>,
    ready: Condvar,
    space: Condvar,
    abort: AtomicBool,
    done: AtomicBool,
    /// Time spent writing frames, for `--timing`.
    write_ns: AtomicI64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn nanos(since: Instant) -> i64 {
    i64::try_from(since.elapsed().as_nanos()).unwrap_or(i64::MAX)
}

/// The decode size for a source of `size` scaled by `scale`: even, at least 2, at most the
/// source.
fn even_scaled(size: i32, scale: f64) -> i32 {
    cpp::min(size, cpp::max(2, q_round(f64::from(size) * scale / 2.0).wrapping_mul(2)))
}

/// What the decode thread works from.
struct DecodePlan {
    clips: Vec<Clip>,
    timeline: ClipTimeline,
    inserts: HashMap<String, VideoInfo>,
    bundle: String,
    video_path: String,
    camera_path: String,
    media: MediaInfo,
    camera_media: MediaInfo,
    camera_enabled: bool,
    camera_offset: f64,
    decode_width: i32,
    decode_height: i32,
    camera_decode_width: i32,
    camera_decode_height: i32,
    width: i32,
    height: i32,
    fps: i32,
    total_frames: i32,
}

pub struct ExportJob {
    options: ExportOptions,
    cancelled: Arc<AtomicBool>,
    total_timer: Instant,
    decode_ns: i64,
    bundle: String,
    video_path: String,
    camera_path: String,
    media: MediaInfo,
    project: Project,
    timeline: ClipTimeline,
    output_duration: f64,
    inserts: HashMap<String, VideoInfo>,
    input: InputLog,
    camera_offset: f64,
    camera_media: MediaInfo,
    camera_enabled: bool,
    motion: MotionTrack,
    keystrokes: KeystrokeTrack,
    gif: bool,
    fps: i32,
    width: i32,
    height: i32,
    total_frames: i32,
    decode_width: i32,
    decode_height: i32,
    camera_decode_width: i32,
    camera_decode_height: i32,
    state: CompositionState,
    software_rendering: bool,
    raw_file: Option<TemporaryFile>,
    click_file: Option<TemporaryFile>,
    encoded_file: Option<TemporaryFile>,
    encoded_path: String,
    encode: Arc<EncodeShared>,
    encode_thread: Option<JoinHandle<()>>,
    decode: Arc<DecodeShared>,
    decode_thread: Option<JoinHandle<()>>,
}

impl ExportJob {
    /// Probes the recording, loads the bundle (or makes a project for one without
    /// `project.json`), probes inserted clips and the webcam video, builds the motion and
    /// keystroke tracks, and works out the output and decode sizes. `cancelled` is the
    /// exporter's flag, which [`cancel`](Self::cancel) sets too.
    pub fn prepare(options: &ExportOptions, cancelled: Arc<AtomicBool>) -> Result<ExportJob, String> {
        let total_timer = Instant::now();
        let bundle = options.bundle_path.clone();
        let video_path = qdir::file_path(&bundle, "screen.mp4");
        let camera_path = qdir::file_path(&bundle, "camera.mp4");
        let (media, probe_error) = plan::probe(&video_path);
        if media.width <= 0 {
            return Err(probe_error);
        }

        let project_path = qdir::file_path(&bundle, "project.json");
        let mut project;
        if FileInfo::new(&project_path).is_file() {
            let (loaded, result) = Project::load(&project_path);
            result?;
            project = loaded;
        } else {
            project = Project::defaults(&qdir::dir_name(&bundle), media.duration);
            project.camera.enabled = FileInfo::new(&camera_path).is_file();
            if let Ok(bytes) = qfile::read_all(qdir::file_path(&bundle, "capture.json")) {
                let camera = qjson::object_from_json(&bytes).unwrap_or_default().value("camera").to_object();
                let rotation = camera.value("rotation").to_int(0);
                if matches!(rotation, 0 | 90 | 180 | 270) {
                    project.camera.rotation = rotation;
                }
                project.camera.flip_horizontal = camera.value("flipHorizontal").to_bool(false);
                let shape = camera.value("shape").to_string_or("round");
                if matches!(shape.as_str(), "round" | "square" | "rectangle") {
                    project.camera.shape = shape;
                }
                project.camera.radius = camera.value("radius").to_double(16.0);
            }
            let input = InputLog::load_bundle(&qdir::absolute_path(&bundle), media.duration)?;
            let clicks: Vec<f64> = input.click_downs(true).iter().map(|event| event.time).collect();
            project.zooms = zoom_timeline::generate(&clicks, media.duration);
        }
        if project.clips.is_empty() {
            project.clips = vec![Clip { id: "c1".into(), in_: 0.0, out: media.duration, speed: 1.0, ..Clip::default() }];
        }
        let timeline = ClipTimeline::new(project.clips.clone());
        let output_duration = timeline.total_duration();
        if output_duration <= 0.0 {
            return Err("Project has no exportable clips".into());
        }
        let mut inserts: HashMap<String, VideoInfo> = HashMap::new();
        for clip in &project.clips {
            if !clip.is_media() || inserts.contains_key(&clip.media) {
                continue;
            }
            let info = media_probe::probe_video(&qdir::file_path(&bundle, &clip.media))
                .map_err(|error| format!("Inserted clip: {error}"))?;
            inserts.insert(clip.media.clone(), info);
        }

        let input = InputLog::load_bundle(&qdir::absolute_path(&bundle), media.duration)?;
        let mut capture_fps = 60.0;
        let mut capture_scale = 1.0;
        let mut camera_offset = 0.0;
        if let Ok(bytes) = qfile::read_all(qdir::file_path(&bundle, "capture.json")) {
            let capture = qjson::object_from_json(&bytes).unwrap_or_default();
            capture_fps = capture.value("fps").to_double(60.0);
            capture_scale = capture.value("scale").to_double(1.0);
            let screen_first = capture.value("first_frame_us").to_variant_long_long();
            let camera_first = capture.value("camera").to_object().value("first_frame_us").to_variant_long_long();
            if screen_first > 0 && camera_first > 0 {
                camera_offset = camera_first.wrapping_sub(screen_first) as f64 / 1000000.0;
            }
        }
        let camera_media =
            if FileInfo::new(&camera_path).is_file() { plan::probe(&camera_path).0 } else { MediaInfo::default() };
        let camera_full_frame = project.clips.iter().any(|clip| clip.layout == "camera");
        // Camera clips show the webcam even when its bubble is switched off.
        let camera_enabled = camera_media.width > 0 && (project.camera.enabled || camera_full_frame);
        let motion = MotionTrack::build(media.duration, media.width, media.height, input.events(), &project, capture_fps);
        let keystrokes = KeystrokeTrack::build(input.events(), &project.keystrokes);

        let output = options.output_path.as_bytes();
        let gif = output.len() >= 4 && output[output.len() - 4..].eq_ignore_ascii_case(b".gif");
        let fps = if options.fps > 0 {
            options.fps
        } else if gif && options.gif_fps > 0 {
            options.gif_fps
        } else if gif {
            project.export_settings.gif.fps
        } else {
            project.export_settings.fps
        };
        let aspect = plan::output_aspect(&project.aspect, media.width, media.height, &project.crop);
        let mut width = if gif && options.gif_width > 0 { options.gif_width } else { options.width };
        let height;
        if width > 0 {
            height = q_round(f64::from(width) / aspect);
        } else {
            height = if gif { project.export_settings.gif.height } else { project.export_settings.height };
            width = q_round(f64::from(height) * aspect);
        }
        let (width, height) = plan::padded_even_size(width, height);
        let total_frames = cpp::max(1, cpp::int_cast((output_duration * f64::from(fps)).ceil()));

        let mut maximum_zoom = 1.0;
        for zoom in &project.zooms {
            maximum_zoom = cpp::max(maximum_zoom, zoom.level);
        }
        let needed_width = maximum_zoom * f64::from(width) / cpp::max(0.0001, project.crop.width);
        let needed_height = maximum_zoom * f64::from(height) / cpp::max(0.0001, project.crop.height);
        let decode_scale = cpp::clamp(
            cpp::max(needed_width / f64::from(media.width), needed_height / f64::from(media.height)),
            0.01,
            1.0,
        );
        let decode_width = even_scaled(media.width, decode_scale);
        let decode_height = even_scaled(media.height, decode_scale);
        // A bubble needs a fraction of the output height; a full-frame camera has to cover the
        // output once rotated.
        let camera_quarter_turn = project.camera.rotation == 90 || project.camera.rotation == 270;
        let camera_display_width =
            f64::from(if camera_quarter_turn { camera_media.height } else { camera_media.width });
        let camera_display_height =
            f64::from(if camera_quarter_turn { camera_media.width } else { camera_media.height });
        let camera_decode_scale = if camera_enabled {
            cpp::clamp(
                cpp::max(
                    f64::from(height) * project.camera.size * 1.5 / camera_display_height,
                    if camera_full_frame {
                        cpp::max(f64::from(width) / camera_display_width, f64::from(height) / camera_display_height)
                    } else {
                        0.0
                    },
                ),
                0.01,
                1.0,
            )
        } else {
            1.0
        };
        let camera_decode_width = if camera_enabled { even_scaled(camera_media.width, camera_decode_scale) } else { 0 };
        let camera_decode_height = if camera_enabled { even_scaled(camera_media.height, camera_decode_scale) } else { 0 };

        // Composition.qml reads the state as it loads.
        let mut state = CompositionState::initial(&project);
        state.output_width = width;
        state.output_height = height;
        state.source_width = media.width;
        state.source_height = media.height;
        state.capture_scale = if capture_scale > 0.0 { capture_scale } else { 1.0 };
        state.camera_available = camera_media.width > 0;
        state.camera_source_width = camera_media.width;
        state.camera_source_height = camera_media.height;

        Ok(ExportJob {
            options: options.clone(),
            cancelled,
            total_timer,
            decode_ns: 0,
            bundle,
            video_path,
            camera_path,
            media,
            project,
            timeline,
            output_duration,
            inserts,
            input,
            camera_offset,
            camera_media,
            camera_enabled,
            motion,
            keystrokes,
            gif,
            fps,
            width,
            height,
            total_frames,
            decode_width,
            decode_height,
            camera_decode_width,
            camera_decode_height,
            state,
            software_rendering: false,
            raw_file: None,
            click_file: None,
            encoded_file: None,
            encoded_path: String::new(),
            encode: Arc::new(EncodeShared {
                queue: Mutex::new(EncodeQueue {
                    frames: VecDeque::new(),
                    started: false,
                    start_ok: false,
                    closing: false,
                    error: String::new(),
                }),
                ready: Condvar::new(),
                space: Condvar::new(),
                abort: AtomicBool::new(false),
                done: AtomicBool::new(false),
                write_ns: AtomicI64::new(0),
            }),
            encode_thread: None,
            decode: Arc::new(DecodeShared {
                queue: Mutex::new(DecodeQueue { packets: VecDeque::new(), done: false }),
                ready: Condvar::new(),
                space: Condvar::new(),
                stop: AtomicBool::new(false),
            }),
            decode_thread: None,
        })
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn total_frames(&self) -> i32 {
        self.total_frames
    }

    pub fn is_gif(&self) -> bool {
        self.gif
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// What the composition shows: the initial state until the first frame, then the last
    /// frame's.
    pub fn state(&self) -> &CompositionState {
        &self.state
    }

    /// The local image the background draws, "" for none: the project's image, or its
    /// wallpaper, "omarchy:current" being the wallpaper Omarchy shows now.
    pub fn background_image(&self) -> String {
        let background = &self.project.background;
        if background.type_ == "image" {
            background.image.clone()
        } else if background.type_ == "wallpaper" {
            if background.wallpaper == "omarchy:current" {
                omarchy_paths::current_background()
            } else {
                background.wallpaper.clone()
            }
        } else {
            String::new()
        }
    }

    /// Stops the export at the next point the C++ checked its flag. Safe from any thread.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Once the renderer is ready: the temporary files, the encoder's arguments (with the audio
    /// mix and the NVENC probe), the encode thread for MP4 (waiting until ffmpeg has started)
    /// and the decode thread. `software_rendering` frames come top-down; OpenGL's are flipped.
    pub fn start(&mut self, software_rendering: bool) -> Result<(), String> {
        self.software_rendering = software_rendering;
        let gif = self.gif;
        let mut runtime = qdir::env_var("XDG_RUNTIME_DIR").unwrap_or_default();
        if runtime.is_empty() || !FileInfo::new(&runtime).is_writable() {
            runtime = qdir::temp_path();
        }
        let raw_file = self.raw_file.insert(TemporaryFile::new(&qdir::file_path(&runtime, "omareel-export-XXXXXX.rgba")));
        let click_file = self.click_file.insert(TemporaryFile::new(&qdir::file_path(&runtime, "omareel-click-XXXXXX.wav")));
        let encoded_file = self.encoded_file.insert(TemporaryFile::new(&qdir::file_path(
            &FileInfo::new(&self.options.output_path).absolute_path(),
            ".omareel-export-XXXXXX.mp4",
        )));
        self.encoded_path = self.options.output_path.clone();
        if !gif {
            if !encoded_file.open() {
                return Err(encoded_file.error_string());
            }
            self.encoded_path = encoded_file.file_name();
            encoded_file.close();
        }
        let project = &self.project;
        let mut encoder_args: Vec<String> = Vec::new();
        let use_source_audio = !gif && self.media.audio && (project.audio.desktop || project.audio.mic);
        let click_times = if !gif && project.cursor.click_sound == "soft" {
            plan::click_output_times(&self.input, &project.clips)
        } else {
            Vec::new()
        };
        let use_clicks = !click_times.is_empty();
        if gif {
            if !raw_file.open() {
                return Err(raw_file.error_string());
            }
        } else {
            encoder_args = [
                "-y",
                "-v",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s:v",
                &format!("{}x{}", self.width, self.height),
                "-r",
                &self.fps.to_string(),
                "-i",
                "-",
            ]
            .map(String::from)
            .into();
            let mut next_input = 1;
            let mut source_input = -1;
            let mut click_input = -1;
            // One audio segment per clip: the recording's audio, an inserted video's own audio,
            // or silence. Each inserted clip opens its file at its in point, so containers that
            // do not start at zero need no timestamp arithmetic.
            let mut audio_segments: Vec<String> = Vec::new();
            let mut any_audio = false;
            for clip in &project.clips {
                let length = (clip.out - clip.in_) / clip.speed;
                if clip.is_media() && self.inserts.get(&clip.media).is_some_and(|info| info.audio) {
                    let input = next_input;
                    next_input += 1;
                    encoder_args.extend([
                        "-ss".into(),
                        number_f64(clip.in_, 'f', 6),
                        "-t".into(),
                        number_f64(clip.out - clip.in_, 'f', 6),
                        "-i".into(),
                        qdir::file_path(&self.bundle, &clip.media),
                    ]);
                    audio_segments.push(plan::audio_segment(
                        &format!("[{input}:a:0]asetpts=PTS-STARTPTS"),
                        clip.speed,
                        length,
                    ));
                    any_audio = true;
                } else if !clip.is_media() && use_source_audio {
                    if source_input < 0 {
                        source_input = next_input;
                        next_input += 1;
                        encoder_args.extend(["-i".into(), self.video_path.clone()]);
                    }
                    audio_segments.push(plan::audio_segment(
                        &format!(
                            "[{source_input}:a:0]atrim=start={}:end={},asetpts=PTS-STARTPTS",
                            number_f64(clip.in_, 'f', 6),
                            number_f64(clip.out, 'f', 6)
                        ),
                        clip.speed,
                        length,
                    ));
                    any_audio = true;
                } else {
                    audio_segments.push(plan::silent_segment(length));
                }
            }
            if !any_audio {
                audio_segments.clear();
            }
            if use_clicks {
                let prepare_failed = || "Could not prepare the click sound".to_string();
                if !click_file.open() {
                    return Err(prepare_failed());
                }
                if click_file.write(CLICK_SOUND) != CLICK_SOUND.len() as i64 || !click_file.flush() {
                    return Err(prepare_failed());
                }
                click_file.close();
                click_input = next_input;
                encoder_args.extend(["-i".into(), click_file.file_name()]);
            }
            if any_audio || use_clicks {
                encoder_args.extend([
                    "-filter_complex".into(),
                    plan::audio_filter(&audio_segments, project.audio.volume, &click_times, click_input, self.output_duration),
                    "-map".into(),
                    "0:v:0".into(),
                    "-map".into(),
                    "[aout]".into(),
                    "-c:a".into(),
                    "aac".into(),
                    "-b:a".into(),
                    "192k".into(),
                ]);
            } else {
                encoder_args.push("-an".into());
            }
            if !software_rendering {
                encoder_args.extend(["-vf".into(), "vflip".into()]);
            }
            let quality = plan::normalized_quality(if self.options.quality.is_empty() {
                &project.export_settings.quality
            } else {
                &self.options.quality
            });
            let bitrate = cpp::int64_cast(
                (f64::from(self.width) * f64::from(self.height) * f64::from(self.fps) * plan::bitrate_multiplier(&quality))
                    .floor(),
            );
            let (codec, preset) = if std::env::var_os("OMAREEL_DISABLE_NVENC").is_none() && plan::nvenc_works() {
                ("h264_nvenc", "p5")
            } else {
                ("libx264", "medium")
            };
            encoder_args.extend([
                "-c:v".into(),
                codec.into(),
                "-preset".into(),
                preset.into(),
                "-b:v".into(),
                qnumber::number_i64(bitrate),
                "-maxrate".into(),
                qnumber::number_i64(cpp::int64_cast(bitrate as f64 * 1.2)),
                "-bufsize".into(),
                qnumber::number_i64(bitrate.wrapping_mul(2)),
                "-profile:v".into(),
                "high".into(),
            ]);
            encoder_args.extend([
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-movflags".into(),
                "+faststart".into(),
                self.encoded_path.clone(),
            ]);
        }

        if !gif {
            let shared = Arc::clone(&self.encode);
            let cancelled = Arc::clone(&self.cancelled);
            self.encode_thread = Some(std::thread::spawn(move || encode_thread(encoder_args, &shared, &cancelled)));
            let queue = lock(&self.encode.queue);
            let queue = self.encode.ready.wait_while(queue, |q| !q.started).unwrap_or_else(PoisonError::into_inner);
            if !queue.start_ok {
                let error = queue.error.clone();
                drop(queue);
                if let Some(thread) = self.encode_thread.take() {
                    let _ = thread.join();
                }
                return Err(error);
            }
        }

        let work = DecodePlan {
            clips: self.project.clips.clone(),
            timeline: self.timeline.clone(),
            inserts: self.inserts.clone(),
            bundle: self.bundle.clone(),
            video_path: self.video_path.clone(),
            camera_path: self.camera_path.clone(),
            media: self.media,
            camera_media: self.camera_media,
            camera_enabled: self.camera_enabled,
            camera_offset: self.camera_offset,
            decode_width: self.decode_width,
            decode_height: self.decode_height,
            camera_decode_width: self.camera_decode_width,
            camera_decode_height: self.camera_decode_height,
            width: self.width,
            height: self.height,
            fps: self.fps,
            total_frames: self.total_frames,
        };
        let shared = Arc::clone(&self.decode);
        let cancelled = Arc::clone(&self.cancelled);
        self.decode_thread = Some(std::thread::spawn(move || decode_thread(&work, &shared, &cancelled)));
        Ok(())
    }

    /// Frame `frame` of the output: waits for the decoder, then updates the composition state.
    /// An error ends the export ("Export cancelled", or why decoding stopped).
    pub fn next_frame(&mut self, frame: i32) -> Result<FrameImages, String> {
        if self.is_cancelled() {
            return Err("Export cancelled".into());
        }
        let output_time = f64::from(frame) / f64::from(self.fps);
        let stage_timer = Instant::now();
        let packet = {
            let queue = lock(&self.decode.queue);
            let cancelled = &self.cancelled;
            let mut queue = self
                .decode
                .ready
                .wait_while(queue, |q| q.packets.is_empty() && !q.done && !cancelled.load(Ordering::SeqCst))
                .unwrap_or_else(PoisonError::into_inner);
            queue.packets.pop_front().unwrap_or_default()
        };
        self.decode.space.notify_one();
        self.decode_ns = self.decode_ns.wrapping_add(nanos(stage_timer));
        let clip = usize::try_from(packet.clip).ok().and_then(|index| self.project.clips.get(index)).cloned();
        let insert = clip.as_ref().is_some_and(Clip::is_media);
        let shown = if insert { &packet.insert_image } else { &packet.image };
        let Some(clip) = clip.filter(|_| shown.is_some()) else {
            return Err(if !packet.error.is_empty() {
                packet.error
            } else if self.is_cancelled() {
                "Export cancelled".into()
            } else {
                "Decoder ended unexpectedly".into()
            });
        };
        let sources = Sources {
            timeline: &self.timeline,
            project: &self.project,
            motion: &self.motion,
            keystrokes: &self.keystrokes,
            camera_width: self.camera_media.width,
            camera_duration: self.camera_media.duration,
            camera_offset: self.camera_offset,
            inserts: &self.inserts,
        };
        self.state.export_frame(&sources, &clip, output_time);
        Ok(FrameImages {
            screen: if insert { None } else { packet.image },
            camera: if self.camera_enabled && !insert { packet.camera_image } else { None },
            insert: if insert { packet.insert_image } else { None },
        })
    }

    /// Whether the encoder can take another frame, waiting up to `timeout_ms` for it: two
    /// queued frames, one in the pipe, and two GPU readbacks bound how far rendering can run
    /// ahead. Also true once the encoder failed or the export was cancelled, so that
    /// [`encode`](Self::encode) can say so. Always true for GIF.
    pub fn wait_encode_space(&self, timeout_ms: u64) -> bool {
        if self.gif {
            return true;
        }
        let queue = lock(&self.encode.queue);
        let (abort, cancelled) = (&self.encode.abort, &self.cancelled);
        let (_queue, result) = self
            .encode
            .space
            .wait_timeout_while(queue, Duration::from_millis(timeout_ms), |q| {
                !(q.frames.len() < 2
                    || abort.load(Ordering::SeqCst)
                    || !q.error.is_empty()
                    || cancelled.load(Ordering::SeqCst))
            })
            .unwrap_or_else(PoisonError::into_inner);
        !result.timed_out()
    }

    /// Hands a rendered frame to the encoder (MP4, after [`wait_encode_space`](Self::wait_encode_space))
    /// or appends it to the raw file (GIF). An error is the encoder's, "Export cancelled", or
    /// the raw file's.
    pub fn encode(&mut self, bytes: FrameBytes) -> Result<(), String> {
        if self.gif {
            let Some(raw_file) = self.raw_file.as_mut() else { return Err("Export failed".into()) };
            let write_timer = Instant::now();
            let data = (*bytes).as_ref();
            let result =
                if raw_file.write(data) != data.len() as i64 { Err(raw_file.error_string()) } else { Ok(()) };
            self.encode.write_ns.fetch_add(nanos(write_timer), Ordering::SeqCst);
            return result;
        }
        let mut queue = lock(&self.encode.queue);
        if self.encode.abort.load(Ordering::SeqCst) || !queue.error.is_empty() || self.is_cancelled() {
            return Err(if !queue.error.is_empty() { queue.error.clone() } else { "Export cancelled".into() });
        }
        queue.frames.push_back(bytes);
        drop(queue);
        self.encode.ready.notify_one();
        Ok(())
    }

    /// Stops the decode thread (after the last frame, or on an error) and waits for it.
    pub fn finish_decoder(&mut self) {
        self.decode.stop.store(true, Ordering::SeqCst);
        self.decode.ready.notify_all();
        self.decode.space.notify_all();
        if let Some(thread) = self.decode_thread.take() {
            let _ = thread.join();
        }
    }

    /// Tells the encode thread to finish the file (`abort` false: after the queued frames) or
    /// to stop. Then wait with [`encoder_done`](Self::encoder_done) and
    /// [`wait_encoder_done`](Self::wait_encoder_done), and [`join_encoder`](Self::join_encoder).
    pub fn close_encoder(&mut self, abort: bool) {
        if self.gif {
            return;
        }
        {
            let mut queue = lock(&self.encode.queue);
            self.encode.abort.store(abort, Ordering::SeqCst);
            queue.closing = !abort;
        }
        self.encode.ready.notify_all();
        self.encode.space.notify_all();
    }

    /// Whether the encode thread has finished (always, for GIF or before it started).
    pub fn encoder_done(&self) -> bool {
        self.gif || self.encode_thread.is_none() || self.encode.done.load(Ordering::SeqCst)
    }

    /// Waits up to `timeout_ms` for the encode thread to finish.
    pub fn wait_encoder_done(&self, timeout_ms: u64) {
        let queue = lock(&self.encode.queue);
        let done = &self.encode.done;
        let _ = self
            .encode
            .space
            .wait_timeout_while(queue, Duration::from_millis(timeout_ms), |_| !done.load(Ordering::SeqCst))
            .unwrap_or_else(PoisonError::into_inner);
    }

    pub fn join_encoder(&mut self) {
        if let Some(thread) = self.encode_thread.take() {
            let _ = thread.join();
        }
    }

    /// After the encoder finished: MP4 puts the encoded file in place of the output (replacing
    /// what is there, with the permissions an ordinary new file has); GIF runs the palette pass
    /// over the raw frames, then gifsicle when it is installed.
    pub fn finish(&mut self) -> Result<(), String> {
        let output = self.options.output_path.clone();
        if !self.gif {
            let error = lock(&self.encode.queue).error.clone();
            if !error.is_empty() || self.is_cancelled() {
                return Err(if !error.is_empty() { error } else { "Export cancelled".into() });
            }
            if qdir::file_exists(&output) && !qfile::remove(&output) {
                return Err(format!("Could not replace existing output {output}"));
            }
            if !qfile::rename(&self.encoded_path, &output) {
                return Err(format!("Could not finalize output {output}"));
            }
            // QTemporaryFile creates 0600 files; exports are meant to be shared, so give them
            // the ordinary umask-style permissions a freshly written file would have.
            qfile::set_permissions(&output, 0o644);
        } else {
            let Some(raw_file) = self.raw_file.as_mut() else { return Err("Export failed".into()) };
            raw_file.flush();
            let raw_name = raw_file.file_name();
            let quality = plan::normalized_quality(if self.options.quality.is_empty() {
                &self.project.export_settings.gif.quality
            } else {
                &self.options.quality
            });
            let args = plan::gif_arguments(
                self.width,
                self.height,
                self.fps,
                &raw_name,
                &quality,
                self.software_rendering,
                self.project.export_settings.gif.loop_,
                &output,
            );
            let mut gif_process = Process::new();
            gif_process.start("ffmpeg", &args);
            if !gif_process.wait_for_finished(120000) || gif_process.exit_code() != 0 {
                let details = qstring::from_utf8(&gif_process.read_all_standard_error());
                return Err(qstring::trimmed(&details).to_owned());
            }
            if !qstandardpaths::find_executable("gifsicle").is_empty() {
                let mut optimize = Process::new();
                optimize.start("gifsicle", &["--batch", "--optimize=3", "--lossy=30", output.as_str()]);
                optimize.wait_for_finished(120000);
            }
        }
        Ok(())
    }

    /// The `--timing` report, with the renderer's own stage times.
    pub fn timing_report(&self, upload_ns: i64, render_ns: i64, readback_ns: i64) -> String {
        let divisor = f64::from(cpp::max(1, self.total_frames)) * 1000000.0;
        let elapsed_ms = i64::try_from(self.total_timer.elapsed().as_millis()).unwrap_or(i64::MAX);
        let seconds = elapsed_ms as f64 / 1000.0;
        let per_frame = |ns: i64| number_f64(ns as f64 / divisor, 'f', 3);
        format!(
            "\nTiming ({} frames):\n  decode wait:  {} ms/frame\n  upload:       {} ms/frame\n  render:       {} ms/frame\n  \
             readback:     {} ms/frame\n  encode write: {} ms/frame\n  total:        {} s ({} frames/s)\n",
            self.total_frames,
            per_frame(self.decode_ns),
            per_frame(upload_ns),
            per_frame(render_ns),
            per_frame(readback_ns),
            per_frame(self.encode.write_ns.load(Ordering::SeqCst)),
            number_f64(seconds, 'f', 3),
            number_f64(f64::from(self.total_frames) / cpp::max(0.001, seconds), 'f', 1),
        )
    }
}

impl Drop for ExportJob {
    /// Every way out of `Exporter::run` joined its threads first; a job dropped early stops
    /// the decoder and aborts the encoder before its temporary files go.
    fn drop(&mut self) {
        if self.decode_thread.is_some() {
            self.finish_decoder();
        }
        if self.encode_thread.is_some() {
            self.close_encoder(true);
            self.join_encoder();
        }
    }
}

/// The encode thread: starts the encoder, then writes each queued frame until told to close
/// (then finishes the file) or to stop. ffmpeg is killed if it is still running at the end.
fn encode_thread(args: Vec<String>, shared: &EncodeShared, cancelled_flag: &AtomicBool) {
    let mut process = Process::new();
    let cancelled = || shared.abort.load(Ordering::SeqCst) || cancelled_flag.load(Ordering::SeqCst);
    process.start("ffmpeg", &args);
    let started = process.wait_for_started(10000);
    {
        let mut queue = lock(&shared.queue);
        queue.started = true;
        queue.start_ok = started;
        if !started {
            queue.error = process.error_string();
        }
    }
    shared.ready.notify_all();
    if started {
        encode_frames(&mut process, shared, cancelled_flag, &cancelled);
    }
    if process.state() != crate::compat::qprocess::ProcessState::NotRunning {
        process.kill();
        process.wait_for_finished(2000);
    }
    shared.done.store(true, Ordering::SeqCst);
    shared.space.notify_all();
}

fn encode_frames(process: &mut Process, shared: &EncodeShared, cancelled_flag: &AtomicBool, cancelled: &dyn Fn() -> bool) {
    loop {
        let bytes = {
            let queue = lock(&shared.queue);
            let mut queue = shared
                .ready
                .wait_while(queue, |q| {
                    !(!q.frames.is_empty()
                        || q.closing
                        || shared.abort.load(Ordering::SeqCst)
                        || cancelled_flag.load(Ordering::SeqCst))
                })
                .unwrap_or_else(PoisonError::into_inner);
            if cancelled() {
                queue.error = "Export cancelled".into();
                return;
            }
            if queue.frames.is_empty() && queue.closing {
                break;
            }
            let Some(bytes) = queue.frames.pop_front() else { continue };
            bytes
        };
        shared.space.notify_one();
        let write_timer = Instant::now();
        if let Err(pipe_error) = write_encoder_frame(process, (*bytes).as_ref(), cancelled, STALL_TIMEOUT_MS) {
            let mut queue = lock(&shared.queue);
            queue.error = pipe_error;
            shared.abort.store(true, Ordering::SeqCst);
            shared.space.notify_all();
            return;
        }
        shared.write_ns.fetch_add(nanos(write_timer), Ordering::SeqCst);
    }
    if let Err(pipe_error) = finish_encoder_pipe(process, cancelled, FINISH_TIMEOUT_MS) {
        lock(&shared.queue).error = pipe_error;
    }
}

/// Queues `error` for the frame loop, which stops at it.
fn push_error(shared: &DecodeShared, error: String) {
    lock(&shared.queue).packets.push_back(DecodedPacket::failed(error));
}

fn image(pixels: Vec<u8>, width: i32, height: i32) -> Image {
    Image { width, height, pixels: Arc::new(pixels) }
}

/// The decode thread: one decoder for the clip playing (restarted at each clip) and one for
/// the webcam video (rewound when the camera time goes back), three frames ahead of the frame
/// loop.
fn decode_thread(work: &DecodePlan, shared: &DecodeShared, cancelled: &AtomicBool) {
    let mut decoder = FfmpegDecoder::new();
    let mut camera_decoder = FfmpegDecoder::new();
    let mut active_clip = -1;
    // The last frame of the recording and of the current inserted clip, kept apart so a clip
    // that runs out of frames repeats its own last frame, never the other's.
    let mut decoded: Option<Image> = None;
    let mut insert_decoded: Option<Image> = None;
    let mut camera_decoded: Option<Image> = None;
    let mut camera_frame = -1;
    let mut camera_eof = false;
    let camera = &work.camera_media;
    let camera_scaled =
        work.camera_decode_width != camera.width || work.camera_decode_height != camera.height;
    if work.camera_enabled {
        let mut camera_error = String::new();
        if !camera_decoder.start(
            &work.camera_path,
            0.0,
            camera.duration,
            camera.fps,
            work.camera_decode_width,
            work.camera_decode_height,
            camera_scaled,
            &mut camera_error,
        ) {
            let mut queue = lock(&shared.queue);
            queue.packets.push_back(DecodedPacket::failed(camera_error));
            queue.done = true;
            drop(queue);
            shared.ready.notify_all();
            return;
        }
    }
    let stopped = || shared.stop.load(Ordering::SeqCst) || cancelled.load(Ordering::SeqCst);
    let mut frame = 0;
    while frame < work.total_frames && !stopped() {
        let output_time = f64::from(frame) / f64::from(work.fps);
        let clip_index = work.timeline.clip_at(output_time, None);
        let Some(clip) = usize::try_from(clip_index).ok().and_then(|index| work.clips.get(index)) else { break };
        let mut decode_error = String::new();
        if clip_index != active_clip {
            let started = if clip.is_media() {
                let info = work.inserts.get(&clip.media).copied().unwrap_or_default();
                let (width, height) = plan::insert_decode_size(work.width, work.height, &info);
                insert_decoded = None;
                // Always scaled: the filter fixes the frame size whatever rotation or sample
                // aspect the file carries.
                decoder.start(
                    &qdir::file_path(&work.bundle, &clip.media),
                    clip.in_,
                    clip.out - clip.in_,
                    f64::from(work.fps) / clip.speed,
                    width,
                    height,
                    true,
                    &mut decode_error,
                )
            } else {
                decoder.start(
                    &work.video_path,
                    clip.in_,
                    clip.out - clip.in_,
                    f64::from(work.fps) / clip.speed,
                    work.decode_width,
                    work.decode_height,
                    work.decode_width != work.media.width || work.decode_height != work.media.height,
                    &mut decode_error,
                )
            };
            if !started {
                push_error(shared, decode_error);
                break;
            }
            active_clip = clip_index;
        }
        let shown = if clip.is_media() { &mut insert_decoded } else { &mut decoded };
        if let Some(next) = decoder.read_frame(&mut decode_error) {
            *shown = Some(image(next, decoder.width(), decoder.height()));
        } else if shown.is_none() {
            push_error(
                shared,
                if decode_error.is_empty() { "Decoder ended before the first frame".into() } else { decode_error },
            );
            break;
        }
        if work.camera_enabled && !clip.is_media() {
            let screen_source_time = work.timeline.source_time(output_time);
            let mapped = map_camera_time(screen_source_time, work.camera_offset, camera.duration);
            let frames = if cfg!(target_arch = "aarch64") {
                // GCC fuses the multiply and add into one fmadd on aarch64.
                mapped.seconds.mul_add(camera.fps, 1e-6)
            } else {
                mapped.seconds * camera.fps + 1e-6
            };
            let wanted = cpp::max(0, cpp::int_cast(frames.floor()));
            if wanted < camera_frame {
                camera_decoder.cancel();
                camera_frame = -1;
                camera_eof = false;
                if !camera_decoder.start(
                    &work.camera_path,
                    0.0,
                    camera.duration,
                    camera.fps,
                    work.camera_decode_width,
                    work.camera_decode_height,
                    camera_scaled,
                    &mut decode_error,
                ) {
                    push_error(shared, decode_error);
                    break;
                }
            }
            while !camera_eof && camera_frame < wanted {
                if let Some(next) = camera_decoder.read_frame(&mut decode_error) {
                    camera_decoded = Some(image(next, camera_decoder.width(), camera_decoder.height()));
                    camera_frame += 1;
                } else {
                    camera_eof = true;
                    if camera_decoded.is_none() {
                        push_error(
                            shared,
                            if decode_error.is_empty() {
                                "Camera decoder ended before the first frame".into()
                            } else {
                                decode_error.clone()
                            },
                        );
                    }
                }
            }
            if camera_decoded.is_none() {
                break;
            }
        }
        let queue = lock(&shared.queue);
        let mut queue = shared
            .space
            .wait_while(queue, |q| !(q.packets.len() < 3 || stopped()))
            .unwrap_or_else(PoisonError::into_inner);
        if stopped() {
            break;
        }
        queue.packets.push_back(DecodedPacket {
            image: decoded.clone(),
            camera_image: camera_decoded.clone(),
            error: String::new(),
            clip: clip_index,
            insert_image: insert_decoded.clone(),
        });
        drop(queue);
        shared.ready.notify_one();
        frame += 1;
    }
    decoder.cancel();
    camera_decoder.cancel();
    lock(&shared.queue).done = true;
    shared.ready.notify_all();
}
