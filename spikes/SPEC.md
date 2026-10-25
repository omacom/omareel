# GPU UI spikes: GPUI, iced, Slint

The question: could a Rust GPU toolkit replace omareel's QML front end? Each spike builds the
same small program in one toolkit, so the three can be compared with each other and with the
current Qt Quick build. Qt Quick already renders on the GPU; what matters is whether a toolkit
can do omareel's hard parts as well, how fast, and how pleasant it is to write (most of
omareel's code is written with AI agents, so how much the compiler catches matters).

Each spike lives in `spikes/<toolkit>/`, its own cargo workspace (not part of the main build),
depending on `../../crates/omareel_core` by path for the real logic. Build with
`CARGO_TARGET_DIR=~/.cache/omareel-spikes/targets/<toolkit>`; nothing big goes in `/tmp`.

## The program

1. **Input**: a real recording bundle. `make-bundle.sh` (shared, in `spikes/`) makes one with
   ffmpeg: `screen.mp4` (testsrc2, 1920x1080 and a 3840x2160 variant, 60 fps, 10 s),
   `camera.mp4` (640x480, 30 fps), `tests/fixtures/synthetic/input.jsonl` and `capture.json`,
   and a `project.json` with zooms, a round camera bubble with border and 45° shadow, a
   wallpaper background, and one clip with the camera's "square + blur" full-frame layout.
2. **Logic**: open it with `omareel_core::editor::Document` and take the per-frame state from
   it (zoom, cursor, layout, camera visibility and placement, keystroke pills) exactly as the
   editor's preview does. No logic of your own beyond drawing.
3. **Decoding**: screen and camera frames from ffmpeg as RGBA (`omareel_core::export`'s decoder,
   or the same ffmpeg command), uploaded to the GPU each frame.
4. **The scene**, matching `src/ui/qml/Composition.qml` and friends closely enough to judge:
   wallpaper background; the screen frame with rounded corners, padding and a soft shadow; the
   zoom (scale and pan with the spring the core computes); the cursor; the webcam bubble (round,
   border, 45° shadow); a Gaussian-blurred copy of the camera behind a centred square for the
   full-frame layout; a keystroke pill with text.
5. **Preview window**: plays at 60 fps with play/pause, a scrub bar and a time label. Measure
   with the 1080p and the 4K screen: frame time (median, p95, max), dropped frames over 10 s,
   CPU % and RSS of the process, and GPU % if available (`nvtop`/`intel_gpu_top` if present).
6. **Offscreen export**: render the same scene without a window at 1920x1080, read it back and
   pipe RGBA to ffmpeg (libx264, the same arguments `omareel export` uses for quality "social"),
   and measure fps against `omareel export` of the same bundle with the same settings (build
   omareel from this branch: `cmake -S . -B ~/.cache/omareel-spikes/targets/omareel -G Ninja
   -DCMAKE_BUILD_TYPE=Release -DCorrosion_DIR=/tmp/rust-deps/corrosion-root/usr/lib/cmake/Corrosion
   && ninja -C ...`).
7. **Layer-shell bar**: a small always-on-top pill (elapsed time and a stop button) as a
   wlr-layer-shell surface anchored to the top of the screen, as the recording bar is.

Run windowed tests in a nested Hyprland (`tests/tools/nested-hyprland.sh`, see
`nested-hyprland.py`) where possible rather than on the user's desktop; note where a number
needed the real display.

## The report: `spikes/<toolkit>/REPORT.md`

- What worked, what didn't, and every fork, patch, unstable feature or workaround it took.
- The numbers from 5 and 6, next to the Qt build's on the same machine (Intel Xe, foundry2).
- Code size (lines) for each part, build time (clean and incremental), binary size.
- Writing it: how the toolkit's API, docs and compile errors helped or hindered an agent;
  what broke at runtime that the compiler didn't catch.
- How to run it on n1x (aarch64, NVIDIA), which the user will do.
- A verdict for omareel.

Commit everything (code, scripts, report) on branch `ui-spikes`; no build output.
