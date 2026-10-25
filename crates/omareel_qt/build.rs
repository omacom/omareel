use cxx_qt_build::{CppFile, CxxQtBuilder, MocArguments};

fn main() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let plumbing = manifest.join("../../src/plumbing");
    let cpp = manifest.join("cpp");
    println!("cargo::rerun-if-changed={}", plumbing.display());
    // The C++ compiled into this archive: the launcher's and the recording bar's base classes,
    // which call the Rust here and the plumbing below it.
    let moc = || MocArguments::default().include_path(plumbing.clone()).include_path(cpp.clone());
    let builder = CxxQtBuilder::new()
        .qt_module("Gui")
        .qt_module("Multimedia")
        .files(["src/cli.rs", "src/export.rs", "src/launcher.rs", "src/recording_bar.rs", "src/theme.rs"])
        .cpp_files([
            CppFile::from(cpp.join("LauncherBase.h")).moc_arguments(moc()),
            CppFile::from(cpp.join("LauncherBase.cpp")),
            CppFile::from(cpp.join("RecordingBarBase.h")).moc_arguments(moc()),
            CppFile::from(cpp.join("RecordingBarBase.cpp")),
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
