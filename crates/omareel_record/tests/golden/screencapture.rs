//! screencapture: `screen_capture` against the parts of `ScreenCapture.cpp` that need no
//! compositor (README "screencapture"): the capture ring, the crop's row copies, the Matroska
//! headers, `start`'s crop trim and encoder arguments, the encoder probe and its cache (ffmpeg
//! answered by the replay shim), and the writer loop driven as the oracle drives it: frames
//! queued into the ring as `captureFrame` queues them, the stream written into a file.

use crate::hermetic::*;
use crate::replay::*;
use crate::support::*;
use omareel_core::compat::qjson::{Format, Object, Value};
use omareel_record::screen_capture::*;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

fn cases() -> Object {
    read_json("screencapture/cases.json").to_object()
}

fn rect_from(value: &Value) -> Rect {
    let r = value.to_array();
    let at = |i: usize| r.get(i).map_or(0, |v| v.to_int(0));
    Rect::new(at(0), at(1), at(2), at(3))
}

fn size_from(value: &Value) -> Size {
    let s = value.to_array();
    let at = |i: usize| s.get(i).map_or(0, |v| v.to_int(0));
    Size::new(at(0), at(1))
}

fn rect_text(rect: &Rect) -> String {
    format!("{},{} {}x{}", rect.x(), rect.y(), rect.width(), rect.height())
}

fn size_text(size: Size) -> String {
    format!("{}x{}", size.width, size.height)
}

fn row_text(row: &CaptureRowCopy) -> String {
    format!("{}:{}:{}", row.source_offset, row.destination_offset, row.bytes)
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn ring_states(ring: &CaptureRingBookkeeping, slots: i32) -> String {
    (0..slots.max(0))
        .map(|slot| match ring.state(slot) {
            SlotState::Available => 'A',
            SlotState::Capturing => 'C',
            SlotState::Queued => 'Q',
            SlotState::Writing => 'W',
        })
        .collect()
}

fn capture_ring(cases: &Object) -> String {
    let mut out = Text::new();
    for (label, ring_case) in cases.value("rings").to_object().iter() {
        let ring_case = ring_case.to_array();
        let slot_total = ring_case[0].to_int(0);
        let mut ring = CaptureRingBookkeeping::new(slot_total);
        let shown = slot_total.max(1) + 1;
        out.section(&format!("{label} slotCount={slot_total}"));
        for op in ring_case[1].to_array() {
            let op = op.to_array();
            let name = op[0].to_string_or("");
            let slot = op.get(1).map_or(0, |v| v.to_int(0));
            let result = match name.as_str() {
                "acquire" => ring.acquire().to_string(),
                "markQueued" => boolean(ring.mark_queued(slot)).to_owned(),
                "takeQueued" => ring.take_queued().to_string(),
                "release" => boolean(ring.release(slot)).to_owned(),
                "state" => ring_states(&ring, slot + 1).chars().last().map(String::from).unwrap_or_default(),
                other => panic!("unknown ring operation {other}"),
            };
            out.row([
                name,
                if op.len() > 1 { slot.to_string() } else { "-".into() },
                result,
                ring_states(&ring, shown),
                ring.drops().to_string(),
                [SlotState::Available, SlotState::Capturing, SlotState::Queued, SlotState::Writing]
                    .map(|state| ring.count(state).to_string())
                    .join(" "),
            ]);
        }
    }
    out.into_string()
}

fn capture_crops(cases: &Object) -> String {
    let mut out = Text::new();
    for input in cases.value("crops").to_array() {
        let input = input.to_object();
        let size = size_from(input.value("size"));
        let stride = input.value("stride").to_int(0);
        let crop = rect_from(input.value("crop"));
        let rows = capture_crop_rows(size, stride, crop);
        let mut cells = vec![size_text(size), stride.to_string(), rect_text(&crop), rows.len().to_string()];
        let sums = rows.iter().fold((0i64, 0i64, 0i64), |(s, d, b), row| {
            (s + row.source_offset, d + row.destination_offset, b + row.bytes)
        });
        cells.push(format!("sums={}:{}:{}", sums.0, sums.1, sums.2));
        for (index, row) in rows.iter().enumerate() {
            if rows.len() > 8 && index >= 3 && index < rows.len() - 3 {
                continue;
            }
            cells.push(format!("{index}={}", row_text(row)));
        }
        out.row(cells);
    }
    out.into_string()
}

fn capture_headers(cases: &Object) -> String {
    let mut out = Text::new();
    let streams = cases.value("streamHeaders").to_object();
    for size in streams.value("sizes").to_array() {
        for fps in streams.value("fps").to_array() {
            let (size, fps) = (size_from(&size), fps.to_int(0));
            out.row([
                "stream".to_owned(),
                size_text(size),
                fps.to_string(),
                hex_bytes(&capture_stream_header(size, fps)),
            ]);
        }
    }
    let frames = cases.value("frameHeaders").to_object();
    for timestamp in frames.value("timestamps").to_array() {
        for bytes in frames.value("bytes").to_array() {
            let (timestamp, bytes) = (timestamp.to_integer(0), bytes.to_integer(0));
            out.row([
                "frame".to_owned(),
                timestamp.to_string(),
                bytes.to_string(),
                hex_bytes(&capture_frame_header(timestamp, bytes)),
            ]);
        }
    }
    out.into_string()
}

#[test]
fn ring_crop_rows_and_matroska_headers_txt() {
    let cases = cases();
    assert_no_differences(
        [
            ("screencapture/capture-ring.txt", capture_ring(&cases)),
            ("screencapture/capture-crop-rows.txt", capture_crops(&cases)),
            ("screencapture/matroska-headers.txt", capture_headers(&cases)),
        ]
        .iter()
        .filter_map(|(name, text)| difference(name, &read_expected(name), text.as_bytes()))
        .collect(),
    );
}

#[test]
fn start_txt() {
    let h = Hermetic::new("screencapture");
    let cases = cases();
    let mut out = Text::new();
    for input in cases.value("trims").to_array() {
        let input = input.to_object();
        let size = size_from(input.value("size"));
        let requested = rect_from(input.value("crop"));
        let mut crop = CaptureCrop::default();
        // The oracle set the error to "<untouched>"; only a failure sets it.
        let (ok, error) = match trim_capture_crop(size.width, size.height, size.width * 4, requested, &mut crop) {
            Ok(()) => (true, "<untouched>".to_owned()),
            Err(error) => (false, error),
        };
        out.row([
            "crop".to_owned(),
            size_text(size),
            rect_text(&requested),
            boolean(ok).to_owned(),
            quote(&error),
            rect_text(&crop.crop),
            size_text(crop.encoded_size),
            crop.crop_rows.len().to_string(),
        ]);
    }
    for input in cases.value("encoders").to_array() {
        let input = input.to_object();
        let mut environment = ScopedEnvironment::new(&h);
        environment.unset("OMAREEL_NATIVE_CONVERSION");
        environment.unset("OMAREEL_NATIVE_ENCODER");
        let conversion = input.contains("conversion").then(|| input.value("conversion").to_string_or(""));
        let encoder = input.contains("encoder").then(|| input.value("encoder").to_string_or(""));
        if let Some(value) = &conversion {
            environment.set("OMAREEL_NATIVE_CONVERSION", value);
        }
        if let Some(value) = &encoder {
            environment.set("OMAREEL_NATIVE_ENCODER", value);
        }
        let size = size_from(input.value("size"));
        let fps = input.value("fps").to_int(0);
        let probes = EncoderProbeResults {
            preferred: input.value("preferred").to_bool(false),
            gpu_conversion: input.value("gpuConversion").to_bool(false),
            cached: false,
        };
        let encoder_arguments =
            encoder_arguments_from_environment(size, fps, &probes, "/recordings/Recording.omareel/screen-video.mp4");
        out.row([
            "encoder".to_owned(),
            size_text(size),
            fps.to_string(),
            boolean(probes.preferred).to_owned(),
            boolean(probes.gpu_conversion).to_owned(),
            conversion.as_deref().map_or("<unset>".to_owned(), quote),
            encoder.as_deref().map_or("<unset>".to_owned(), quote),
            quote(&encoder_arguments.conversion),
            quote_list(&encoder_arguments.arguments),
        ]);
    }
    assert_golden("screencapture/start.txt", &out.into_string());
}

/// The oracle's `maskDriver`: the key's NVIDIA driver version, a fact of the machine, written
/// `{driver}`; then the hermetic paths put back.
fn mask_driver(h: &Hermetic, text: &str, driver: &str) -> String {
    let value = Value::array(vec![Value::String(driver.to_owned())]).to_json(Format::Compact);
    let quoted = &value[1..value.len() - 1];
    let text = text
        .replace(&format!("\"driver\": {quoted}"), "\"driver\": \"{driver}\"")
        .replace(&format!("\"driver\":{quoted}"), "\"driver\":\"{driver}\"");
    h.sanitize(&text)
}

#[test]
fn encoder_probe_txt() {
    if running_as_root() {
        eprintln!("skipped: the encoder probe's unwritable cache needs an ordinary user");
        return;
    }
    let h = Hermetic::new("screencapture");
    let scratch = h.scratch();
    let shims = std::fs::canonicalize(format!("{GOLDEN}/shims")).expect("tests/golden/shims");
    let bin = shim_directory(&scratch, "ffmpeg", &[]);
    // A copy of the ffmpeg shim with a fixed modification time stands for the installed ffmpeg,
    // whose path, time and size the key records.
    std::fs::copy(shims.join("ffmpeg"), format!("{bin}/ffmpeg")).expect("copy the ffmpeg shim");
    std::fs::copy(shims.join("replay.sh"), format!("{bin}/replay.sh")).expect("copy replay.sh");
    let ffmpeg = format!("{bin}/ffmpeg");
    set_times(&ffmpeg, 1700000000, 0);
    let cache = format!("{scratch}/cache");
    let mut environment = ScopedEnvironment::new(&h);
    environment.set("XDG_RUNTIME_DIR", &cache);
    environment.set("PATH", &bin);
    let driver = encoder_probe_key().value("driver").to_string_or("");
    let preferred_probe = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=size=256x256:rate=1",
        "-frames:v",
        "1",
        "-c:v",
        "h264_nvenc",
        "-f",
        "null",
        "-",
    ];
    let gpu_probe = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "bgra",
        "-s",
        "256x256",
        "-r",
        "1",
        "-i",
        "pipe:0",
        "-frames:v",
        "1",
        "-vf",
        "hwupload_cuda,scale_cuda=format=nv12",
        "-c:v",
        "h264_nvenc",
        "-f",
        "null",
        "-",
    ];
    let cache_file = format!("{cache}/omareel/encoder-probe.json");
    let mut out = Text::new();
    out.row(["encoderProbeCachePath".to_owned(), quote(&h.sanitize(&encoder_probe_cache_path()))]);
    out.row([
        "encoderProbeKey".to_owned(),
        mask_driver(&h, &Value::Object(encoder_probe_key()).to_json(Format::Compact), &driver),
    ]);
    struct Scenario {
        label: &'static str,
        cache: Option<String>,
        preferred_status: i32,
        gpu_status: i32,
        mtime: i64,
        ffmpeg_on_path: bool,
        cache_writable: bool,
    }
    let cached = |preferred: Value, gpu: Value| {
        let mut object = Object::new();
        object.insert("key", encoder_probe_key());
        object.insert("preferred", preferred);
        object.insert("gpuConversion", gpu);
        Some(Value::Object(object).to_json(Format::Indented))
    };
    let matching = cached(true.into(), false.into());
    let stringy = cached("true".into(), 1.into());
    let scenario = |label, cache: Option<String>, preferred_status, gpu_status| Scenario {
        label,
        cache,
        preferred_status,
        gpu_status,
        mtime: 1700000000,
        ffmpeg_on_path: true,
        cache_writable: true,
    };
    let scenarios = [
        scenario("no cache, both probes pass", None, 0, 0),
        scenario("no cache, NVENC missing", None, 1, 1),
        scenario("no cache, no GPU conversion", None, 0, 1),
        scenario("no cache, only the conversion probe passes", None, 1, 0),
        scenario("cache matches", matching.clone(), 0, 0),
        scenario("cache with wrong types", stringy, 0, 0),
        Scenario { mtime: 1700000001, ..scenario("cache key differs (ffmpeg updated)", matching, 1, 0) },
        scenario("cache is not JSON", Some("{".into()), 0, 0),
        scenario("cache is an array", Some("[]".into()), 0, 1),
        Scenario { cache_writable: false, ..scenario("cache directory not writable", None, 0, 0) },
        Scenario { ffmpeg_on_path: false, ..scenario("ffmpeg not on PATH", None, 0, 0) },
    ];
    for scenario in &scenarios {
        let _ = std::fs::remove_dir_all(&cache);
        std::fs::create_dir_all(format!("{cache}/omareel")).expect("mkpath cache");
        if let Some(bytes) = &scenario.cache {
            write_file(&cache_file, bytes);
        }
        if !scenario.cache_writable {
            chmod(&format!("{cache}/omareel"), 0o500);
        }
        set_times(&ffmpeg, scenario.mtime, 0);
        if !scenario.ffmpeg_on_path {
            environment.set("PATH", shim_directory(&scratch, "empty", &[]));
        }
        let mut replay = Replay::new(&format!("{scratch}/replay"));
        replay.answer("ffmpeg", &preferred_probe, &Answer::status(scenario.preferred_status), 0);
        let gpu = Answer {
            status: scenario.gpu_status,
            read_stdin: true,
            err: if scenario.gpu_status != 0 {
                b"Impossible to convert between the formats\n".to_vec()
            } else {
                Vec::new()
            },
            ..Answer::default()
        };
        replay.answer("ffmpeg", &gpu_probe, &gpu, 0);
        replay.activate(&h);
        out.section(scenario.label);
        out.row([
            "key".to_owned(),
            mask_driver(&h, &Value::Object(encoder_probe_key()).to_json(Format::Compact), &driver),
        ]);
        let results = encoder_probe_results();
        out.row([
            "encoderProbeResults".to_owned(),
            boolean(results.preferred).into(),
            boolean(results.gpu_conversion).into(),
            boolean(results.cached).into(),
        ]);
        // The two probes run at once, so their calls are listed sorted.
        let mut calls = replay.calls();
        calls.sort();
        for call in &calls {
            out.row(["  call".to_owned(), quote_list(call)]);
        }
        let stdin_bytes: usize =
            (0..calls.len()).map(|index| replay.standard_input(index).map_or(0, |b| b.len())).sum();
        out.row(["  stdin bytes".to_owned(), stdin_bytes.to_string()]);
        let cache_bytes =
            if std::path::Path::new(&cache_file).is_file() { std::fs::read(&cache_file).ok() } else { None };
        out.row([
            "  cache".to_owned(),
            cache_bytes.map_or("-".into(), |bytes| {
                quote_bytes(mask_driver(&h, &String::from_utf8_lossy(&bytes), &driver).as_bytes())
            }),
            octal_mode(&cache_file),
        ]);
        out.row(["cachedPreferredEncoderAvailable".to_owned(), boolean(cached_preferred_encoder_available()).into()]);
        chmod(&format!("{cache}/omareel"), 0o700);
        if !scenario.ffmpeg_on_path {
            environment.set("PATH", &bin);
        }
    }
    let _ = std::fs::remove_dir_all(&cache);
    out.row([
        "cachedPreferredEncoderAvailable".to_owned(),
        "<no cache>".into(),
        boolean(cached_preferred_encoder_available()).into(),
    ]);
    drop(environment);
    assert_golden("screencapture/encoder-probe.txt", &out.into_string());
}

/// The slots' memory, filled by the test as the compositor would fill it.
struct TestSlots(Vec<Mutex<Vec<u8>>>);

impl SlotMemory for TestSlots {
    fn with_slot(&self, index: usize, read: &mut dyn FnMut(&[u8])) -> bool {
        read(&self.0[index].lock().unwrap_or_else(PoisonError::into_inner));
        true
    }
}

/// `ScreenCapture::Private::slotCount`.
const SLOT_COUNT: i32 = 5;

#[test]
fn writer_loop_txt() {
    let h = Hermetic::new("screencapture");
    let files = format!("{}/writer", h.scratch());
    let mut out = Text::new();
    for (label, input) in cases().value("writer").to_object().iter() {
        let input = input.to_object();
        let _ = std::fs::remove_dir_all(&files);
        std::fs::create_dir_all(&files).expect("mkpath writer");
        let size = size_from(input.value("size"));
        let stride = if input.contains("stride") { input.value("stride").to_int(0) } else { size.width * 4 };
        let fps = input.value("fps").to_int(0).max(1);
        let timestamp_name = if input.contains("timestampPath") {
            input.value("timestampPath").to_string_or("")
        } else {
            "screen.mp4.ts".into()
        };
        let timestamp_path = format!("{files}/{timestamp_name}");
        let crop = if input.contains("crop") { rect_from(input.value("crop")) } else { Rect::from_size(size) };
        let config = WriterConfig {
            fps,
            timestamp_path: timestamp_path.clone(),
            encoded_size: crop.size(),
            crop_rows: capture_crop_rows(size, stride, crop),
        };
        let stream = format!("{files}/stream.mkv");
        let encoder_fd: RawFd = if input.value("encoder").to_string_or("") == "closed" {
            -1
        } else {
            let path = std::ffi::CString::new(stream.clone()).expect("path");
            // SAFETY: a valid NUL-terminated path.
            unsafe {
                libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC, 0o644)
            }
        };
        let queue = Arc::new(FrameQueue::new(SLOT_COUNT));
        if input.value("abort").to_bool(false) {
            queue.set_writer_abort();
        }
        let slot_bytes = (stride * size.height) as usize;
        let memory = Arc::new(TestSlots((0..SLOT_COUNT).map(|_| Mutex::new(vec![0u8; slot_bytes])).collect()));
        let finished = Arc::new(AtomicBool::new(false));
        let before = realtime_now_us();
        let start_writer = || {
            let (queue, memory, config, finished) = (queue.clone(), memory.clone(), config.clone(), finished.clone());
            std::thread::spawn(move || {
                writer_loop(&queue, &config, &*memory, &mut FdSink(encoder_fd));
                finished.store(true, Ordering::SeqCst);
            })
        };
        let mut writer = None;
        // Frames are queued as captureFrame queues them: a slot taken from the ring, its pixels
        // and presentation time set, marked queued. Frame k's bytes are (16k + offset) & 0xff.
        // The first frames fill the ring before the writer starts.
        let mut queued = 0;
        let times = input.value("times").to_array();
        for time in &times {
            if writer.is_none() && queued == SLOT_COUNT {
                writer = Some(start_writer());
            }
            let mut slot = -1;
            while !finished.load(Ordering::SeqCst) {
                if queue.count(SlotState::Available) > 0 {
                    slot = queue.acquire();
                }
                if slot >= 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if slot < 0 {
                break;
            }
            {
                let mut bytes = memory.0[slot as usize].lock().unwrap_or_else(PoisonError::into_inner);
                for (offset, byte) in bytes.iter_mut().enumerate() {
                    *byte = ((16 * queued as usize + offset) & 0xff) as u8;
                }
            }
            queue.mark_queued(slot, time.to_integer(0));
            queued += 1;
        }
        let writer = writer.unwrap_or_else(start_writer);
        let stop_us = input.value("stop").to_integer(0);
        queue.set_stop_us(stop_us);
        queue.stop_writer(true);
        writer.join().expect("writer thread");
        let after = realtime_now_us();
        if encoder_fd >= 0 {
            // SAFETY: the descriptor was opened above and is closed once.
            unsafe { libc::close(encoder_fd) };
        }
        out.section(label);
        let time_list: Vec<String> = times.iter().map(|t| t.to_integer(0).to_string()).collect();
        out.row([
            "input".to_owned(),
            fps.to_string(),
            size_text(size),
            stride.to_string(),
            rect_text(&crop),
            if time_list.is_empty() { "-".into() } else { time_list.join(",") },
            stop_us.to_string(),
            queued.to_string(),
        ]);
        out.row([
            "result".to_owned(),
            format!("frames={}", queue.encoded_frames()),
            format!("firstUs={}", queue.first_frame_us()),
            format!("lastFrameIndex={}", queue.last_frame_index()),
            format!("videoDurationUs={}", queue.video_duration_us(fps)),
            format!("failed={}", boolean(queue.writer_failed())),
            format!("error={}", quote(&queue.writer_error())),
        ]);
        out.row(["ring".to_owned(), ring_states(&queue.ring(), SLOT_COUNT), queue.drops().to_string()]);
        out.row([
            "timestamp".to_owned(),
            std::fs::read(&timestamp_path).map_or("-".into(), |ts| quote_bytes(&mask_realtime(&ts, before, after))),
        ]);
        match std::fs::read(&stream) {
            Ok(bytes) => out.row(["stream".to_owned(), bytes.len().to_string(), hex_bytes(&bytes)]),
            Err(_) => out.row(["stream", "-", "-"]),
        }
    }
    let _ = std::fs::remove_dir_all(&files);
    assert_golden("screencapture/writer-loop.txt", &out.into_string());
}
