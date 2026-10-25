# Moving omareel's logic to Rust

omareel's program logic is moving from C++ to Rust. The QML interface and Qt Quick rendering
stay as they are. The move happens a few pull requests at a time, and the app behaves the
same after each one: same commands, same files written byte for byte, same pixels.

## Layout

| Path | What |
|---|---|
| `crates/omareel_core` | Plain Rust logic with no Qt. Any front end can use it. |
| `crates/omareel_qt` | The one Rust static library linked into the C++ binary: CXX-Qt objects and C++-callable functions that expose `omareel_core` to QML and C++. |
| `crates/omareel_qt/cpp` | C++ that calls into Rust and is compiled into the same archive by the crate's build script: `EditorBase` (the editor object's C++ base) and `Exporter` (the export render loop over the Rust export job). |
| `src/plumbing` | C++ that Rust drives: Qt-only pieces (the editor's players, GL export rendering, camera, file watchers). |
| `src/` (rest) | C++ that has not moved yet, plus `main.cpp`'s window setup. |

The layering only goes one way, which keeps static-library linking free of cycles:

```
omareel (exe), C++ tests       may call Rust and plumbing
  omareel_render               QML modules, FrameSource
omareel_qt (Rust staticlib)    calls plumbing through cxx
  crates/omareel_qt/cpp        C++ in the same archive: calls Rust and plumbing
  omareel_core                 pure Rust
omareel_plumbing (C++)         never calls Rust; only emits Qt signals and implements virtuals
```

The editor object needed C++ on both sides of Rust: players and timers below it, and the
exporter, which drives the Rust export job, above it. The exporter is therefore compiled into
`omareel_qt`'s own archive (`CxxQtBuilder::cpp_files`, which also runs moc on the headers), with
`EditorBase`, which starts it. Within one archive the linker resolves references in any
direction, so nothing depends on link order; CMake only adds `crates/omareel_qt/cpp` to the
include path of what links `omareel_qt`, and the Qt modules that C++ needs to `QT_MODULES` (they
must match the build script's `qt_module` calls). A CMake cycle between two static libraries, or
the Rust editor running the export loop itself, were the other options: the first depends on
CMake repeating archives on the link line, the second would have changed when the event loop
runs during an export.

## Rules for ported code

- **Port faithfully first.** Keep the C++ names (Rust style), the order of operations, rounding,
  clamping, error strings, defaults and JSON key names. Behaviour changes, including merging
  the duplicated C++ helpers, come in their own commits with reviewed golden diffs.
- **Qt behaviour is reproduced, not approximated.** `omareel_core::compat` holds the Qt rules
  omareel depends on: `QJsonDocument` output (sorted keys, 4-space indent, Qt's number
  format), `QColor` parsing and lighter/darker, `QString::number`, `QStandardPaths`, `QSaveFile`.
- **Golden fixtures are the reference.** `tools/oracle` is a C++ program built at the tag
  `rust-migration-base` that writes what the C++ code produced over a corpus of inputs into
  `tests/golden/`. Rust tests must reproduce those outputs. Once a C++ file is deleted, its
  goldens are what's left of it, so do not regenerate them from new code.
- **No C++ core changes while both exist.** A fix to logic that is being ported goes into Rust
  (and the goldens, deliberately), not into the C++ that is about to be deleted.
- **Nothing panics across FFI.** No `unwrap` in `omareel_qt`; the release profile aborts on
  panic.

## CXX-Qt notes

Everything the existing QML and tests use was confirmed in a throwaway build before this
started (CXX-Qt 0.10, Qt 6.11):

- Several properties can share one NOTIFY signal (`projectChanged`), and a NOTIFY signal can
  carry an argument (`exportFinished(QString)`).
- Custom `READ` getters with a `cxx_name` keep C++ call sites such as `editor.bundleName()`.
- Default arguments work either as two Rust methods sharing one `cxx_name` (QML picks the
  overload by argument count) or through a C++ base class whose `Q_INVOKABLE` forwards to a
  pure virtual the Rust object overrides (`#[base]` plus `#[cxx_override]`).
- `cxx_qt::Constructor<(QString,)>` lets C++ construct the object on the stack as today.
- `qt_thread().queue(...)` replaces `QtConcurrent` and `QFutureWatcher`.
- Maps for QML are built natively with cxx-qt-lib. A JSON `null` must be
  `QVariant::fromValue(nullptr)`, which QML sees as `null`; a default `QVariant` is `undefined`.
  `QColor` values stay colours.
- Values coming in from QML go through `QJsonValue::fromVariant`, the same conversion the C++
  editor uses.

- CXX-Qt generates an `Omareel::rust` namespace for the objects declared in `Omareel`, so C++
  in that namespace spells cxx's types `::rust::Box` and `::rust::Slice`.
- A C++ base class can own Qt objects the Rust object drives (`EditorBase` owns the players and
  timers): Rust calls its protected methods with `#[inherit]` and it reports through pure
  virtuals the Rust object overrides. Override signatures must match exactly; a Rust `i64` is
  `std::int64_t`, which is not `qint64`. The base disconnects its players in its destructor, since
  the Rust object (and its overrides) is gone by then.
- The generated class derives from the base, then `CxxQtType` (which owns the Rust object), then
  `CxxQtThreading`, so the threading guard is cleared before the Rust object drops: a worker that
  queues a result while `Drop` waits for it gets an error instead of a deadlock, and nothing runs
  for a destroyed object.
- A C++ function can be declared in one bridge only (cxx emits a shim symbol per declaration);
  other modules call it through a Rust wrapper. A shared struct of one bridge can be used in
  another as `type ExportOptions = crate::export::ffi::ExportOptions;`.
- cxx-qt-lib's Qt containers are trivially relocatable, so C++ can hand Rust a `QByteArray` by
  value; the export job moves each rendered frame to its encode thread that way, in a `Send`
  wrapper, without copying. Decoded frames go the other way as `QImage`s over Rust's pixels,
  whose cleanup function drops the Rust box.

## Build notes

- CMake drives cargo through Corrosion (`corrosion` on Arch). `cmake/CxxQt.cmake` is vendored
  from cxx-qt-cmake 0.10 and patched to use the system Corrosion: the upstream file asks for
  Corrosion 0.5.2 and fetches it from GitHub otherwise, and Corrosion 0.6's version file does
  not accept a 0.5 request.
- makepkg's default LTO puts GCC bytecode into the objects the `cc` crate builds inside the Rust
  static library, and the final link then fails with undefined references. The PKGBUILD turns
  LTO off.
- The PKGBUILD builds into `build-pkg/` and CI tests that same build, so what ships is what was
  tested.
- `ctest` runs the Rust crates' tests as the `rust` test. `omareel_qt` is excluded there; it only
  links inside a Qt binary and its behaviour is covered by the C++ tests through QML.
