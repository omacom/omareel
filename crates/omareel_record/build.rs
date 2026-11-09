//! Builds `csrc/gpu_capture.c`, the screen capture's GBM and VA-API side, against the system's
//! libraries (found with pkg-config, which also links GBM, libdrm and libva). FFmpeg's it calls
//! through omareel_core's table (omareel_core/csrc/ffmpeg_dl.h), so only its headers are found.

fn main() {
    println!("cargo:rerun-if-changed=csrc/gpu_capture.c");
    println!("cargo:rerun-if-changed=csrc/gpu_capture.h");
    println!("cargo:rerun-if-changed=../omareel_core/csrc/ffmpeg_dl.h");
    let mut build = cc::Build::new();
    build.file("csrc/gpu_capture.c").std("c11").warnings(true).extra_warnings(true).include("../omareel_core/csrc");
    for library in ["gbm", "libdrm", "libva"] {
        let found = pkg_config::Config::new()
            .probe(library)
            .unwrap_or_else(|error| panic!("the screen capture needs {library}: {error}"));
        for path in found.include_paths {
            build.include(path);
        }
    }
    for library in ["libavcodec", "libavformat", "libavfilter", "libavutil", "libswscale"] {
        let found = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe(library)
            .unwrap_or_else(|error| panic!("the screen capture needs {library}'s headers: {error}"));
        for path in found.include_paths {
            build.include(path);
        }
    }
    build.compile("omareel_gpu_capture");
}
