//! Builds `csrc/media.c`, the editor's in-process probes and filmstrip frames, against the
//! system's FFmpeg libraries (found with pkg-config, which also links them).

fn main() {
    println!("cargo:rerun-if-changed=csrc/media.c");
    println!("cargo:rerun-if-changed=csrc/media.h");
    let mut build = cc::Build::new();
    build.file("csrc/media.c").std("c11").warnings(true).extra_warnings(true);
    for library in ["libavcodec", "libavformat", "libavutil", "libswscale"] {
        let found = pkg_config::Config::new()
            .probe(library)
            .unwrap_or_else(|error| panic!("the editor's media probes need {library}: {error}"));
        for path in found.include_paths {
            build.include(path);
        }
    }
    build.compile("omareel_media");
}
