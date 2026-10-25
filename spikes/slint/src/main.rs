//! omareel's preview, export and recording bar in Slint, over omareel_core.
//!
//!   omareel-slint-spike setup <bundle>
//!   omareel-slint-spike preview <bundle> [--upload wgpu|cpu] [--canvas-height <px>] [--measure <s>] [--stats <json>]
//!   omareel-slint-spike export <bundle> -o <out.mp4> [--quality social] [--width <px>] [--upload wgpu|cpu] [--timing] [--png <frame,...>]
//!   omareel-slint-spike bar

slint::include_modules!();

mod bar;
mod export;
mod preview;
mod scene;
mod setup;
mod video;

use scene::Upload;

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn upload(args: &[String]) -> Upload {
    match value(args, "--upload") {
        Some("cpu") => Upload::Cpu,
        _ => Upload::Wgpu,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("setup") if args.len() > 2 => setup::run(&args[2]),
        Some("export") if args.len() > 2 => export::run(export::Options {
            bundle: args[2].clone(),
            output: value(&args, "-o").unwrap_or("out.mp4").to_owned(),
            quality: value(&args, "--quality").unwrap_or("").to_owned(),
            width: value(&args, "--width").and_then(|v| v.parse().ok()).unwrap_or(0),
            fps: value(&args, "--fps").and_then(|v| v.parse().ok()).unwrap_or(0),
            upload: upload(&args),
            timing: args.iter().any(|a| a == "--timing"),
            png_frames: value(&args, "--png").map(|v| v.split(',').filter_map(|f| f.parse().ok()).collect()).unwrap_or_default(),
        }),
        Some("preview") if args.len() > 2 => preview::run(preview::Options {
            bundle: args[2].clone(),
            upload: upload(&args),
            canvas_height: value(&args, "--canvas-height").and_then(|v| v.parse().ok()).unwrap_or(620.0),
            decode_height: value(&args, "--decode-height").and_then(|v| v.parse().ok()).unwrap_or(0),
            measure: value(&args, "--measure").and_then(|v| v.parse().ok()),
            stats: value(&args, "--stats").map(str::to_owned),
        }),
        Some("bar") => bar::run(),
        _ => Err("usage: omareel-slint-spike setup|preview|export|bar ...".into()),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
