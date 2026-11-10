//! Builds `csrc/media.c`, the editor's in-process probes and filmstrip frames, and
//! `csrc/ffmpeg_dl.c`, which opens the system's FFmpeg libraries the first time they are needed:
//! only their headers are found here (pkg-config), they are not linked (see ffmpeg_dl.h).

fn main() {
    for file in ["csrc/media.c", "csrc/media.h", "csrc/ffmpeg_dl.c", "csrc/ffmpeg_dl.h"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let mut build = cc::Build::new();
    build.file("csrc/media.c").file("csrc/ffmpeg_dl.c").std("c11").warnings(true).extra_warnings(true);
    for library in ["libavcodec", "libavformat", "libavfilter", "libavutil", "libswscale"] {
        let found = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe(library)
            .unwrap_or_else(|error| panic!("the editor's media probes need {library}'s headers: {error}"));
        for path in found.include_paths {
            build.include(path);
        }
    }
    build.compile("omareel_media");
}
