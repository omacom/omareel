//! Single frames of an export (`omareel frame`): which output frame a time shows, where the
//! export's decoders would be at that frame, and the command's own arithmetic (its arguments,
//! file names, labels and contact sheet).
//!
//! The composition is stateless per frame, so a still only needs the pictures the export's
//! decode thread would hand over for that frame. Each of its decoders runs ffmpeg's `fps`
//! filter on a grid anchored where the decoder started: the clip's at its in point, at the
//! output rate over the clip's speed; the webcam's at zero, at the webcam's rate. Starting a
//! decoder on one of its own grid points instead, the frame it gives first is the one the
//! export's would give there, without decoding the frames before it.

use crate::clip_timeline::ClipTimeline;
use crate::compat::cpp;
use crate::compat::qnumber::number_f64;
use crate::project::Clip;

/// Where the export's decoder for a clip is at an output frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipSeek {
    /// The clip on screen, as the decode thread finds it.
    pub clip: i32,
    /// How many frames the clip's decoder gave before this one.
    pub index: i32,
}

/// The output frame on screen at `time` seconds: the last frame that starts at or before it,
/// in `[0, total_frames)`.
pub fn output_frame(time: f64, fps: i32, total_frames: i32) -> i32 {
    let frame = if time.is_finite() { cpp::int_cast((time * f64::from(fps) + 1e-6).floor()) } else { 0 };
    cpp::clamp(frame, 0, cpp::max(0, total_frames - 1))
}

/// When output frame `frame` starts, as the export computes it.
pub fn frame_time(frame: i32, fps: i32) -> f64 {
    f64::from(frame) / f64::from(fps)
}

/// The clip at output frame `frame` and its decoder's frame there: the frames since the first
/// output frame the decode thread gave that clip.
pub fn clip_seek(timeline: &ClipTimeline, fps: i32, frame: i32) -> ClipSeek {
    let clip_of = |f: i32| timeline.clip_at(frame_time(f, fps), None);
    let clip = clip_of(frame);
    // clip_at only grows with time: the first frame of the clip is the first one not before it.
    let (mut low, mut high) = (0, frame);
    while low < high {
        let middle = low + (high - low) / 2;
        if clip_of(middle) < clip {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    ClipSeek { clip, index: frame - low }
}

/// The rate the export's decoder runs `clip` at, as ffmpeg reads it from the command line.
fn clip_rate(clip: &Clip, fps: i32) -> f64 {
    number_f64(f64::from(fps) / clip.speed, 'g', 12).parse().unwrap_or(f64::from(fps))
}

/// Where in its file a decoder for `clip` starts so that its first frame is the export's frame
/// `index` of that clip.
pub fn clip_start(clip: &Clip, fps: i32, index: i32) -> f64 {
    clip.in_ + f64::from(index) / clip_rate(clip, fps)
}

/// The webcam frame the export shows at `seconds` into the webcam video.
pub fn camera_frame(seconds: f64, fps: f64) -> i32 {
    let frames = if cfg!(target_arch = "aarch64") {
        // GCC fuses the multiply and add into one fmadd on aarch64.
        seconds.mul_add(fps, 1e-6)
    } else {
        seconds * fps + 1e-6
    };
    cpp::max(0, cpp::int_cast(frames.floor()))
}

/// Where a decoder for the webcam starts so that its first frame is the export's frame `index`.
pub fn camera_start(fps: f64, index: i32) -> f64 {
    let rate: f64 = number_f64(fps, 'g', 12).parse().unwrap_or(fps);
    f64::from(index) / rate
}

/// The grid frames to start a decoder at, in turn, for frame `index`: there first; when the
/// video gives nothing from there (the frame lies past the video's end, so the export shows the
/// last one it got), `back` frames earlier; and last from the start, as the export decoded it.
pub fn fallback_starts(index: i32, back: i32) -> Vec<i32> {
    let mut starts = vec![cpp::max(0, index)];
    for start in [cpp::max(0, index - back), 0] {
        if starts.last() != Some(&start) {
            starts.push(start);
        }
    }
    starts
}

/// `count` times spread evenly over `duration`: the middle of each of `count` equal parts, so
/// neither the first frame nor the last (often a fade or a blank screen) is one of them.
pub fn spaced_times(count: i32, duration: f64) -> Vec<f64> {
    (0..cpp::max(0, count)).map(|i| (f64::from(i) + 0.5) * duration / f64::from(count)).collect()
}

/// A time on the output timeline as `mm:ss.s` (`h:mm:ss.s` from an hour).
pub fn time_label(seconds: f64) -> String {
    let tenths = cpp::int64_cast((cpp::max(0.0, seconds) * 10.0).floor());
    let (hours, minutes, seconds, tenth) = (tenths / 36000, tenths / 600 % 60, tenths / 10 % 60, tenths % 10);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}.{tenth}")
    } else {
        format!("{minutes:02}:{seconds:02}.{tenth}")
    }
}

/// Seconds as `--at` takes them: `12.5`, `1:02.5` or `1:00:02.5`. `None` for anything else.
pub fn parse_time(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    for (i, part) in parts.iter().enumerate() {
        let last = i == parts.len() - 1;
        let valid = !part.is_empty()
            && part.chars().all(|c| c.is_ascii_digit() || (last && c == '.'))
            && (last || part.len() <= 2 || i == 0);
        if !valid {
            return None;
        }
        let value: f64 = part.parse().ok()?;
        if i > 0 && value >= 60.0 {
            return None;
        }
        seconds = seconds * 60.0 + value;
    }
    Some(seconds).filter(|s| s.is_finite())
}

/// The file each of `count` stills goes to: `output` with `%d` replaced by the still's number
/// from 1; without `%d`, `output` itself for one still, else `-1`, `-2`... before its extension.
pub fn output_paths(output: &str, count: usize) -> Vec<String> {
    (1..=count)
        .map(|n| {
            if output.contains("%d") {
                output.replace("%d", &n.to_string())
            } else if count == 1 {
                output.to_owned()
            } else {
                let name_start = output.rfind('/').map_or(0, |slash| slash + 1);
                match output[name_start..].rfind('.').filter(|&dot| dot > 0) {
                    Some(dot) => format!("{}-{n}{}", &output[..name_start + dot], &output[name_start + dot..]),
                    None => format!("{output}-{n}"),
                }
            }
        })
        .collect()
}

/// The image formats `omareel frame` writes, by extension.
pub fn image_format(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase())?;
    match extension.as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "webp" => Some("webp"),
        _ => None,
    }
}

/// The contact sheet's space between tiles and around them, in pixels.
pub const SHEET_GAP: i32 = 8;
/// The contact sheet's width when `--width` doesn't say.
pub const SHEET_WIDTH: i32 = 1920;

/// How a contact sheet of `count` tiles is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetGrid {
    pub columns: i32,
    pub rows: i32,
    /// The width each tile renders at.
    pub tile_width: i32,
}

/// `count` tiles in a near-square grid (more columns than rows when it can't be square), on a
/// sheet `width` pixels wide.
pub fn sheet_grid(count: i32, width: i32) -> SheetGrid {
    let count = cpp::max(1, count);
    let columns = cpp::int_cast(f64::from(count).sqrt().ceil());
    let rows = (count + columns - 1) / columns;
    let tile_width = cpp::max(16, (width - (columns + 1) * SHEET_GAP) / columns);
    SheetGrid { columns, rows, tile_width }
}

/// What `omareel frame` was asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameRequest {
    pub bundle: String,
    pub output: String,
    /// `--at` times as given, in order.
    pub times: Vec<f64>,
    /// `--count`: this many evenly spaced frames instead.
    pub count: i32,
    pub sheet: bool,
    /// The still's width, or the contact sheet's; 0 for the project's.
    pub width: i32,
    /// 0 for the project's frame rate.
    pub fps: i32,
}

/// The most frames one `omareel frame` renders.
pub const MAX_STILLS: i32 = 400;

/// `omareel frame`'s arguments (after `frame`), or what is wrong with them.
pub fn parse_frame_args(args: &[String]) -> Result<FrameRequest, String> {
    let mut request = FrameRequest::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--sheet" {
            request.sheet = true;
        } else if matches!(arg, "-o" | "--at" | "--count" | "--width" | "--fps") {
            i += 1;
            let Some(value) = args.get(i) else { return Err(format!("{arg} requires a value")) };
            let positive = || value.parse::<i32>().ok().filter(|&n| n > 0);
            match arg {
                "-o" => request.output = value.clone(),
                "--at" => request.times.push(
                    parse_time(value).ok_or_else(|| format!("--at takes seconds (12.5 or 1:02.5), not {value}"))?,
                ),
                "--count" => {
                    request.count = positive()
                        .filter(|&n| n <= MAX_STILLS)
                        .ok_or_else(|| format!("--count takes a number from 1 to {MAX_STILLS}, not {value}"))?
                }
                "--width" => request.width = positive().ok_or_else(|| format!("--width takes a positive number, not {value}"))?,
                _ => request.fps = positive().ok_or_else(|| format!("--fps takes a positive number, not {value}"))?,
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unknown frame option {arg}"));
        } else if request.bundle.is_empty() {
            request.bundle = arg.to_owned();
        } else {
            return Err(format!("unexpected argument {arg}"));
        }
        i += 1;
    }
    if request.bundle.is_empty() {
        return Err("frame requires a bundle".into());
    }
    if request.output.is_empty() {
        return Err("frame requires -o <file.png|file.jpg>".into());
    }
    if image_format(&request.output).is_none() {
        return Err("frame output must end in .png, .jpg or .webp".into());
    }
    if request.times.is_empty() == (request.count == 0) {
        return Err("frame takes --at SECONDS (one or more) or --count N".into());
    }
    if request.times.len() > MAX_STILLS as usize {
        return Err(format!("frame renders at most {MAX_STILLS} frames"));
    }
    if request.sheet && request.output.contains("%d") {
        return Err("--sheet writes one file; drop the %d".into());
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(in_: f64, out: f64, speed: f64) -> Clip {
        Clip { in_, out, speed, ..Clip::default() }
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn a_time_shows_the_frame_that_started_at_or_before_it() {
        assert_eq!(output_frame(0.0, 60, 600), 0);
        assert_eq!(output_frame(1.0, 60, 600), 60);
        assert_eq!(output_frame(1.0 - 1e-9, 60, 600), 60);
        assert_eq!(output_frame(1.016, 60, 600), 60);
        assert_eq!(output_frame(2.35, 60, 600), 141);
        // Clamped into the video.
        assert_eq!(output_frame(-3.0, 60, 600), 0);
        assert_eq!(output_frame(10.0, 60, 600), 599);
        assert_eq!(output_frame(99.0, 60, 600), 599);
        assert_eq!(output_frame(f64::NAN, 60, 600), 0);
    }

    // The decode thread walks the frames from 0, starting each clip's decoder at the first frame
    // clip_at gives it; a seek must agree with that walk wherever it lands.
    #[test]
    fn seeks_agree_with_the_export_walking_every_frame() {
        let timeline = ClipTimeline::new(vec![
            clip(0.0, 2.0, 1.0),
            clip(5.0, 7.3, 1.5),
            Clip { in_: 0.5, out: 1.7, media: "media/a.mp4".into(), ..Clip::default() },
            clip(10.0, 12.0, 0.7),
            clip(12.0, 12.01, 1.0),
            clip(20.0, 23.0, 2.0),
        ]);
        for fps in [24, 30, 60] {
            let total = cpp::int_cast((timeline.total_duration() * f64::from(fps)).ceil());
            let (mut active, mut first) = (-1, 0);
            for frame in 0..total {
                let clip = timeline.clip_at(frame_time(frame, fps), None);
                if clip != active {
                    active = clip;
                    first = frame;
                }
                assert_eq!(clip_seek(&timeline, fps, frame), ClipSeek { clip, index: frame - first }, "{fps} fps, frame {frame}");
            }
        }
    }

    #[test]
    fn decoders_start_on_the_exports_grid() {
        assert_eq!(clip_start(&clip(3.0, 9.0, 1.0), 60, 0), 3.0);
        assert!((clip_start(&clip(3.0, 9.0, 1.0), 60, 90) - 4.5).abs() < 1e-12);
        // Twice as fast: the decoder runs at 30 frames of source a second of output.
        assert!((clip_start(&clip(3.0, 9.0, 2.0), 60, 90) - 6.0).abs() < 1e-12);
        // A rate ffmpeg is given rounded to 12 digits is used rounded.
        let odd = clip(0.0, 100.0, 1.3);
        let rate = 60.0 / 1.3;
        assert!((clip_start(&odd, 60, 4000) - 4000.0 / rate).abs() < 1e-6);
        assert_eq!(camera_start(30.0, 45), 1.5);
        assert_eq!(camera_frame(1.5, 30.0), 45);
        assert_eq!(camera_frame(1.0 / 30.0 * 7.0, 30.0), 7);
        assert_eq!(camera_frame(0.0, 30.0), 0);
        assert_eq!(camera_frame(1.49, 30.0), 44);
    }

    #[test]
    fn falls_back_to_earlier_starts_without_repeating_one() {
        assert_eq!(fallback_starts(500, 120), [500, 380, 0]);
        assert_eq!(fallback_starts(100, 120), [100, 0]);
        assert_eq!(fallback_starts(0, 120), [0]);
    }

    #[test]
    fn spaces_times_over_the_middle_of_equal_parts() {
        assert_eq!(spaced_times(1, 10.0), [5.0]);
        assert_eq!(spaced_times(4, 8.0), [1.0, 3.0, 5.0, 7.0]);
        assert!(spaced_times(0, 8.0).is_empty());
    }

    #[test]
    fn labels_times_in_minutes_seconds_and_tenths() {
        assert_eq!(time_label(0.0), "00:00.0");
        assert_eq!(time_label(5.06), "00:05.0");
        assert_eq!(time_label(83.45), "01:23.4");
        assert_eq!(time_label(3723.9), "1:02:03.9");
        assert_eq!(time_label(-1.0), "00:00.0");
    }

    #[test]
    fn parses_seconds_and_clock_times() {
        assert_eq!(parse_time("12.5"), Some(12.5));
        assert_eq!(parse_time("0"), Some(0.0));
        assert_eq!(parse_time("1:02.5"), Some(62.5));
        assert_eq!(parse_time("01:23.4"), Some(83.4));
        assert_eq!(parse_time("1:00:02"), Some(3602.0));
        for bad in ["", "abc", "1:", ":5", "1:75", "1.5:02", "1:2:3:4", "-2", "1e3", "inf", "NaN", "5s"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn names_each_stills_file() {
        assert_eq!(output_paths("/x/out.png", 1), ["/x/out.png"]);
        assert_eq!(output_paths("/x/out.png", 2), ["/x/out-1.png", "/x/out-2.png"]);
        assert_eq!(output_paths("/x.d/out", 2), ["/x.d/out-1", "/x.d/out-2"]);
        assert_eq!(output_paths("/x/f%d.jpg", 2), ["/x/f1.jpg", "/x/f2.jpg"]);
        assert_eq!(output_paths("/x/f%d.jpg", 1), ["/x/f1.jpg"]);
        assert_eq!(output_paths("/x/.png", 2), ["/x/.png-1", "/x/.png-2"]);
    }

    #[test]
    fn knows_the_image_formats() {
        assert_eq!(image_format("a.PNG"), Some("png"));
        assert_eq!(image_format("a.jpeg"), Some("jpg"));
        assert_eq!(image_format("a.webp"), Some("webp"));
        assert_eq!(image_format("a.mp4"), None);
        assert_eq!(image_format("png"), None);
    }

    #[test]
    fn lays_sheets_out_near_square() {
        assert_eq!(sheet_grid(1, 1920), SheetGrid { columns: 1, rows: 1, tile_width: 1904 });
        assert_eq!(sheet_grid(12, 1920), SheetGrid { columns: 4, rows: 3, tile_width: 470 });
        assert_eq!(sheet_grid(16, 1920), SheetGrid { columns: 4, rows: 4, tile_width: 470 });
        assert_eq!(sheet_grid(5, 1000), SheetGrid { columns: 3, rows: 2, tile_width: 322 });
    }

    #[test]
    fn parses_the_command_line() {
        let request = parse_frame_args(&args("b.omareel --at 1.5 --at 0:03 -o out.png --width 640")).unwrap();
        assert_eq!(request, FrameRequest {
            bundle: "b.omareel".into(),
            output: "out.png".into(),
            times: vec![1.5, 3.0],
            width: 640,
            ..FrameRequest::default()
        });
        let sheet = parse_frame_args(&args("b --count 12 --sheet -o s.jpg --fps 30")).unwrap();
        assert_eq!((sheet.count, sheet.sheet, sheet.fps), (12, true, 30));
        for (line, error) in [
            ("", "frame requires a bundle"),
            ("b --at 1", "frame requires -o <file.png|file.jpg>"),
            ("b --at 1 -o x.mp4", "frame output must end in .png, .jpg or .webp"),
            ("b -o x.png", "frame takes --at SECONDS (one or more) or --count N"),
            ("b --at 1 --count 2 -o x.png", "frame takes --at SECONDS (one or more) or --count N"),
            ("b --at soon -o x.png", "--at takes seconds (12.5 or 1:02.5), not soon"),
            ("b --at -1 -o x.png", "--at takes seconds (12.5 or 1:02.5), not -1"),
            ("b --count 0 -o x.png", "--count takes a number from 1 to 400, not 0"),
            ("b --at 1 --width -5 -o x.png", "--width takes a positive number, not -5"),
            ("b --at", "--at requires a value"),
            ("b --at 1 -o x.png --zoom", "unknown frame option --zoom"),
            ("b c --at 1 -o x.png", "unexpected argument c"),
            ("b --count 3 --sheet -o s%d.png", "--sheet writes one file; drop the %d"),
        ] {
            assert_eq!(parse_frame_args(&args(line)), Err(error.to_owned()), "{line}");
        }
    }
}
