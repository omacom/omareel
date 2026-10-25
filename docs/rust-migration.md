# Moving omareel's logic to Rust

omareel's program logic is moving from C++ to Rust. The QML interface and Qt Quick rendering
stay as they are. The move happens a few pull requests at a time, and the app behaves the
same after each one: same commands, same files written byte for byte, same pixels.

## Layout

| Path | What |
|---|---|
| `crates/omareel_core` | Plain Rust logic with no Qt. Any front end can use it. |
| `crates/omareel_record` | The recording side with no Qt, on top of `omareel_core`: the recorder daemon and `omareel record`, the in-process Wayland screen capture, input and cursor sampling, the state files, region picker and capture exclusion, and the camera, launcher and recording bar helpers. |
| `crates/omareel_qt` | The one Rust static library linked into the C++ binary: CXX-Qt objects and C++-callable functions that expose `omareel_core` to QML and C++. |
| `src/plumbing` | C++ that Rust drives: Qt-only pieces (players, GL export rendering, camera, file watchers, timers). |
| `src/` (rest) | C++ that has not moved yet, plus `main.cpp`'s window setup. |

The layering only goes one way, which keeps static-library linking free of cycles:

```
omareel (exe), C++ tests       may call Rust and plumbing
  omareel_render               QML modules, FrameSource
omareel_qt (Rust staticlib)    calls plumbing through cxx
  omareel_record               pure Rust, the recording side
  omareel_core                 pure Rust
omareel_plumbing (C++)         never calls Rust; only emits Qt signals and implements virtuals
```

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
- cxx-qt-lib's Qt containers are trivially relocatable, so C++ can hand Rust a `QByteArray` by
  value; the export job moves each rendered frame to its encode thread that way, in a `Send`
  wrapper, without copying. Decoded frames go the other way as `QImage`s over Rust's pixels,
  whose cleanup function drops the Rust box.

## Recording runtime notes

- `main.cpp` still dispatches the recording commands where it did, after the `QGuiApplication`
  exists (so the daemon's environment and Qt's own start-up stay as they were), and hands them to
  `omareel_qt::record`. `__record-gsr` stays first in `main()`: it only `execvp`s
  gpu-screen-recorder with that argv[0], which Omarchy's REC indicator matches.
- The screen capture has its own Wayland connection, as the C++ had: `wayland-client` with its
  pure-Rust backend, connected the way `wl_display_connect(NULL)` connects, and the
  `ext-image-copy-capture-v1` protocols from `wayland-protocols` (the same XML as
  `/usr/share/wayland-protocols`). The encoder is spawned with `posix_spawnp` as before.
- Input devices are read through libevdev itself (a few `extern "C"` declarations), so key names
  are libevdev's. CMake links libevdev to `omareel_qt` for it.
- `compat::qprocess::start_detached` is `QProcess::startDetached`: a grandchild in a session of
  its own, made with `CLONE_VFORK` children as Qt's vfork does; with plain `fork` every
  detached start of the daemon or a notification copied the process's page tables and
  `omareel record` took a few milliseconds longer to its first frame.
- The daemon's warnings go through `Omareel::Shims::warning` (`qWarning`), so they reach the
  journal as they did when there is no terminal.
- `src/record/Recorder.cpp` and `CursorSampler.cpp` are thin C++ wrappers over the Rust state
  layer and cursor query for the launcher, the recording bar and its windows, until those move;
  state file objects cross the bridge as JSON text.
- `tools/parity/recording/recording-parity.py` records the same sessions with two binaries in
  the nested compositor (`tests/tools/nested-hyprland.sh`) and compares what each did: CLI
  output, state files, bundles, the argv of every program it ran, notifications and journal
  entries, and with `--latency` the time to the first frame.

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
