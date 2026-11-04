//! Runs several screen captures of one output at once, to measure what capturing costs the
//! compositor: `capture_bench MONITOR COUNT SECONDS OUTPUT_DIR [FPS [X Y WIDTH HEIGHT]]`, the last
//! four a region of the output in pixels. `OMAREEL_CAPTURE_BUFFERS=shm`
//! forces shared memory; otherwise the capture picks dmabufs as a recording would. Prints each
//! capture's frames, drops and conversion.

use omareel_record::screen_capture::{Rect, ScreenCapture, ScreenCaptureConfig};
use std::time::{Duration, Instant};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() < 5 {
        eprintln!("usage: capture_bench MONITOR COUNT SECONDS OUTPUT_DIR [FPS [X Y WIDTH HEIGHT]]");
        std::process::exit(2);
    }
    let monitor = arguments[1].clone();
    let count: usize = arguments[2].parse().expect("a count");
    let seconds: u64 = arguments[3].parse().expect("seconds");
    let directory = arguments[4].clone();
    let fps: i32 = arguments.get(5).map_or(60, |fps| fps.parse().expect("an fps"));
    let region: Vec<i32> = arguments.iter().skip(6).map(|value| value.parse().expect("a number")).collect();
    let crop = if region.len() == 4 { Rect::new(region[0], region[1], region[2], region[3]) } else { Rect::default() };
    let threads: Vec<_> = (0..count)
        .map(|index| {
            let (monitor, directory) = (monitor.clone(), directory.clone());
            std::thread::spawn(move || {
                let mut capture = ScreenCapture::new();
                let config = ScreenCaptureConfig {
                    monitor,
                    output_path: format!("{directory}/bench-{index}.mp4"),
                    timestamp_path: format!("{directory}/bench-{index}.mp4.ts"),
                    crop,
                    fps,
                };
                let mut error = String::new();
                if !capture.start(&config, &mut error) {
                    return format!("capture {index}: start failed: {error}");
                }
                let started = Instant::now();
                while started.elapsed() < Duration::from_secs(seconds) {
                    if !capture.capture_frame(&mut error) {
                        return format!("capture {index}: {error}");
                    }
                }
                let finished = capture.finish(&mut error);
                format!(
                    "capture {index}: conversion={} frames={} drops={} finished={finished} {error}",
                    capture.conversion_mode(),
                    capture.encoded_frames(),
                    capture.dropped_frames()
                )
            })
        })
        .collect();
    for thread in threads {
        println!("{}", thread.join().unwrap_or_else(|_| "capture panicked".into()));
    }
}
