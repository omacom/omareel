use cxx_qt_build::{CppFile, CxxQtBuilder, MocArguments};

fn main() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let plumbing = manifest.join("../../src/plumbing");
    let cpp = manifest.join("cpp");
    println!("cargo::rerun-if-changed={}", plumbing.display());
    // The C++ compiled into this archive: the editor's base class and the exporter it starts,
    // which call the Rust here and the plumbing below it.
    let moc = || MocArguments::default().include_path(plumbing.clone()).include_path(cpp.clone());
    let builder = CxxQtBuilder::new()
        .qt_module("Gui")
        .qt_module("Multimedia")
        .qt_module("Quick")
        .files(["src/cli.rs", "src/editor.rs", "src/export.rs", "src/theme.rs"])
        .cpp_files([
            CppFile::from(cpp.join("EditorBase.h")).moc_arguments(moc()),
            CppFile::from(cpp.join("EditorBase.cpp")),
            CppFile::from(cpp.join("Exporter.h")).moc_arguments(moc()),
            CppFile::from(cpp.join("Exporter.cpp")),
        ]);
    // Safety: only adds the include paths of the C++ plumbing headers and of the C++ here.
    let builder = unsafe {
        builder.cc_builder(move |cc| {
            cc.include(&plumbing);
            cc.include(&cpp);
        })
    };
    builder.build();
}
