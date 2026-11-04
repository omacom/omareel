//! Builds `csrc/gpu_capture.c`, the screen capture's GBM and VA-API side, against the system's
//! libraries (found with pkg-config, which also links them).

fn main() {
    println!("cargo:rerun-if-changed=csrc/gpu_capture.c");
    println!("cargo:rerun-if-changed=csrc/gpu_capture.h");
    let mut build = cc::Build::new();
    build.file("csrc/gpu_capture.c").std("c11").warnings(true).extra_warnings(true);
    for library in ["gbm", "libdrm", "libva", "libavcodec", "libavformat", "libavfilter", "libavutil"] {
        let found = pkg_config::Config::new()
            .probe(library)
            .unwrap_or_else(|error| panic!("the screen capture needs {library}: {error}"));
        for path in found.include_paths {
            build.include(path);
        }
    }
    build.compile("omareel_gpu_capture");
}
