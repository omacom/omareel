//! omareel's preview, export and recording bar in Rune, over omareel_core.
//!
//!   omareel-rune-spike setup <bundle>
//!   omareel-rune-spike preview <bundle> [--upload surface|pixels] [--decode vaapi|software] [--canvas-height <pt>]
//!                                       [--decode-height <px>] [--measure <s>] [--stats <json>]
//!   omareel-rune-spike export <bundle> -o <out.mp4> [--quality social] [--width <px>] [--timing] [--png <frame,...>]
//!   omareel-rune-spike editor <bundle> [preview flags] [-- <Rune flags: --snap out.png, --script s.json --video out.mp4, --tree t.json>]
//!   omareel-rune-spike bar

mod bar;
mod comp;
mod editor;
mod export;
mod preview;
mod setup;
mod theme;
mod video;

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
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
            timing: args.iter().any(|a| a == "--timing"),
            gpu_timing: args.iter().any(|a| a == "--gpu-timing"),
            png_frames: value(&args, "--png").map(|v| v.split(',').filter_map(|f| f.parse().ok()).collect()).unwrap_or_default(),
        }),
        Some(mode @ ("preview" | "editor")) if args.len() > 2 => preview::run(preview::Options {
            editor: mode == "editor",
            // Rune's own flags follow `--`: `--snap out.png`, `--script s.json --video out.mp4`, ...
            rune_args: args.iter().position(|a| a == "--").map(|i| args[i + 1..].to_vec()).unwrap_or_default(),
            bundle: args[2].clone(),
            upload: match value(&args, "--upload") {
                Some("pixels") => video::Upload::Pixels,
                _ => video::Upload::Surface,
            },
            decode: match value(&args, "--decode") {
                Some("software") => video::Decode::Software,
                Some("vaapi") => omareel_core::export::hardware::vaapi_device().map_or(video::Decode::Software, video::Decode::Vaapi),
                _ => video::Decode::auto(),
            },
            canvas_height: value(&args, "--canvas-height").and_then(|v| v.parse().ok()).unwrap_or(620.0),
            decode_height: value(&args, "--decode-height").and_then(|v| v.parse().ok()).unwrap_or(0),
            measure: value(&args, "--measure").and_then(|v| v.parse().ok()),
            stats: value(&args, "--stats").map(str::to_owned),
        }),
        Some("bar") => bar::run(),
        _ => Err("usage: omareel-rune-spike setup|preview|export|bar ...".into()),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
