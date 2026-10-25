//! The GPUI spike (spikes/SPEC.md): omareel's composition drawn with GPUI over omareel_core.
//!
//!   omareel-gpui setup <bundle>                         write the spike's project.json
//!   omareel-gpui preview <bundle> [--bench S] [--fill]  play it in a window
//!   omareel-gpui export <bundle> -o <out.mp4> [--quality social]
//!   omareel-gpui bar                                    the layer-shell recording bar

mod bar;
mod export;
mod media;
mod preview;
mod scene;
mod setup;

fn usage() -> ! {
    eprintln!(
        "usage: omareel-gpui setup <bundle>\n       omareel-gpui preview <bundle> [--bench SECONDS] [--fill]\n       \
         omareel-gpui export <bundle> -o <out.mp4> [--quality social]\n       omareel-gpui bar"
    );
    std::process::exit(2);
}

fn main() {
    let launched = std::time::Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("setup") => {
            let bundle = args.get(1).unwrap_or_else(|| usage());
            if let Err(error) = setup::run(bundle) {
                eprintln!("omareel-gpui: {error}");
                std::process::exit(1);
            }
        }
        Some("preview") => preview::run(preview::Options {
            bundle: args.get(1).cloned().unwrap_or_else(|| usage()),
            bench: flag("--bench").and_then(|s| s.parse().ok()),
            max_height: if args.iter().any(|a| a == "--fill") { 0.0 } else { 620.0 },
            launched,
            decode_height: flag("--decode-height").and_then(|s| s.parse().ok()).unwrap_or(0),
        }),
        Some("export") => {
            let bundle = args.get(1).unwrap_or_else(|| usage());
            let output = flag("-o").unwrap_or_else(|| usage());
            let quality = flag("--quality").unwrap_or_else(|| "social".into());
            if let Err(error) = export::run(bundle, &output, &quality) {
                eprintln!("omareel-gpui: {error}");
                std::process::exit(1);
            }
        }
        Some("bar") => bar::run(),
        _ => usage(),
    }
}
