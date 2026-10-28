//! Runs the recorder's [`CursorSampler`] on one monitor until standard input closes, then prints
//! where its samples came from and the samples, as one JSON object:
//! `{"source": "session"|"polling"|"none", "seconds": s, "ipc_requests": n, "samples": [[monotonic_us, x, y], ...]}`,
//! where `ipc_requests` counts what the sampler asked of Hyprland's IPC socket.
//! `tests/tools/cursor-sampler-proof.py` drives it inside a nested Hyprland.
//!
//! Usage: `cursor_probe <monitor> [fps]`.

use omareel_record::cursor_sampler::{self, CursorSampler, CursorSource};
use std::io::Read;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let monitor = arguments.next().expect("usage: cursor_probe <monitor> [fps]");
    let fps = arguments.next().and_then(|fps| fps.parse().ok()).unwrap_or(60);
    let mut sampler = CursorSampler::new();
    if let Err(error) = sampler.start(&monitor, fps) {
        eprintln!("cursor_probe: {error}");
        std::process::exit(2);
    }
    // The thread chooses its source after a few roundtrips; say so once it has.
    for _ in 0..200 {
        if sampler.source() != CursorSource::None {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    eprintln!("cursor_probe: source {:?}", sampler.source());
    let source = match sampler.source() {
        CursorSource::Session => "session",
        CursorSource::Polling => "polling",
        CursorSource::None => "none",
    };
    let started = std::time::Instant::now();
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    sampler.stop();
    let samples: Vec<String> = sampler
        .samples()
        .iter()
        .map(|sample| format!("[{}, {}, {}]", sample.monotonic_us, sample.logical_position.x, sample.logical_position.y))
        .collect();
    println!(
        "{{\"source\": \"{source}\", \"seconds\": {}, \"ipc_requests\": {}, \"samples\": [{}]}}",
        started.elapsed().as_secs_f64(),
        cursor_sampler::ipc_request_count(),
        samples.join(", ")
    );
}
