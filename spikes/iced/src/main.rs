//! omareel's hard UI parts in iced: the editor's preview, the offscreen export and the
//! recording bar, over omareel_core. See spikes/SPEC.md and REPORT.md.

mod bar;
mod export;
mod preview;
mod scene;
mod setup;
mod video;

fn usage() -> ! {
    eprintln!(
        "usage: omareel-iced-spike <command>\n\n\
         setup <bundle>                          finish a make-bundle.sh bundle (project.json, key presses)\n\
         preview <bundle> [--measure SECONDS] [--stats FILE] [--canvas-height PX]\n\
         export <bundle> -o <out.mp4> [--quality Q] [--timing] [--png-at T,T,...]\n\
         bar                                     the recording bar (layer shell)"
    );
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else { usage() };
    let rest = &args[1..];
    let result = match command.as_str() {
        "setup" => match rest {
            [bundle] => setup::run(bundle),
            _ => usage(),
        },
        "preview" => {
            let Some(bundle) = rest.first() else { usage() };
            let mut options = preview::Options { bundle: bundle.clone(), measure: None, stats: None, canvas_height: None };
            let mut i = 1;
            while i < rest.len() {
                match (rest[i].as_str(), rest.get(i + 1)) {
                    ("--measure", Some(v)) => options.measure = v.parse().ok(),
                    ("--stats", Some(v)) => options.stats = Some(v.clone()),
                    ("--canvas-height", Some(v)) => options.canvas_height = v.parse().ok(),
                    _ => usage(),
                }
                i += 2;
            }
            preview::run(options)
        }
        "export" => {
            let Some(bundle) = rest.first() else { usage() };
            let mut options = export::Options {
                bundle: bundle.clone(),
                output: String::new(),
                quality: "social".into(),
                timing: false,
                png_at: Vec::new(),
            };
            let mut i = 1;
            while i < rest.len() {
                match (rest[i].as_str(), rest.get(i + 1)) {
                    ("-o", Some(v)) => {
                        options.output = v.clone();
                        i += 1;
                    }
                    ("--quality", Some(v)) => {
                        options.quality = v.clone();
                        i += 1;
                    }
                    ("--png-at", Some(v)) => {
                        options.png_at = v.split(',').filter_map(|t| t.parse().ok()).collect();
                        i += 1;
                    }
                    ("--timing", _) => options.timing = true,
                    _ => usage(),
                }
                i += 1;
            }
            if options.output.is_empty() {
                usage();
            }
            export::run(&options)
        }
        "bar" => bar::run(),
        _ => usage(),
    };
    if let Err(error) = result {
        eprintln!("omareel-iced-spike: {error}");
        std::process::exit(1);
    }
}
