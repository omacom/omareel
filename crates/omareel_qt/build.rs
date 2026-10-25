use cxx_qt_build::CxxQtBuilder;

fn main() {
    let plumbing = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/plumbing");
    println!("cargo::rerun-if-changed={}", plumbing.display());
    let builder = CxxQtBuilder::new().qt_module("Gui").files(["src/cli.rs", "src/theme.rs"]);
    // Safety: only adds the include path of the C++ plumbing headers the bridges name.
    let builder = unsafe {
        builder.cc_builder(move |cc| {
            cc.include(&plumbing);
        })
    };
    builder.build();
}
