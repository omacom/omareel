//! What `Exporter.cpp` computes before it renders: the recording's ffprobe summary, the output
//! size, the quality and bitrate tables, the audio `filter_complex`, the video encoder (which,
//! its arguments and its probe) and the GIF palette pass's arguments.

use crate::compat::geometry::RectF;
use crate::compat::qjson;
use crate::compat::qnumber::{self, number_f64};
use crate::compat::qprocess::Process;
use crate::compat::qround::{q_round, q_round64};
use crate::compat::{cpp, qstring};
use crate::input_log::InputLog;
use crate::media_probe::VideoInfo;
use crate::project::Clip;

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
}

impl Default for MediaInfo {
    fn default() -> MediaInfo {
        MediaInfo { width: 0, height: 0, duration: 0.0, fps: 30.0, audio: false, colour_tagged: false }
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
            "stream=codec_type,width,height,r_frame_rate,color_space:format=duration",
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

pub fn silent_segment(length: f64) -> String {
    format!(
        "anullsrc=r=48000:cl=stereo,atrim=duration={},aformat=sample_fmts=fltp:channel_layouts=stereo",
        number_f64(length, 'f', 6)
    )
}

/// The `-filter_complex` that mixes the clips' audio (`segments`, one chain per clip in output
/// order, or none when no clip has audio) with a click sound from input `click_input` at each
/// of `clicks`, padded or cut to the output's length.
pub fn audio_filter(segments: &[String], volume: f64, clicks: &[f64], click_input: i32, output_duration: f64) -> String {
    let mut filter = String::new();
    let mut mix_labels: Vec<String> = Vec::new();
    if !segments.is_empty() {
        let mut clip_labels: Vec<String> = Vec::new();
        for (i, segment) in segments.iter().enumerate() {
            let label = format!("a{i}");
            filter += &format!("{segment}[{label}];");
            clip_labels.push(format!("[{label}]"));
        }
        filter += &format!(
            "{}concat=n={}:v=0:a=1,volume={}[source];",
            clip_labels.concat(),
            segments.len(),
            number_f64(volume, 'g', 8)
        );
        mix_labels.push("[source]".into());
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
