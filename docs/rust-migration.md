# Moving omareel's logic to Rust

omareel's program logic is moving from C++ to Rust. The QML interface and Qt Quick rendering
stay as they are. The move happens a few pull requests at a time, and the app behaves the
same after each one: same commands, same files written byte for byte, same pixels.

## Layout

| Path | What |
|---|---|
| `crates/omareel_core` | Plain Rust logic with no Qt. Any front end can use it. |
| `crates/omareel_record` | The recording side's logic with no Qt (recorder state files, region picker, capture exclusion, camera and screen capture decisions, launcher and recording bar helpers), on top of `omareel_core`. |
| `crates/omareel_qt` | The one Rust static library linked into the C++ binary: CXX-Qt objects and C++-callable functions that expose `omareel_core` and `omareel_record` to QML and C++. |
| `crates/omareel_qt/cpp` | C++ that calls into Rust and is compiled into the same archive by the crate's build script: `LauncherBase` and `RecordingBarBase`, the C++ bases of the launcher and recording bar objects. |
| `src/plumbing` | C++ that Rust drives: Qt-only pieces (players, GL export rendering, the camera, file watchers, timers). |
| `src/` (rest) | C++ that has not moved yet, plus `main.cpp`'s window setup. |

The layering only goes one way, which keeps static-library linking free of cycles:

```
omareel (exe), C++ tests       may call Rust and plumbing
  omareel_render               QML modules, FrameSource
omareel_qt (Rust staticlib)    calls plumbing through cxx
  crates/omareel_qt/cpp        C++ in the same archive: calls Rust and plumbing
  omareel_record               pure Rust, the recording side
  omareel_core                 pure Rust
omareel_plumbing (C++)         never calls Rust; only emits Qt signals and implements virtuals
```

The launcher and the recording bar are Rust objects over C++ bases that own their Qt pieces:
`LauncherBase` holds QMediaDevices, the cameras Qt lists (the `webcamCameraDevice` property hands
one to QML in a QVariant), the microphone meter's QAudioSource and the launcher's timers, and starts
the detached processes; `RecordingBarBase` holds the camera (`CameraCapture`, plumbing), the watcher
on the recorder's state directory, the bar's timers and the standby host's QLockFile, which it
releases only after the camera is gone, as `RecordingBar`'s members did. Both bases call Rust (the
camera's encoder asks the Rust encoder probe through a callback the base installs), so they are
compiled into `omareel_qt`'s own archive (`CxxQtBuilder::cpp_files`, which also runs moc on the
headers). Within one archive the linker resolves references in any direction, so nothing depends on
link order; CMake only adds `crates/omareel_qt/cpp` to the include path of what links `omareel_qt`,
and the Qt modules that C++ needs to `QT_MODULES` (they must match the build script's `qt_module`
calls). That C++ is built by the `cc` crate with the crate's flags (`-Wall -Wextra`) rather than
CMake's, so it should not lean on `Q_ASSERT` or `NDEBUG`, and its headers include `<QString>` before
any other Qt header (see the CXX-Qt notes). Plumbing itself still never calls Rust: what it needs
from Rust comes in as a `std::function`, like `CameraCapture::setPreferredEncoderQuery`.

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
- A C++ base class can own Qt objects the Rust object drives (`LauncherBase`, `RecordingBarBase`):
  Rust calls its protected methods with `#[inherit]` and it reports through pure virtuals the Rust
  object overrides. Override signatures must match exactly; a Rust `i64` is `std::int64_t`, which
  is not `qint64`. The generated class derives from the base, then `CxxQtType` (which owns the
  Rust object), then `CxxQtThreading`, so the Rust object drops before the base's members: the base
  disconnects what reports to it in its destructor, and anything that must outlive the Rust state
  (the camera before the host lock) is ordered among the base's members.
- A base method that emits synchronously (a camera that fails as it starts) calls the Rust override
  while a Rust method that called the base is still on the stack. The C++ handled those signals
  inside the call, and the state files they write must come out in the same order, so the calls
  stay direct; the Rust methods hold no borrow of their fields across such a call.
- cxx explicitly instantiates `rust::Vec<T>` for every shared struct `T` a signature mentions, and
  GCC 16 reports `-Wmaybe-uninitialized` in its constructor; lists go across as an opaque Rust
  collector C++ appends to (`CameraList`) instead.
- A C++ header compiled by the build script that includes another Qt header before `<QString>`
  makes GCC 16 warn that `QChar` was completed after a SFINAE check; `QtShims.h` (or `<QString>`)
  goes first.

- CXX-Qt generates an `Omareel::rust` namespace for the objects declared in `Omareel`, so C++
  in that namespace spells cxx's types `::rust::Box` and `::rust::Slice`.
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
