//! What `Exporter.cpp` computes before it renders: the recording's ffprobe summary, the output
//! size, the quality and bitrate tables, the audio `filter_complex`, the video encoder (which,
//! its arguments and its probe) and the GIF palette pass's arguments.

use crate::compat::geometry::RectF;
use crate::compat::qjson;
use crate::compat::qnumber::{self, number_f64};
use crate::compat::qprocess::Process;
use crate::compat::qround::{q_round, q_round64};
use crate::compat::{cpp, qdir, qstring};
use crate::input_log::InputLog;
use crate::media_probe::VideoInfo;
use crate::project::{Clip, Music};

/// The even size an encoder needs: odd dimensions grow by one.
pub fn padded_even_size(width: i32, height: i32) -> (i32, i32) {
    (width.wrapping_add((width % 2).abs()), height.wrapping_add((height % 2).abs()))
}

/// What the exporter's own ffprobe call reports for the recording and the webcam video.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaInfo {
    pub width: i32,
    pub height: i32,
    pub duration: f64,
    pub fps: f64,
    pub audio: bool,
    /// The video says which matrix converts it to RGB.
    pub colour_tagged: bool,
    /// The matrix for converting the video's YUV to RGB on the GPU, when it is one that path
    /// knows: BT.709, or BT.601 (also for an untagged video, as ffmpeg assumes and the decoder
    /// says with `untagged_bt601`). `None` (BT.2020, ...) leaves the conversion to ffmpeg.
    pub yuv_matrix: Option<YuvMatrix>,
    /// ffprobe's `color_range` is "pc" (full range); otherwise limited ("tv" or untagged).
    pub full_range: bool,
}

/// The YUV to RGB matrix of a YUV frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvMatrix {
    Bt601,
    Bt709,
}

impl Default for MediaInfo {
    fn default() -> MediaInfo {
        MediaInfo {
            width: 0,
            height: 0,
            duration: 0.0,
            fps: 30.0,
            audio: false,
            colour_tagged: false,
            yuv_matrix: Some(YuvMatrix::Bt601),
            full_range: false,
        }
    }
}

/// `QString::toDouble()`, 0 when the text is not a number.
fn to_double(text: &str) -> f64 {
    qnumber::to_double(text).unwrap_or(0.0)
}

/// A frame rate such as "30000/1001", or a plain number.
fn rate_value(value: &str) -> f64 {
    let parts: Vec<&str> = value.split('/').collect();
    if parts.len() == 2 && to_double(parts[1]) != 0.0 {
        to_double(parts[0]) / to_double(parts[1])
    } else {
        to_double(value)
    }
}

/// ffprobe's first video stream and duration of `path`, and the error the C++ set: ffprobe's
/// own message when it failed (then nothing else is read), or a complaint about metadata that
/// is not a video, which the caller only looks at when there is no width.
pub fn probe(path: &str) -> (MediaInfo, String) {
    let mut process = Process::new();
    process.start(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,r_frame_rate,color_space,color_range:format=duration",
            "-of",
            "json",
            path,
        ],
    );
    if !process.wait_for_finished(15000) || process.exit_code() != 0 {
        let details = qstring::from_utf8(&process.read_all_standard_error());
        return (MediaInfo::default(), qstring::trimmed(&details).to_owned());
    }
    let root = qjson::object_from_json(&process.read_all_standard_output()).unwrap_or_default();
    let mut result = MediaInfo {
        duration: to_double(&root.value("format").to_object().value("duration").to_string_or("")),
        ..MediaInfo::default()
    };
    let mut error = String::new();
    for value in root.value("streams").to_array() {
        let stream = value.to_object();
        let kind = stream.value("codec_type").as_str();
        if kind == Some("video") && result.width == 0 {
            result.width = stream.value("width").to_int(0);
            result.height = stream.value("height").to_int(0);
            result.fps = rate_value(&stream.value("r_frame_rate").to_string_or(""));
            let colour_space = stream.value("color_space").to_string_or("");
            result.colour_tagged = !colour_space.is_empty() && colour_space != "unknown";
            result.yuv_matrix = match colour_space.as_str() {
                "" | "unknown" | "smpte170m" | "bt470bg" => Some(YuvMatrix::Bt601),
                "bt709" => Some(YuvMatrix::Bt709),
                _ => None,
            };
            result.full_range = stream.value("color_range").to_string_or("") == "pc";
        } else if kind == Some("audio") {
            result.audio = true;
        }
    }
    if result.width <= 0 || result.height <= 0 || result.duration <= 0.0 {
        error = "ffprobe returned invalid video metadata".into();
    }
    if result.fps <= 0.0 {
        result.fps = 30.0;
    }
    (result, error)
}

/// Width over height of the output: the project's aspect, or the cropped recording's.
pub fn output_aspect(aspect: &str, source_width: i32, source_height: i32, crop: &RectF) -> f64 {
    match aspect {
        "16:9" => 16.0 / 9.0,
        "1:1" => 1.0,
        "4:3" => 4.0 / 3.0,
        "9:16" => 9.0 / 16.0,
        "3:4" => 3.0 / 4.0,
        "4:5" => 4.0 / 5.0,
        _ => (f64::from(source_width) * crop.width) / cpp::max(1.0, f64::from(source_height) * crop.height),
    }
}

/// An `atempo` chain for any speed: each filter takes 0.5 to 2.
pub fn atempo(mut speed: f64) -> String {
    let mut filters: Vec<String> = Vec::new();
    while speed > 2.0 {
        filters.push("atempo=2".into());
        speed /= 2.0;
    }
    while speed < 0.5 {
        filters.push("atempo=0.5".into());
        speed /= 0.5;
    }
    filters.push(format!("atempo={}", number_f64(speed, 'g', 8)));
    filters.join(",")
}

/// When each click sounds in the output: every button press inside a recording clip, mapped
/// through its speed, at most 200.
pub fn click_output_times(input: &InputLog, clips: &[Clip]) -> Vec<f64> {
    let mut result = Vec::new();
    let mut output_start = 0.0;
    let clicks = input.click_downs(false);
    for clip in clips {
        for click in &clicks {
            if clip.is_media() || click.time < clip.in_ || click.time >= clip.out {
                continue;
            }
            result.push(output_start + (click.time - clip.in_) / clip.speed);
            if result.len() == 200 {
                return result;
            }
        }
        output_start += (clip.out - clip.in_) / clip.speed;
    }
    result
}

/// A clip's audio, brought to one format and to exactly the clip's output length, so a source
/// with other rates or channels, or audio that ends early, cannot shift the clips after it.
pub fn audio_segment(source: &str, speed: f64, length: f64) -> String {
    let length = number_f64(length, 'f', 6);
    format!(
        "{source},aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,{},apad=whole_dur={length},atrim=end={length}",
        atempo(speed)
    )
}

/// The recording's audio from `in_` to `out` source seconds, from input `input`, timed from
/// the clip's start. The recording's sound can start after the screen (it is muxed in with
/// `-itsoffset`); `aresample` pads it with silence from the file's time 0, so a clip from the
/// start keeps it where the preview plays it instead of pulling it early.
pub fn source_audio(input: i32, in_: f64, out: f64) -> String {
    format!(
        "[{input}:a:0]aresample=async=1:first_pts=0,atrim=start={}:end={},asetpts=PTS-STARTPTS",
        number_f64(in_, 'f', 6),
        number_f64(out, 'f', 6)
    )
}

/// Fades for a segment of `length` seconds that meets a cut with a transition: in at its start
/// and out at its end, as `(fade_in, fade_out)` asks (`transitions::audio_fades`); nothing for
/// neither, so cuts without a transition keep their filter as it was.
pub fn declick((fade_in, fade_out): (bool, bool), length: f64) -> String {
    let duration = crate::transitions::DECLICK.min(length / 2.0);
    let mut filter = String::new();
    if fade_in {
        filter += &format!(",afade=t=in:st=0:d={}", number_f64(duration, 'f', 6));
    }
    if fade_out {
        filter += &format!(
            ",afade=t=out:st={}:d={}",
            number_f64((length - duration).max(0.0), 'f', 6),
            number_f64(duration, 'f', 6)
        );
    }
    filter
}

/// The recording's whole sound as one stream on its own timeline (0 is the container's start,
/// as ffmpeg's input timestamps count it), at 48 kHz stereo, through `cleanup` (filters such as
/// the denoiser, which then hears the recording continuously instead of clip by clip).
pub fn recording_audio_chain(source: &str, cleanup: &str) -> String {
    format!("{source}aresample=48000:async=1:first_pts=0,aformat=sample_fmts=fltp:channel_layouts=stereo,{cleanup}")
}

/// The export's audio sources: the inputs to open after the video's, from `first_input` on,
/// and one segment per clip in output order (none when no clip has sound).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioInputs {
    pub arguments: Vec<String>,
    /// Chains the segments read from, each ending in `;`, to come first in the graph: the
    /// recording's denoised sound, split once per recording clip. Empty without denoising.
    pub prelude: String,
    pub segments: Vec<String>,
    /// The next input number free after these.
    pub next_input: i32,
}

/// One audio segment per clip: the recording's audio, an inserted video's own audio, or
/// silence. Each inserted clip opens its file at its in point, so containers that do not start
/// at zero need no timestamp arithmetic. `insert_has_audio` says whether an inserted clip's
/// file (its bundle-relative path) has sound. `fades` (one per clip, or none) fades a
/// recording clip's sound in and out at cuts with a transition ([`declick`]).
///
/// With `denoise` (a [`crate::audio_cleanup::denoise_filter`]) the recording's sound goes
/// through it once, whole, in the [`AudioInputs::prelude`], and each recording clip cuts its
/// part from that: a denoiser per clip would start each clip without the sound before it and
/// lose the end it still holds back at the cut.
pub fn audio_inputs(
    clips: &[Clip],
    insert_has_audio: &dyn Fn(&str) -> bool,
    use_source_audio: bool,
    bundle: &str,
    video_path: &str,
    first_input: i32,
    denoise: Option<&str>,
    fades: &[(bool, bool)],
) -> AudioInputs {
    let mut result = AudioInputs { next_input: first_input, ..AudioInputs::default() };
    let mut source_input = -1;
    let mut any_audio = false;
    let mut recording_clips = 0;
    for (i, clip) in clips.iter().enumerate() {
        let length = (clip.out - clip.in_) / clip.speed;
        if clip.is_media() && insert_has_audio(&clip.media) {
            let input = result.next_input;
            result.next_input += 1;
            result.arguments.extend([
                "-ss".into(),
                number_f64(clip.in_, 'f', 6),
                "-t".into(),
                number_f64(clip.out - clip.in_, 'f', 6),
                "-i".into(),
                qdir::file_path(bundle, &clip.media),
            ]);
            result.segments.push(audio_segment(&format!("[{input}:a:0]asetpts=PTS-STARTPTS"), clip.speed, length));
            any_audio = true;
        } else if !clip.is_media() && use_source_audio {
            if source_input < 0 {
                source_input = result.next_input;
                result.next_input += 1;
                result.arguments.extend(["-i".into(), video_path.to_owned()]);
            }
            // Denoised, the prelude's chain has already placed the sound from the file's time 0.
            let source = match denoise {
                Some(_) => format!(
                    "[rec{recording_clips}]atrim=start={}:end={},asetpts=PTS-STARTPTS",
                    number_f64(clip.in_, 'f', 6),
                    number_f64(clip.out, 'f', 6)
                ),
                None => source_audio(source_input, clip.in_, clip.out),
            };
            recording_clips += 1;
            let fade = fades.get(i).copied().unwrap_or((false, false));
            result.segments.push(audio_segment(&source, clip.speed, length) + &declick(fade, length));
            any_audio = true;
        } else {
            result.segments.push(silent_segment(length));
        }
    }
    if !any_audio {
        result.segments.clear();
    }
    if let (Some(denoise), true) = (denoise, recording_clips > 0) {
        let outputs: String = (0..recording_clips).map(|i| format!("[rec{i}]")).collect();
        result.prelude = format!(
            "{},asplit=outputs={recording_clips}{outputs};",
            recording_audio_chain(&format!("[{source_input}:a:0]"), denoise)
        );
    }
    result
}

pub fn silent_segment(length: f64) -> String {
    format!(
        "anullsrc=r=48000:cl=stereo,atrim=duration={},aformat=sample_fmts=fltp:channel_layouts=stereo",
        number_f64(length, 'f', 6)
    )
}

/// Background music in the export's mix: ffmpeg input `input` (opened with
/// [`music_input_arguments`]), at `volume`, faded in and out over the output, and ducked under
/// the clips' sound when `duck`.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicMix {
    pub input: i32,
    pub volume: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub duck: bool,
}

impl MusicMix {
    pub fn new(input: i32, music: &Music, output_duration: f64) -> MusicMix {
        let (fade_in, fade_out) = crate::music::fade_lengths(music, output_duration);
        MusicMix { input, volume: music.volume, fade_in, fade_out, duck: music.duck }
    }
}

/// The input options that open the music: from `start` into the file, then the whole file over
/// and over when it loops (ffmpeg's later passes start at the beginning).
pub fn music_input_arguments(music: &Music, path: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if music.loop_ {
        args.extend(["-stream_loop".into(), "-1".into()]);
    }
    if music.start > 0.0 {
        args.extend(["-ss".into(), number_f64(music.start, 'f', 6)]);
    }
    args.extend(["-i".into(), path.into()]);
    args
}

/// The music's chain, labelled `[music]`: brought to the clips' format and to exactly the
/// output's length, its volume and fades, and, keyed on `duck_key` (the clips' mixed sound),
/// ffmpeg's `sidechaincompress` with the preview's settings ([`crate::music`]).
pub fn music_filter(music: &MusicMix, output_duration: f64, duck_key: Option<&str>) -> String {
    let duration = number_f64(output_duration, 'f', 6);
    let mut filter = format!(
        "[{}:a:0]asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,\
         apad=whole_dur={duration},atrim=end={duration},volume={}",
        music.input,
        number_f64(music.volume, 'g', 8)
    );
    if music.fade_in > 0.0 {
        filter += &format!(",afade=t=in:st=0:d={}", number_f64(music.fade_in, 'f', 6));
    }
    if music.fade_out > 0.0 {
        filter += &format!(
            ",afade=t=out:st={}:d={}",
            number_f64(cpp::max(0.0, output_duration - music.fade_out), 'f', 6),
            number_f64(music.fade_out, 'f', 6)
        );
    }
    match duck_key {
        Some(key) => {
            filter += &format!(
                "[musicsrc];[musicsrc]{key}sidechaincompress=threshold={}:ratio={}:attack={}:release={}[music];",
                number_f64(crate::music::DUCK_THRESHOLD, 'g', 8),
                number_f64(crate::music::DUCK_RATIO, 'g', 8),
                number_f64(crate::music::DUCK_ATTACK_MS, 'g', 8),
                number_f64(crate::music::DUCK_RELEASE_MS, 'g', 8)
            );
        }
        None => filter += "[music];",
    }
    filter
}

/// The `-filter_complex` that mixes the clips' audio (`segments`, one chain per clip in output
/// order, or none when no clip has audio) with a click sound from input `click_input` at each
/// of `clicks`, padded or cut to the output's length.
pub fn audio_filter(segments: &[String], volume: f64, clicks: &[f64], click_input: i32, output_duration: f64) -> String {
    audio_filter_with_music(segments, volume, clicks, click_input, output_duration, None)
}

/// [`audio_filter`] with background music added to the mix, the clips' sound and clicks
/// passing through at the same level. Without music it is [`audio_filter`]'s graph.
pub fn audio_filter_with_music(
    segments: &[String],
    volume: f64,
    clicks: &[f64],
    click_input: i32,
    output_duration: f64,
    music: Option<&MusicMix>,
) -> String {
    mastered_audio_filter("", segments, None, volume, clicks, click_input, output_duration, music)
}

/// The clips' sound, joined: the program loudness normalisation measures and normalises
/// (without the click sound, which keeps its own level, and before the volume).
fn program_audio(prelude: &str, segments: &[String]) -> String {
    let mut filter = prelude.to_owned();
    let mut clip_labels = String::new();
    for (i, segment) in segments.iter().enumerate() {
        filter += &format!("{segment}[a{i}];");
        clip_labels += &format!("[a{i}]");
    }
    filter + &format!("{clip_labels}concat=n={}:v=0:a=1", segments.len())
}

/// The loudness measuring pass's graph: exactly the sound [`mastered_audio_filter`] normalises,
/// through `measure`, into `[aout]`.
pub fn program_measure_filter(prelude: &str, segments: &[String], measure: &str) -> String {
    format!("{},{measure}[aout]", program_audio(prelude, segments))
}

/// [`audio_filter`] after `prelude` ([`AudioInputs::prelude`]), with the clips' joined sound
/// through `master` (the loudness normalisation) before the volume, so the volume trims the
/// normalised level; the music (keyed on the program for ducking, never measured) and the click
/// sound are mixed in after, each at its own level.
pub fn mastered_audio_filter(
    prelude: &str,
    segments: &[String],
    master: Option<&str>,
    volume: f64,
    clicks: &[f64],
    click_input: i32,
    output_duration: f64,
    music: Option<&MusicMix>,
) -> String {
    let mut filter = String::new();
    let mut mix_labels: Vec<String> = Vec::new();
    let duck = music.is_some_and(|music| music.duck) && !segments.is_empty();
    if !segments.is_empty() {
        let master = master.map(|master| format!("{master},")).unwrap_or_default();
        filter += &format!(
            "{},{master}volume={}{};",
            program_audio(prelude, segments),
            number_f64(volume, 'g', 8),
            if duck { ",asplit=2[source][duckkey]" } else { "[source]" }
        );
        mix_labels.push("[source]".into());
    }
    if let Some(music) = music {
        filter += &music_filter(music, output_duration, duck.then_some("[duckkey]"));
        mix_labels.push("[music]".into());
    }
    if !clicks.is_empty() {
        let mut click_sources: Vec<String> = Vec::new();
        if clicks.len() > 1 {
            filter += &format!("[{click_input}:a]asplit={}", clicks.len());
            for i in 0..clicks.len() {
                let label = format!("clicksrc{i}");
                filter += &format!("[{label}]");
                click_sources.push(format!("[{label}]"));
            }
            filter.push(';');
        } else {
            click_sources.push(format!("[{click_input}:a]"));
        }
        for (i, click) in clicks.iter().enumerate() {
            let label = format!("click{i}");
            let delay_ms = cpp::max(0, q_round64(click * 1000.0));
            filter += &format!(
                "{}atrim=duration=0.03,asetpts=PTS-STARTPTS,volume=0.25,adelay={delay_ms}:all=1[{label}];",
                click_sources[i]
            );
            mix_labels.push(format!("[{label}]"));
        }
    }
    let duration = number_f64(output_duration, 'f', 6);
    filter += &format!(
        "{}amix=inputs={}:normalize=0:duration=longest,apad=pad_dur={duration},atrim=duration={duration}[aout]",
        mix_labels.concat(),
        mix_labels.len()
    );
    filter
}

/// A video encoder export can use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoEncoder {
    /// NVIDIA's `h264_nvenc`.
    Nvenc,
    /// `h264_vaapi` on a render node such as `/dev/dri/renderD128`.
    Vaapi(String),
    /// `libx264`, which needs no GPU.
    X264,
}

/// RGB to Y'CbCr with BT.709's matrix in limited range. swscale's default is BT.601, which
/// players do not assume for HD video: reds came out visibly off.
const BT709_CONVERSION: &str = "scale=out_color_matrix=bt709:out_range=tv";
/// The frames say so, and so does the file. ffmpeg takes the encoder's tags from the frames,
/// and only the matrix and range would get there by themselves.
const BT709_FRAMES: &str = "setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv";
const BT709_TAGS: [&str; 8] =
    ["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"];

impl VideoEncoder {
    /// ffmpeg's name for it.
    pub fn name(&self) -> &'static str {
        match self {
            VideoEncoder::Nvenc => "h264_nvenc",
            VideoEncoder::Vaapi(_) => "h264_vaapi",
            VideoEncoder::X264 => "libx264",
        }
    }

    /// Options before the inputs: the device VA-API encodes on.
    pub fn device_arguments(&self) -> Vec<String> {
        match self {
            VideoEncoder::Vaapi(device) => vec!["-vaapi_device".into(), device.clone()],
            _ => Vec::new(),
        }
    }

    /// The video options for RGBA frames, flipped first when `flip` (OpenGL's come bottom up),
    /// at `bitrate` bits per second: converted to 4:2:0 with BT.709 on the CPU (then uploaded,
    /// for VA-API), encoded at the High profile, and tagged BT.709.
    pub fn video_arguments(&self, flip: bool, bitrate: i64) -> Vec<String> {
        let vaapi = matches!(self, VideoEncoder::Vaapi(_));
        let mut filter = format!(
            "{}{BT709_CONVERSION},format={},{BT709_FRAMES}",
            if flip { "vflip," } else { "" },
            if vaapi { "nv12" } else { "yuv420p" }
        );
        if vaapi {
            filter += ",hwupload";
        }
        let mut args: Vec<String> = vec!["-vf".into(), filter, "-c:v".into(), self.name().into()];
        args.extend(
            match self {
                VideoEncoder::Nvenc => ["-preset", "p5"],
                VideoEncoder::Vaapi(_) => ["-rc_mode", "VBR"],
                VideoEncoder::X264 => ["-preset", "medium"],
            }
            .map(String::from),
        );
        args.extend([
            "-b:v".into(),
            qnumber::number_i64(bitrate),
            "-maxrate".into(),
            qnumber::number_i64(cpp::int64_cast(bitrate as f64 * 1.2)),
            "-bufsize".into(),
            qnumber::number_i64(bitrate.wrapping_mul(2)),
            "-profile:v".into(),
            "high".into(),
        ]);
        args.extend(BT709_TAGS.map(String::from));
        args
    }
}

/// The encoders to try, best first: NVENC unless `nvenc_disabled` (`OMAREEL_DISABLE_NVENC`),
/// VA-API on `vaapi_device` when there is one, and libx264. `choice` (`OMAREEL_EXPORT_ENCODER`)
/// "nvenc", "vaapi" or "x264" keeps only that one and libx264.
pub fn video_encoders(choice: &str, nvenc_disabled: bool, vaapi_device: Option<String>) -> Vec<VideoEncoder> {
    let mut encoders = Vec::new();
    if !nvenc_disabled && matches!(choice, "nvenc" | "") {
        encoders.push(VideoEncoder::Nvenc);
    }
    if let Some(device) = vaapi_device.filter(|_| matches!(choice, "vaapi" | "")) {
        encoders.push(VideoEncoder::Vaapi(device));
    }
    encoders.push(VideoEncoder::X264);
    encoders
}

/// Whether ffmpeg can encode one `width` x `height` RGBA frame with `encoder` the way export
/// will, at `bitrate`.
pub fn encoder_works(encoder: &VideoEncoder, width: i32, height: i32, bitrate: i64) -> bool {
    let mut args: Vec<String> = vec!["-v".into(), "error".into()];
    args.extend(encoder.device_arguments());
    args.extend([
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        format!("color=size={width}x{height}:rate=1,format=rgba"),
        "-frames:v".into(),
        "1".into(),
    ]);
    args.extend(encoder.video_arguments(false, bitrate));
    args.extend(["-f".into(), "null".into(), "-".into()]);
    let mut process = Process::new();
    process.start("ffmpeg", &args);
    process.wait_for_finished(15000) && process.exit_code() == 0
}

/// The old quality names as the current ones.
pub fn normalized_quality(quality: &str) -> String {
    match quality {
        "low" => "web-low",
        "medium" => "web-high",
        "high" => "social",
        "best" => "studio",
        other => other,
    }
    .to_owned()
}

/// Bits per pixel and frame for each quality ("social" and anything unknown: 0.05).
pub fn bitrate_multiplier(quality: &str) -> f64 {
    match quality {
        "web-low" => 0.0075,
        "web-high" => 0.0175,
        "studio" => 0.3,
        _ => 0.05,
    }
}

/// An inserted clip decodes at the size that covers the output, never larger than itself.
pub fn insert_decode_size(width: i32, height: i32, info: &VideoInfo) -> (i32, i32) {
    let scale = cpp::clamp(
        cpp::max(f64::from(width) / f64::from(info.width), f64::from(height) / f64::from(info.height)),
        0.01,
        1.0,
    );
    (
        cpp::max(2, q_round(f64::from(info.width) * scale / 2.0).wrapping_mul(2)),
        cpp::max(2, q_round(f64::from(info.height) * scale / 2.0).wrapping_mul(2)),
    )
}

/// The GIF pass: ffmpeg reads the raw frames back, flips them when they came from OpenGL, and
/// for "studio" builds a palette from them first.
#[allow(clippy::too_many_arguments)]
pub fn gif_arguments(
    width: i32,
    height: i32,
    fps: i32,
    raw_path: &str,
    quality: &str,
    software_rendering: bool,
    loop_: bool,
    output_path: &str,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "-y",
        "-v",
        "error",
        "-f",
        "rawvideo",
        "-pixel_format",
        "rgba",
        "-video_size",
        &format!("{width}x{height}"),
        "-framerate",
        &fps.to_string(),
        "-i",
        raw_path,
    ]
    .map(String::from)
    .into();
    let flip = if software_rendering { "" } else { "vflip," };
    if quality == "studio" {
        args.push("-vf".into());
        args.push(format!(
            "{flip}split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle"
        ));
    } else if !flip.is_empty() {
        args.push("-vf".into());
        args.push("vflip".into());
    }
    args.push("-loop".into());
    args.push(if loop_ { "0" } else { "-1" }.into());
    args.push(output_path.into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    // tests/test_exporter_smoke.cpp: padsOddDimensionsToEven
    #[test]
    fn pads_odd_dimensions_to_even() {
        assert_eq!(padded_even_size(1367, 781), (1368, 782));
        assert_eq!(padded_even_size(1920, 1080), (1920, 1080));
        assert_eq!(padded_even_size(-3, 0), (-2, 0));
    }

    #[test]
    fn atempo_chains_stay_in_range() {
        assert_eq!(atempo(1.0), "atempo=1");
        assert_eq!(atempo(0.75), "atempo=0.75");
        assert_eq!(atempo(8.0), "atempo=2,atempo=2,atempo=2");
        assert_eq!(atempo(24.0), "atempo=2,atempo=2,atempo=2,atempo=2,atempo=1.5");
        assert_eq!(atempo(0.2), "atempo=0.5,atempo=0.5,atempo=0.8");
        assert_eq!(atempo(1.4), "atempo=1.4");
    }

    /// A clip from the start of a recording whose sound starts late keeps the sound where the
    /// preview plays it.
    #[test]
    fn late_recording_audio_keeps_its_place_in_the_export() {
        use crate::transcript::audio::tests::{late_audio_recording, onset};
        let dir = std::env::temp_dir().join(format!("omareel-export-audio-offset-{}", std::process::id()));
        let Some(screen) = late_audio_recording(&dir, 0.7) else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let filter = format!("{}[a]", audio_segment(&source_audio(0, 0.0, 3.0), 1.0, 3.0));
        let output = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-i", &screen, "-filter_complex", &filter, "-map", "[a]"])
            .args(["-ac", "1", "-f", "s16le", "-acodec", "pcm_s16le", "pipe:1"])
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let samples: Vec<i16> = output.stdout.chunks_exact(2).map(|p| i16::from_le_bytes([p[0], p[1]])).collect();
        let onset = onset(&samples, 48_000);
        assert!((onset - 0.7).abs() < 0.03, "the tone starts at {onset}");
        assert!((samples.len() as f64 / 48_000.0 - 3.0).abs() < 0.01);
        assert_eq!(
            source_audio(2, 1.5, 4.0),
            "[2:a:0]aresample=async=1:first_pts=0,atrim=start=1.500000:end=4.000000,asetpts=PTS-STARTPTS"
        );
    }

    #[test]
    fn audio_filters_read_as_the_cpp_wrote_them() {
        assert_eq!(
            audio_segment("[1:a:0]asetpts=PTS-STARTPTS", 0.5, 2.4),
            "[1:a:0]asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,\
             atempo=0.5,apad=whole_dur=2.400000,atrim=end=2.400000"
        );
        assert_eq!(
            silent_segment(1.2),
            "anullsrc=r=48000:cl=stereo,atrim=duration=1.200000,aformat=sample_fmts=fltp:channel_layouts=stereo"
        );
        let segments = ["S0".to_string(), "S1".to_string()];
        assert_eq!(
            audio_filter(&segments, 1.0, &[0.5, 1.2345], 3, 4.25),
            "S0[a0];S1[a1];[a0][a1]concat=n=2:v=0:a=1,volume=1[source];[3:a]asplit=2[clicksrc0][clicksrc1];\
             [clicksrc0]atrim=duration=0.03,asetpts=PTS-STARTPTS,volume=0.25,adelay=500:all=1[click0];\
             [clicksrc1]atrim=duration=0.03,asetpts=PTS-STARTPTS,volume=0.25,adelay=1235:all=1[click1];\
             [source][click0][click1]amix=inputs=3:normalize=0:duration=longest,apad=pad_dur=4.250000,\
             atrim=duration=4.250000[aout]"
        );
        assert_eq!(
            audio_filter(&[], 0.8, &[2.0], 1, 3.0),
            "[1:a]atrim=duration=0.03,asetpts=PTS-STARTPTS,volume=0.25,adelay=2000:all=1[click0];\
             [click0]amix=inputs=1:normalize=0:duration=longest,apad=pad_dur=3.000000,atrim=duration=3.000000[aout]"
        );
    }

    /// The recording, an inserted video with sound, one without, and the recording again.
    fn mixed_clips() -> Vec<Clip> {
        vec![
            Clip { id: "c1".into(), in_: 0.5, out: 2.0, speed: 1.0, ..Clip::default() },
            Clip { id: "m1".into(), in_: 1.0, out: 3.0, speed: 2.0, media: "media/talk.mp4".into(), ..Clip::default() },
            Clip { id: "m2".into(), in_: 0.0, out: 1.0, speed: 1.0, media: "media/mute.mp4".into(), ..Clip::default() },
            Clip { id: "c2".into(), in_: 4.0, out: 5.0, speed: 0.5, ..Clip::default() },
        ]
    }

    // The arguments start() built before audio cleanup, spelled out: with cleanup off they must
    // stay exactly these (the recording's sound placed from the file's time 0, as
    // [`source_audio`] does).
    #[test]
    fn audio_inputs_without_cleanup_are_as_before() {
        let has_audio = |media: &str| media == "media/talk.mp4";
        let inputs = audio_inputs(&mixed_clips(), &has_audio, true, "/b", "/b/screen.mp4", 1, None, &[]);
        assert_eq!(inputs.arguments, ["-i", "/b/screen.mp4", "-ss", "1.000000", "-t", "2.000000", "-i", "/b/media/talk.mp4"]);
        assert_eq!(inputs.next_input, 3);
        assert_eq!(
            inputs.segments,
            [
                "[1:a:0]aresample=async=1:first_pts=0,atrim=start=0.500000:end=2.000000,asetpts=PTS-STARTPTS,aresample=48000,\
                 aformat=sample_fmts=fltp:channel_layouts=stereo,atempo=1,apad=whole_dur=1.500000,atrim=end=1.500000",
                "[2:a:0]asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,atempo=2,\
                 apad=whole_dur=1.000000,atrim=end=1.000000",
                "anullsrc=r=48000:cl=stereo,atrim=duration=1.000000,aformat=sample_fmts=fltp:channel_layouts=stereo",
                "[1:a:0]aresample=async=1:first_pts=0,atrim=start=4.000000:end=5.000000,asetpts=PTS-STARTPTS,aresample=48000,\
                 aformat=sample_fmts=fltp:channel_layouts=stereo,atempo=0.5,apad=whole_dur=2.000000,atrim=end=2.000000",
            ]
        );
        // No sound anywhere: no inputs and no segments.
        let silent = audio_inputs(&mixed_clips(), &|_| false, false, "/b", "/b/screen.mp4", 1, None, &[]);
        assert_eq!(silent, AudioInputs { next_input: 1, ..AudioInputs::default() });
    }

    #[test]
    fn denoising_runs_once_over_the_whole_recording() {
        let has_audio = |media: &str| media == "media/talk.mp4";
        let plain = audio_inputs(&mixed_clips(), &has_audio, true, "/b", "/b/screen.mp4", 1, None, &[]);
        let inputs = audio_inputs(&mixed_clips(), &has_audio, true, "/b", "/b/screen.mp4", 1, Some("DN"), &[]);
        assert_eq!(inputs.arguments, plain.arguments);
        // One denoiser over the recording's whole sound, split once per recording clip.
        assert_eq!(
            inputs.prelude,
            "[1:a:0]aresample=48000:async=1:first_pts=0,aformat=sample_fmts=fltp:channel_layouts=stereo,DN,\
             asplit=outputs=2[rec0][rec1];"
        );
        // The prelude's chain already placed the sound from the file's time 0.
        let placed = "[1:a:0]aresample=async=1:first_pts=0,";
        assert_eq!(inputs.segments[0], plain.segments[0].replace(placed, "[rec0]"));
        assert_eq!(inputs.segments[1], plain.segments[1]);
        assert_eq!(inputs.segments[2], plain.segments[2]);
        assert_eq!(inputs.segments[3], plain.segments[3].replace(placed, "[rec1]"));
        assert!(!inputs.segments.iter().any(|segment| segment.contains("DN")));
        // No recording clip with sound: nothing to denoise.
        let none = audio_inputs(&mixed_clips(), &has_audio, false, "/b", "/b/screen.mp4", 1, Some("DN"), &[]);
        assert_eq!(none.prelude, "");
    }

    #[test]
    fn transitions_fade_the_recordings_sound_at_their_cuts() {
        let has_audio = |media: &str| media == "media/talk.mp4";
        let plain = audio_inputs(&mixed_clips(), &has_audio, true, "/b", "/b/screen.mp4", 1, None, &[]);
        let fades = [(false, true), (true, false), (false, false), (true, false)];
        let faded = audio_inputs(&mixed_clips(), &has_audio, true, "/b", "/b/screen.mp4", 1, Some("DN"), &fades);
        let placed = "[1:a:0]aresample=async=1:first_pts=0,";
        assert_eq!(faded.segments[0], plain.segments[0].replace(placed, "[rec0]") + &declick((false, true), 1.5));
        // Only the recording's clips fade: an inserted video's sound is its own.
        assert_eq!(faded.segments[1], plain.segments[1]);
        assert_eq!(faded.segments[3], plain.segments[3].replace(placed, "[rec1]") + &declick((true, false), 2.0));
    }

    #[test]
    fn music_joins_the_normalised_program_unmeasured() {
        let segments = ["S0".to_string()];
        let music = Music { file: "media/song.mp3".into(), volume: 0.25, fade_in: 0.0, fade_out: 0.0, ..Music::default() };
        let mix = MusicMix::new(4, &music, 4.0);
        let graph = mastered_audio_filter("P;", &segments, Some("LN"), 0.8, &[0.5], 3, 4.0, Some(&mix));
        // The normalised, trimmed program is split for the ducking key; the music and the clicks
        // join it in the mix.
        assert!(graph.starts_with("P;S0[a0];[a0]concat=n=1:v=0:a=1,LN,volume=0.8,asplit=2[source][duckkey];[4:a:0]"), "{graph}");
        assert!(graph.contains("[musicsrc][duckkey]sidechaincompress="), "{graph}");
        assert!(graph.contains("[source][music][click0]amix=inputs=3:"), "{graph}");
        // The measuring pass hears the program alone.
        assert!(!program_measure_filter("P;", &segments, "M").contains("4:a"));
        // Without a master the graph is the one the music branch always made.
        assert_eq!(
            mastered_audio_filter("", &segments, None, 0.8, &[0.5], 3, 4.0, Some(&mix)),
            audio_filter_with_music(&segments, 0.8, &[0.5], 3, 4.0, Some(&mix))
        );
    }

    #[test]
    fn loudness_normalises_the_program_before_the_volume_and_the_clicks() {
        let segments = ["S0".to_string(), "S1".to_string()];
        // Without a master it is audio_filter's graph.
        assert_eq!(
            mastered_audio_filter("", &segments, None, 0.5, &[0.5], 3, 4.25, None),
            audio_filter(&segments, 0.5, &[0.5], 3, 4.25)
        );
        assert_eq!(
            mastered_audio_filter("P;", &segments, Some("LN"), 0.5, &[0.5], 3, 4.25, None),
            "P;S0[a0];S1[a1];[a0][a1]concat=n=2:v=0:a=1,LN,volume=0.5[source];\
             [3:a]atrim=duration=0.03,asetpts=PTS-STARTPTS,volume=0.25,adelay=500:all=1[click0];\
             [source][click0]amix=inputs=2:normalize=0:duration=longest,apad=pad_dur=4.250000,\
             atrim=duration=4.250000[aout]"
        );
        // The measuring pass hears exactly what LN gets: no volume, no clicks.
        assert_eq!(program_measure_filter("P;", &segments, "M"), "P;S0[a0];S1[a1];[a0][a1]concat=n=2:v=0:a=1,M[aout]");
    }

    /// Mono 32-bit float samples of `path` (any format ffmpeg reads) through `filter`.
    fn samples(args: &[&str]) -> Vec<f32> {
        let mut all: Vec<&str> = vec!["-v", "error"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["-ac", "1", "-f", "f32le", "-"]);
        let out = crate::compat::qprocess::run("ffmpeg", &all, 60000).unwrap();
        assert!(out.succeeded(), "{}", String::from_utf8_lossy(&out.stderr));
        out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
    }

    fn window_db(samples: &[f32], at: usize, length: usize) -> f64 {
        let window = &samples[at.min(samples.len())..(at + length).min(samples.len())];
        let power: f64 = window.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / window.len().max(1) as f64;
        10.0 * power.log10()
    }

    // arnndn holds back 10 ms it never flushes: a denoiser per clip left 10 ms of silence before
    // every cut. The recording denoised once and then cut sounds like the whole recording denoised,
    // right up to each cut.
    #[test]
    fn denoised_cuts_keep_their_sound() {
        use crate::compat::qstandardpaths::find_executable;
        if find_executable("ffmpeg").is_empty() {
            eprintln!("ffmpeg unavailable");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-denoise-cuts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("sound.wav").to_string_lossy().into_owned();
        // Speech-like: a 180 Hz tone with harmonics, its level moving, over a little hiss.
        let made = crate::compat::qprocess::run(
            "ffmpeg",
            &["-y", "-v", "error", "-f", "lavfi", "-i",
              "aevalsrc='(0.3*sin(2*PI*180*t)+0.15*sin(2*PI*360*t)+0.08*sin(2*PI*540*t))*(0.7+0.3*sin(2*PI*2*t))+0.01*(random(0)-0.5)':s=48000:d=6",
              "-ac", "2", &source],
            30000,
        )
        .unwrap();
        assert!(made.succeeded(), "{}", String::from_utf8_lossy(&made.stderr));
        let model = crate::audio_cleanup::model_file(&dir.join("model").to_string_lossy()).unwrap();
        // Half denoised: RNNoise takes a tone for noise, and the half left is what shows a gap.
        let denoise = crate::audio_cleanup::denoise_filter(&model, 0.5);
        // Three cuts, one clip out of order.
        let clips = [(0.5, 2.0), (3.0, 4.0), (2.5, 3.0), (4.5, 6.0)]
            .map(|(in_, out)| Clip { id: format!("c{in_}"), in_, out, speed: 1.0, ..Clip::default() });
        let output_duration: f64 = clips.iter().map(|clip| clip.out - clip.in_).sum();
        // The export's graph for these clips of `source`.
        let export = |source: &str, denoise: Option<&str>| {
            let inputs = audio_inputs(&clips, &|_| false, true, "/b", source, 0, denoise, &[]);
            let graph = mastered_audio_filter(&inputs.prelude, &inputs.segments, None, 1.0, &[], -1, output_duration, None);
            let mut args: Vec<&str> = inputs.arguments.iter().map(String::as_str).collect();
            args.extend(["-filter_complex", &graph, "-map", "[aout]"]);
            samples(&args)
        };
        let cut = export(&source, Some(&denoise));
        // The whole recording denoised, then cut the same way, and the recording cut as it is.
        let whole = dir.join("whole.wav").to_string_lossy().into_owned();
        let filter = format!("aformat=sample_fmts=fltp:channel_layouts=stereo,{denoise}");
        let made = crate::compat::qprocess::run(
            "ffmpeg", &["-y", "-v", "error", "-i", &source, "-af", &filter, "-c:a", "pcm_f32le", &whole], 60000).unwrap();
        assert!(made.succeeded(), "{}", String::from_utf8_lossy(&made.stderr));
        assert_eq!(samples(&["-i", &whole]).len(), samples(&["-i", &source]).len(), "the denoised sound keeps its length");
        let expected = export(&whole, None);
        let recorded = export(&source, None);
        assert!((cut.len() as f64 - output_duration * 48000.0).abs() <= 2.0, "{} samples", cut.len());
        // 2 ms windows over the 20 ms before and after each cut.
        let window = 96;
        let mut output_start = 0usize;
        for clip in &clips {
            let length = ((clip.out - clip.in_) * 48000.0).round() as usize;
            for offset in (0..960).step_by(window).chain((length - 960..length - window + 1).step_by(window)) {
                let at = output_start + offset;
                let (heard, expected, recorded) =
                    (window_db(&cut, at, window), window_db(&expected, at, window), window_db(&recorded, at, window));
                assert!(
                    (heard - expected).abs() < 0.5 && heard > recorded - 12.0,
                    "clip {}-{} at +{offset}: {heard:.1} dB, denoised whole {expected:.1} dB, recorded {recorded:.1} dB",
                    clip.in_,
                    clip.out
                );
            }
            output_start += length;
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn music_joins_the_mix_without_changing_the_rest() {
        let segments = ["S0".to_string()];
        // Without music the graph is the one audio_filter always made.
        assert_eq!(
            audio_filter_with_music(&segments, 1.0, &[0.5], 3, 4.25, None),
            audio_filter(&segments, 1.0, &[0.5], 3, 4.25)
        );
        let music = Music { file: "media/song.mp3".into(), volume: 0.25, fade_in: 1.0, fade_out: 2.0, ..Music::default() };
        let mix = MusicMix::new(4, &music, 4.25);
        assert_eq!(
            audio_filter_with_music(&segments, 0.8, &[0.5], 3, 4.25, Some(&mix)),
            "S0[a0];[a0]concat=n=1:v=0:a=1,volume=0.8,asplit=2[source][duckkey];\
             [4:a:0]asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,\
             apad=whole_dur=4.250000,atrim=end=4.250000,volume=0.25,afade=t=in:st=0:d=1.000000,\
             afade=t=out:st=2.250000:d=2.000000[musicsrc];\
             [musicsrc][duckkey]sidechaincompress=threshold=0.03:ratio=6:attack=20:release=400[music];\
             [3:a]atrim=duration=0.03,asetpts=PTS-STARTPTS,volume=0.25,adelay=500:all=1[click0];\
             [source][music][click0]amix=inputs=3:normalize=0:duration=longest,apad=pad_dur=4.250000,\
             atrim=duration=4.250000[aout]"
        );
        // No ducking without the clips' sound, nor when it is off; no fades of length 0.
        let quiet = Music { duck: false, fade_in: 0.0, fade_out: 0.0, ..music.clone() };
        assert_eq!(
            audio_filter_with_music(&[], 1.0, &[], -1, 3.0, Some(&MusicMix::new(1, &music, 3.0))),
            "[1:a:0]asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,\
             apad=whole_dur=3.000000,atrim=end=3.000000,volume=0.25,afade=t=in:st=0:d=1.000000,\
             afade=t=out:st=1.000000:d=2.000000[music];\
             [music]amix=inputs=1:normalize=0:duration=longest,apad=pad_dur=3.000000,atrim=duration=3.000000[aout]"
        );
        assert_eq!(
            audio_filter_with_music(&segments, 1.0, &[], -1, 3.0, Some(&MusicMix::new(1, &quiet, 3.0))),
            "S0[a0];[a0]concat=n=1:v=0:a=1,volume=1[source];\
             [1:a:0]asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,\
             apad=whole_dur=3.000000,atrim=end=3.000000,volume=0.25[music];\
             [source][music]amix=inputs=2:normalize=0:duration=longest,apad=pad_dur=3.000000,atrim=duration=3.000000[aout]"
        );
        // Fades longer than the output are cut to it.
        assert_eq!(MusicMix::new(1, &Music { fade_in: 9.0, fade_out: 9.0, ..music.clone() }, 3.0).fade_out, 3.0);
        assert_eq!(music_input_arguments(&music, "/b/media/song.mp3"), ["-stream_loop", "-1", "-i", "/b/media/song.mp3"]);
        let once = Music { loop_: false, start: 12.5, ..music };
        assert_eq!(music_input_arguments(&once, "/s.mp3"), ["-ss", "12.500000", "-i", "/s.mp3"]);
    }

    /// The mean level in dB of `path` between two times, of the music's 220 Hz (`low`) or the
    /// program's 2 kHz.
    fn band_level(path: &str, from: f64, to: f64, low: bool) -> f64 {
        let filter = if low { "lowpass=f=400,lowpass=f=400,lowpass=f=400,volumedetect" } else { "highpass=f=1200,highpass=f=1200,highpass=f=1200,volumedetect" };
        let finished = crate::compat::qprocess::run(
            "ffmpeg",
            &["-v", "info", "-ss", &format!("{from}"), "-t", &format!("{}", to - from), "-i", path, "-af", &filter, "-f", "null", "-"],
            30000,
        )
        .unwrap();
        let log = String::from_utf8_lossy(&finished.stderr);
        let value = log.split("mean_volume: ").nth(1).and_then(|rest| rest.split(' ').next()).unwrap_or("-inf");
        value.parse().unwrap_or(-200.0)
    }

    /// Runs the export's audio graph with ffmpeg: a 6 s program, silent for 3 s and then a loud
    /// 2 kHz tone, under 2 s of 220 Hz music that loops.
    fn mix_with_music(dir: &std::path::Path, duck: bool, name: &str) -> String {
        let music_path = dir.join("music.wav").to_string_lossy().into_owned();
        if !std::path::Path::new(&music_path).exists() {
            let made = crate::compat::qprocess::run(
                "ffmpeg",
                &["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=220:sample_rate=44100:duration=2", &music_path],
                30000,
            )
            .unwrap();
            assert!(made.succeeded(), "{}", String::from_utf8_lossy(&made.stderr));
        }
        let music = Music { file: "media/music.wav".into(), volume: 1.0, fade_in: 0.0, fade_out: 0.0, duck, ..Music::default() };
        let mix = MusicMix::new(1, &music, 6.0);
        let segments = [
            silent_segment(3.0),
            audio_segment("[0:a:0]atrim=start=0:end=3,asetpts=PTS-STARTPTS,volume=4", 1.0, 3.0),
        ];
        let graph = audio_filter_with_music(&segments, 1.0, &[], -1, 6.0, Some(&mix));
        let output = dir.join(name).to_string_lossy().into_owned();
        let mut args: Vec<String> =
            ["-y", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=2000:sample_rate=48000:duration=3"].map(String::from).into();
        args.extend(music_input_arguments(&music, &music_path));
        args.extend(["-filter_complex".into(), graph, "-map".into(), "[aout]".into(), output.clone()]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let finished = crate::compat::qprocess::run("ffmpeg", &args, 60000).unwrap();
        assert!(finished.succeeded(), "{}", String::from_utf8_lossy(&finished.stderr));
        output
    }

    #[test]
    fn music_plays_under_the_program_loops_and_ducks() {
        use crate::compat::qstandardpaths::find_executable;
        if find_executable("ffmpeg").is_empty() {
            eprintln!("ffmpeg unavailable");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-music-mix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ducked = mix_with_music(&dir, true, "ducked.wav");
        let plain = mix_with_music(&dir, false, "plain.wav");
        let duration = crate::compat::qprocess::run(
            "ffprobe",
            &["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", &ducked],
            15000,
        )
        .unwrap();
        let duration: f64 = String::from_utf8_lossy(&duration.stdout).trim().parse().unwrap();
        assert!((duration - 6.0).abs() < 0.05, "{duration}");
        // The music alone at the start, at its own level: sine's -21 dBFS mean, a little less
        // through the band filter.
        let alone = band_level(&ducked, 0.2, 2.8, true);
        assert!(alone > -27.0 && alone < -19.0, "music alone at {alone} dB");
        // It loops past its 2 s, under the program; without ducking at the same level.
        let under = band_level(&plain, 3.3, 5.8, true);
        assert!((under - alone).abs() < 2.0, "music under the program at {under} dB, alone at {alone} dB");
        // Ducked under the program's tone (-9 dBFS RMS) by over 12 dB.
        let ducked_level = band_level(&ducked, 3.5, 5.8, true);
        assert!(ducked_level < alone - 12.0, "ducked music at {ducked_level} dB, alone at {alone} dB");
        // The program passes at its own level either way (sine at x4: -9 dBFS mean, less through
        // the band filter).
        let program_ducked = band_level(&ducked, 3.3, 5.8, false);
        let program_plain = band_level(&plain, 3.3, 5.8, false);
        assert!((program_ducked - program_plain).abs() < 0.5, "{program_ducked} vs {program_plain}");
        assert!(program_plain > -16.0, "{program_plain}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cuts_with_a_transition_fade_their_sound() {
        assert_eq!(declick((false, false), 2.0), "");
        assert_eq!(declick((true, false), 2.0), ",afade=t=in:st=0:d=0.020000");
        assert_eq!(declick((true, true), 2.0), ",afade=t=in:st=0:d=0.020000,afade=t=out:st=1.980000:d=0.020000");
        assert_eq!(declick((false, true), 0.03), ",afade=t=out:st=0.015000:d=0.015000");
    }

    #[test]
    fn quality_tables() {
        assert_eq!(
            ["low", "medium", "high", "best", "studio", "x"].map(normalized_quality),
            ["web-low", "web-high", "social", "studio", "studio", "x"]
        );
        assert_eq!(["web-low", "web-high", "social", "studio", ""].map(bitrate_multiplier), [0.0075, 0.0175, 0.05, 0.3, 0.05]);
        assert_eq!(output_aspect("auto", 640, 360, &RectF::new(0.0, 0.0, 0.5, 1.0)), 320.0 / 360.0);
        assert_eq!(output_aspect("9:16", 640, 360, &RectF::new(0.0, 0.0, 1.0, 1.0)), 9.0 / 16.0);
    }

    #[test]
    fn gif_arguments_flip_only_opengl_frames() {
        let args = gif_arguments(480, 270, 12, "/run/raw.rgba", "studio", false, true, "/out.gif");
        assert_eq!(&args[12..], ["/run/raw.rgba", "-vf",
            "vflip,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
            "-loop", "0", "/out.gif"]);
        assert_eq!(&gif_arguments(4, 2, 1, "r", "social", true, false, "o")[12..], ["r", "-loop", "-1", "o"]);
        assert_eq!(&gif_arguments(4, 2, 1, "r", "social", false, false, "o")[13..], ["-vf", "vflip", "-loop", "-1", "o"]);
    }

    #[test]
    fn encoders_go_nvenc_vaapi_x264() {
        let intel = || Some("/dev/dri/renderD128".to_string());
        let vaapi = VideoEncoder::Vaapi("/dev/dri/renderD128".into());
        assert_eq!(video_encoders("", false, intel()), [VideoEncoder::Nvenc, vaapi.clone(), VideoEncoder::X264]);
        assert_eq!(video_encoders("", true, intel()), [vaapi.clone(), VideoEncoder::X264]);
        assert_eq!(video_encoders("", false, None), [VideoEncoder::Nvenc, VideoEncoder::X264]);
        assert_eq!(video_encoders("vaapi", false, intel()), [vaapi, VideoEncoder::X264]);
        assert_eq!(video_encoders("vaapi", false, None), [VideoEncoder::X264]);
        assert_eq!(video_encoders("nvenc", false, intel()), [VideoEncoder::Nvenc, VideoEncoder::X264]);
        assert_eq!(video_encoders("nvenc", true, intel()), [VideoEncoder::X264]);
        assert_eq!(video_encoders("x264", false, intel()), [VideoEncoder::X264]);
    }

    #[test]
    fn encoders_convert_with_bt709_and_say_so() {
        let tags = ["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"];
        let rate = ["-b:v", "1000", "-maxrate", "1200", "-bufsize", "2000", "-profile:v", "high"];
        let x264 = VideoEncoder::X264.video_arguments(true, 1000);
        assert_eq!(&x264[..2], ["-vf",
            "vflip,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,\
             setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv"]);
        assert_eq!(&x264[2..6], ["-c:v", "libx264", "-preset", "medium"]);
        assert_eq!(&x264[6..14], rate);
        assert_eq!(&x264[14..], tags);
        let nvenc = VideoEncoder::Nvenc.video_arguments(false, 1000);
        assert_eq!(&nvenc[..2], ["-vf",
            "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,\
             setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv"]);
        assert_eq!(&nvenc[2..6], ["-c:v", "h264_nvenc", "-preset", "p5"]);
        assert_eq!(&nvenc[14..], tags);
        let vaapi = VideoEncoder::Vaapi("/dev/dri/renderD129".into());
        assert_eq!(vaapi.device_arguments(), ["-vaapi_device", "/dev/dri/renderD129"]);
        assert!(VideoEncoder::X264.device_arguments().is_empty());
        let args = vaapi.video_arguments(true, 1000);
        assert_eq!(&args[..2], ["-vf",
            "vflip,scale=out_color_matrix=bt709:out_range=tv,format=nv12,\
             setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv,hwupload"]);
        assert_eq!(&args[2..6], ["-c:v", "h264_vaapi", "-rc_mode", "VBR"]);
        assert_eq!(&args[6..14], rate);
        assert_eq!(&args[14..], tags);
    }

    /// ffmpeg's answer for a colour sent through `encoder` the way export sends frames, read
    /// back with the file's tags: its RGB, or `None` when the encoder is not there.
    fn round_trip(encoder: &VideoEncoder, colour: &str, path: &str) -> Option<[u8; 3]> {
        let mut args: Vec<String> = ["-y", "-v", "error"].map(String::from).into();
        args.extend(encoder.device_arguments());
        args.extend(["-f", "lavfi", "-i", &format!("color=c={colour}:size=256x144:rate=30,format=rgba"), "-frames:v", "5"]
            .map(String::from));
        args.extend(encoder.video_arguments(false, 1_000_000));
        args.push(path.into());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        if !crate::compat::qprocess::run("ffmpeg", &args, 30000)?.succeeded() {
            return None;
        }
        let decoded = crate::compat::qprocess::run(
            "ffmpeg",
            &["-v", "error", "-i", path, "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
            30000,
        )?;
        let centre = (72 * 256 + 128) * 3;
        Some([decoded.stdout[centre], decoded.stdout[centre + 1], decoded.stdout[centre + 2]])
    }

    #[test]
    fn exported_colours_come_back_as_rendered() {
        use crate::compat::qstandardpaths::find_executable;
        if find_executable("ffmpeg").is_empty() || find_executable("ffprobe").is_empty() {
            eprintln!("ffmpeg/ffprobe unavailable");
            return;
        }
        let dir = std::env::temp_dir().join(format!("omareel-export-colour-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut encoders = vec![VideoEncoder::X264];
        // The GPU encoders this machine has: they have to agree too.
        let device = crate::export::hardware::vaapi_device();
        for encoder in video_encoders("", false, device) {
            if encoder != VideoEncoder::X264 && encoder_works(&encoder, 256, 144, 1_000_000) {
                encoders.push(encoder);
            }
        }
        for encoder in &encoders {
            let path = dir.join(format!("{}.mp4", encoder.name())).to_string_lossy().into_owned();
            for (colour, rgb) in [("0xff0000", [255, 0, 0]), ("0x00ff00", [0, 255, 0]), ("0x0000ff", [0, 0, 255]),
                                  ("0xff8020", [255, 128, 32]), ("0x808080", [128, 128, 128])] {
                let Some(got) = round_trip(encoder, colour, &path) else {
                    panic!("{} could not encode {colour}", encoder.name());
                };
                // Within a couple of code values: BT.601 instead turns red into (255, 23, 0).
                assert!(got.iter().zip(rgb).all(|(a, b)| (i32::from(*a) - b).abs() <= 3),
                        "{}: {colour} came back as {got:?}", encoder.name());
            }
            let probe = crate::compat::qprocess::run(
                "ffprobe",
                &["-v", "error", "-select_streams", "v:0", "-show_entries",
                  "stream=pix_fmt,color_space,color_primaries,color_transfer,color_range", "-of", "compact=p=0:nk=1", &path],
                15000,
            )
            .unwrap();
            assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "yuv420p|tv|bt709|bt709|bt709", "{}", encoder.name());
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
