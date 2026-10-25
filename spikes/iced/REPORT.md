# iced spike: report

omareel's preview, offscreen export and recording bar built with **iced 0.14.0** (the latest
release; nothing from git main was needed) over `omareel_core`, measured against the Qt build of
this branch on foundry2 (Intel Iris Xe, i915, Mesa 26.2.2, Qt 6.11.2, Hyprland 0.56.2).

**In short:** everything in SPEC.md works without a fork, a patch or an unstable feature; the
only workaround in the build is a `Cargo.lock` pin. The preview holds 60 fps at 1080p and 4K
with fewer hitches than the Qt editor, the export is 1.3x faster than `omareel export` at 1080p
(1.08x from the 4K bundle), and the frames match Qt's (PSNR 41.4 dB, SSIM 0.995 between the two
encodes). The price is that iced gives the composition no help: the scene that is 681 lines of
QML is 1,645 lines of Rust and WGSL here, with MultiEffect's shadow and blur, rounded-corner
clipping and text rasterisation written by hand, and video decoding runs on the CPU (2 cores at
4K) because wgpu has no safe way to import VAAPI/NVDEC frames.

## What was built

| Part | Files | Lines | Qt counterpart (lines) |
|---|---|---|---|
| Scene renderer, shared by preview and export | `src/scene/` (`mod.rs`, `composition.rs`, `text.rs`, `scene.wgsl`, `blur.wgsl`) | 1,645 | Composition, RoundedFrame, CameraOverlay, BlurredBackdrop, KeystrokeOverlay, Background, CursorOverlay QML (681) + FrameSource (156) |
| Preview window | `src/preview.rs`, `src/video.rs` (decoding) | 736 | PreviewPane.qml (295, more features) + QtMultimedia playback |
| Offscreen export | `src/export.rs` | 233 | Exporter.cpp + ExportRenderer (533) |
| Layer-shell bar | `src/bar.rs` | 125 | RecordingBar.qml (412, many more features) |
| Bundle setup, CLI | `src/setup.rs`, `src/main.rs` | 207 | (spike only) |
| Measurement | `scripts/` | 433 | |

- **Logic**: `Document::open`, then the motion track built once (`motion_job().run()`), then per
  frame `set_output_position` and `preview_composition` exactly as the CXX-Qt editor reads `comp`.
  The export runs inside `omareel_core::export::ExportJob` (decode thread, per-frame
  `CompositionState`, ffmpeg encoder with `omareel export`'s arguments), so the only thing the spike
  supplies is the renderer. No logic of the spike's own beyond Composition.qml's bindings.
- **Scene** (`src/scene/composition.rs` is Composition.qml and friends binding for binding):
  wallpaper (PreserveAspectCrop), the frame with padding, rounded corners, black shadow shape and
  MultiEffect drop shadow (closed-form Gaussian of a rounded rectangle), the zoom as an affine
  transform about the core's centre, cursor (the SVG rasterised by resvg at QML's 4x
  `sourceSize`) and click ripples, the camera bubble (round, border over the black shape, 45°
  shadow, scale and opacity with `presence`, shrink while zoomed) morphing into the full-frame
  square, the blurred camera copy behind it (captured into 24x24 like the QML, blurred with a
  separable Gaussian, saturation 1.1, darkened), and keystroke pills with keys and "+" laid out as
  KeystrokeOverlay does, text by cosmic-text in the theme's monospace (`fc-match`), DemiBold.
  Everything is one instanced draw of a single uber-shader after three tiny blur passes.
- **Not drawn** (not in the bundle): inserted media clips (MediaLayer), gradient and blurred
  backgrounds, zoom motion blur (off by default), the "dot" cursor is drawn but untested.
- **Preview**: iced's `shader` widget with the renderer as a `shader::Pipeline`; play/pause,
  scrub slider, time label, and space / arrows / Home (driven by `scripts/seek-test.sh` through
  `wtype` in the nested compositor: seeks, play, pause and the jump back all land on the right
  frames). Decoding: one thread per video runs `FfmpegDecoder`'s command (CUDA first, software
  fallback) and reads each RGBA frame straight into a mapped wgpu staging buffer; the UI thread
  only records a buffer-to-texture copy.
- **Export**: the same renderer on a headless wgpu device (no iced involved), 1920x1080
  Rgba8Unorm target, two readback buffers in flight like the Qt exporter, frames handed to
  `ExportJob::encode`. `--png-at` dumps frames for comparison.
- **Bar**: `iced_layershell` 0.19.1: 200x48 overlay-layer surface anchored top, no keyboard
  interactivity, transparent around a pill with a pulsing dot, elapsed time, REC and a stop
  button that exits (not clicked in the tests: the nested compositor takes no pointer injection,
  only `wtype`'s keys). Confirmed in the nested Hyprland with `hyprctl layers`
  (`overlay: xywh 860 8 200 48, namespace omareel-recording-bar`) and a screenshot. 1 % CPU,
  72 MB RSS.

## Numbers

Method: both apps run in a nested Hyprland (`tests/tools/nested-hyprland.sh`) whose own output
is replaced by a **1920x1080@60 headless output**, because the nested compositor's window output
follows the real display, which here runs at 3840x2160@30 Hz (`scripts/in-nested.sh`). The Qt
build is `omareel edit` with `OMAREEL_PREVIEW_STATS=1` (it autoplays) showing the full editor;
the spike is the bare preview with its canvas capped to the 1102x620 pixels the Qt editor gives
the composition (`--canvas-height 620`). Frame times come from the timestamps of the window
surface's `wl_surface.commit` in `WAYLAND_DEBUG=client` output, the same measurement for EGL (Qt)
and Vulkan (wgpu) (`scripts/frames.py`; Qt's `QSG_RENDER_TIMING` printed nothing). CPU, RSS and
GPU come from `/proc` and DRM fdinfo over the 10 s of playback (`scripts/sample.py`). Each cell is
the median of 3 runs with the range in brackets; every run waited for the machine to be idle
(`scripts/quiet.py`) because other agents were building and benchmarking on it, and the 1-minute
load average at the end of each run is in the tables. The tables are the last set of runs, taken
between 10:50 and 11:20 with the machine otherwise idle (load 0.2–5); an earlier set taken while
other agents were building (load up to 23) showed the same picture with more Qt hitches, and every
Qt run in the tables played 8–10 s without its editor being killed. Raw per-run JSON and export logs are in
`results/`; `scripts/measure.sh` reproduces all of it.

### Preview, 10 s of playback at 60 Hz

| | Qt 1080p | iced 1080p | Qt 4K | iced 4K |
|---|---|---|---|---|
| presented fps | 59.7 (59.7–59.8) | 59.6 (59.5–59.6) | 59.5 (59.4–59.6) | 59.6 (59.6–59.6) |
| frame time median (ms) | 16.74 | 16.77 | 16.74 | 16.77 |
| frame time p95 (ms) | 16.90 (16.89–16.94) | 17.13 (17.11–17.15) | 17.01 (17.01–17.10) | 17.09 (17.06–17.16) |
| frame time max (ms) | 19.2 (17.4–30.9) | 17.7 (17.6–18.0) | 30.6 (30.4–37.7) | 18.2 (17.6–33.0) |
| missed vsyncs (gap > 25 ms) | 0 (0–1) | 0 | 4 (3–5) | 0 (0–1) |
| dropped frames, app's own count | 10 (1–11) | 5 (4–6) | 32 (32–95) | 4 (4–5) |
| CPU % of a core, process | 15 (15–16) | 14 (13–14) | 20 (20–21) | 47 (44–49) |
| CPU %, ffmpeg decoder children | – | 52 (51–54) | – | 202 (196–218) |
| RSS peak, process (MB) | 426 | 75 | 649 | 75 |
| RSS peak with children (MB) | 426 | 665 | 649 | 1,772 |
| GPU buffers resident (MB, fdinfo) | 427 | 171 | 714 | 353 |
| GPU render engine busy % | 12 (11–12) | 19 (18–19) | 10 (9–10) | 23 (20–27) |
| GPU video engine busy % (VAAPI) | 27 | 0 | 68 | 0 |
| load average at end | 3.4 (2.5–4.8) | 2.5 (0.2–3.1) | 3.5 (3.2–4.5) | 2.8 (1.0–4.1) |

Reading it:
- Both present at the display rate. The spike's frame pacing is tighter (no frame over 18 ms in
  most runs); the Qt editor has 30 ms hitches at 4K and drops 32–95 of 600 frames by its own count
  there, while VAAPI is 68 % busy. The spike's "dropped" is recording frames never presented,
  about 2 of which come from the headless output running at 59.8 Hz against a 60.0 Hz playhead.
- The spike decodes in software: 0.5 core at 1080p, 2 cores at 4K, and ffmpeg's buffers are most
  of its memory. Qt decodes on the GPU's video engine (VAAPI).
- The spike's own process is far smaller (75 MB RSS, 171–353 MB of GPU buffers: the textures plus
  8 staging buffers per video) than the Qt editor (426–649 MB RSS plus 427–714 MB of GPU buffers),
  but the Qt number is the whole editor (timeline thumbnails, panels, wallpaper picker).
- GPU render time: the spike's scene costs about 10 % of the GPU at 60 fps (measured by switching
  it off with `OMAREEL_SPIKE_NO_SCENE`: 19 % → 7–10 %), about 1.6 ms a frame on the Iris Xe for a
  1102x620 canvas. It draws every layer whole (wallpaper, black shape, shadow, video under a 2x
  zoom are each full-canvas), where Qt Quick's batch renderer culls opaque overdraw with a depth
  pass. The first version read each item's 144 bytes from a storage buffer in every fragment and
  cost 33 %; passing them as flat varyings fixed that.
- Before the staging-buffer upload, the 4K preview ran at 48 fps with 121 drops:
  `Queue::write_texture` cost the UI thread 8 ms a frame copying 33 MB into freshly faulted
  staging memory (results of that version are not in the tables).

### Export, 600 frames to 1920x1080 MP4, libx264 "social" (both: `ExportJob`'s encoder arguments)

| | Qt 1080p bundle | iced 1080p bundle | Qt 4K bundle | iced 4K bundle |
|---|---|---|---|---|
| frames/s, wall clock incl. startup | 90.3 (90.3–90.6) | **119.8** (119.6–122.3) | 51.5 (51.3–51.8) | **55.8** (50.9–57.2) |
| seconds | 6.65 | 5.01 | 11.66 | 10.76 |
| decode wait (ms/frame) | 0.87 | 1.24 | 4.45 | 9.83 |
| upload (ms/frame) | 0.38 | 1.87 | 0.46 | 4.31 |
| render (ms/frame, CPU side) | 6.58 | 0.45 | 10.62 | 0.36 |
| readback (ms/frame) | 1.32 | 1.83 | 1.39 | 2.25 |
| encode write (ms/frame) | 6.00 | 6.26 | 7.14 | 6.82 |
| load average at end | 4.3 | 3.1 | 4.7 | 3.7 |

The stage timings are `ExportJob::timing_report`, which both exporters print with `--timing`.
Qt spends 6.6–10.6 ms a frame in QQuickRenderControl (polish, sync, render); the spike's render is
0.4 ms of CPU to build and submit. From the 4K bundle both are bound by ffmpeg decoding and
scaling 4K to RGBA. The spike's upload still goes through `write_texture` on the main thread (the
staging path is only in the preview); doing the same there would take another 2–4 ms a frame off.

**Fidelity**: PSNR between the Qt and iced exports of the same bundle is 41.4 dB (min 36.3 dB at
the camera-to-screen transition), SSIM 0.995 (1080p bundle; 40.0 dB / 0.993 from the 4K one),
both encodes being lossy at the same bitrate. Side by side the frames are indistinguishable at
normal size; up close, the blurred camera backdrop is a touch softer in the spike (its capture is
box-filtered, Qt's samples bilinearly) and the key labels are rasterised slightly thinner than
Qt's.

### Build

| | |
|---|---|
| Clean release build (20 threads, dependencies downloaded, load ~9) | 81 s wall, 1,307 s CPU |
| Clean debug build | 35 s |
| Incremental release build after editing one file | 6.7–7.4 s |
| Incremental debug build | 1.2 s |
| `cargo check` after an edit | 0.3 s |
| Binary (release, line tables) / stripped | 142 MB / 19.8 MB (statically links wgpu, naga, cosmic-text, resvg, image, omareel_core) |
| Target directory | 2.3 GB |
| Qt `omareel` for comparison | 13.5 MB, links Qt 6 shared libraries |

## What worked, what didn't, every workaround

No fork, no patch, no nightly or unstable feature. In order of how much each mattered:

1. **`Cargo.lock` pins `winit-core` and `winit-common` to 0.31.0-beta.2.** `iced_layershell` 0.19.1
   depends on `iced_exdevtools`, which matches `winit_core::NativeKeyCode` exhaustively; beta.4 made
   that enum `#[non_exhaustive]` and cargo picks it because pre-release requirements are not
   upper-bounded. The build fails until the lock is pinned (`cargo update -p winit-common --precise
   0.31.0-beta.2`, then the same for `winit-core`). Anyone running `cargo update` breaks the build.
2. **Upload path (wgpu, not iced).** 33 MB RGBA frames at 60 fps need staging buffers the decode
   thread writes into; the obvious `Queue::write_texture` is too slow at 4K. The window's device
   and queue only exist inside `shader::Pipeline::new`, so the spike hands them to the decode
   threads through a global `OnceLock`.
3. **Video decoding is CPU-only.** Zero-copy import of VAAPI or NVDEC frames into a wgpu texture
   needs dma-buf external memory through `wgpu-hal`'s unsafe Vulkan escape hatch; nothing in
   wgpu's safe API does it, and iced has no video widget (third-party `iced_video_player` uses
   GStreamer). This is the one place a real port would need unsafe interop work.
4. **No text in custom primitives.** iced's text renderer is not reachable from a `shader`
   primitive, and the export has no iced at all, so the pills' labels are laid out and rasterised
   with cosmic-text (iced's own text engine, through `iced::advanced::graphics::text`) into an
   atlas the scene samples (130 lines). Drawing the pills as iced widgets over the shader would
   have been easier for the preview but would leave the export without them.
5. **`shader::Primitive::render` gets the clip bounds, not the bounds.** The canvas rectangle is
   computed in `prepare` and stashed in the pipeline, which works because there is one preview;
   several instances of one primitive type would have to key their state.
6. **`shader::Pipeline::new` sees no application data**, so the wallpaper and theme ride in every
   frame's primitive (as `Arc`s) and are uploaded on first `prepare`. Primitives must be `Debug`:
   `image::RgbaImage`'s `Debug` prints every pixel, so it needed a wrapper.
7. **Small iced frictions**: `iced::time::every` needs the `tokio` (or `smol`) executor, the
   default `thread-pool` has no timer; `iced::application`'s boot closure is `Fn`, so moving a
   pre-built state in takes a `RefCell<Option<_>>`; the default theme is light; `window::frames()`
   only ticks while something redraws, so a paused preview polls at 10 Hz to pick up decoded frames.
8. **Environment, not iced**: the nested compositor's 30 Hz output (headless 60 Hz output instead),
   windows opening on the wrong output (the script disables all but the headless one), the vsync
   phase making `floor(t * fps)` alternate between two frames (rounded instead).
9. **Not tried**: iced_wgpu 0.14 has a headless renderer (`renderer::Headless`, used by
   `iced_test`) that could render iced widgets offscreen, but its `screenshot` allocates a texture
   and a buffer and blocks per frame; plain wgpu was simpler and faster for the export.

## Writing it (as an agent)

- **API and docs.** The part of iced this needs is small and coherent: `shader::Program` (draw →
  primitive), `shader::Primitive` (`prepare` with device and queue, `render` with an encoder or
  `draw` into iced's pass), `shader::Pipeline` (`new`, `trim`). docs.rs lists the items with little
  prose; the reliable reference was iced's source in `~/.cargo/registry` (about 300 lines of
  `iced_widget::shader` and `iced_wgpu::primitive`). 0.14 changed these APIs from 0.13 (the
  pipeline became an associated type, `Storage` went away), so anything remembered from older
  iced, or found in blog posts, is wrong; reading the pinned source first avoided that. The Elm
  loop (update, view, subscription) maps cleanly onto `Document` as the model.
- **Compile errors** were few and precise: three in the whole spike, each a one-line fix rustc
  pointed at (a field named `seconds` not `time` on omareel_core's `CameraTime`, `&f64` for `f64`, a
  missing `Debug` for a primitive field), plus an unused-`mut` warning. Generic iced signatures
  (`Element<'_, Message, Theme, Renderer>`) never produced the long errors they are known for here.
- **What broke at runtime that the compiler didn't catch**:
  - A WGSL uniform struct 64 bytes long against a 48-byte Rust struct (`vec3<u32>` padding):
    WGSL is compiled by naga when the pipeline is created and layouts are checked by wgpu
    validation when drawing, so it panicked at frame 240, the first frame with the blurred backdrop.
    `bytemuck::Pod` only proves the Rust side has no padding.
  - The dependency break above (a compile failure, but in a crate the spike never named).
  - Performance traps no type shows: `write_texture` at 4K, per-fragment storage-buffer reads.
  - Everything visual: a wrong blur sigma, a misplaced label or a missing black shape behind a
    border compile fine; the spike was checked by exporting frames as PNG and comparing them with
    frames of the Qt export.
- **Effort split.** Of the time spent, most went into reproducing QML effects in WGSL (shadow,
  rounded clipping with antialiasing, blur, text) and into measurement plumbing; the iced-specific
  code (preview window, controls, keyboard, layer-shell bar) was quick and worked on first run.

## Running it on n1x (aarch64, NVIDIA)

Needs Rust 1.88 or newer, ffmpeg, the NVIDIA Vulkan driver (`nvidia-utils`), wayland and
libxkbcommon (loaded at runtime). Nothing is cross-compiled; build on n1x:

```sh
cd spikes/iced
export CARGO_TARGET_DIR=~/.cache/omareel-spikes/targets/iced   # keep the 2+ GB out of the tree
cargo build --release                  # Cargo.lock carries the winit-core pin; don't cargo update
B=$CARGO_TARGET_DIR/release/omareel-iced-spike
bash ../make-bundle.sh ~/spike/b1080 1080 && $B setup ~/spike/b1080
bash ../make-bundle.sh ~/spike/b2160 2160 && $B setup ~/spike/b2160
$B preview ~/spike/b1080               # space, arrows, Home, the slider
$B preview ~/spike/b2160 --measure 10 --stats /tmp/preview.json   # prints the stats JSON
$B export ~/spike/b1080 -o /tmp/iced.mp4 --timing
$B bar                                 # top-centre pill; the stop button exits
```

To compare with Qt as here: build `omareel` as SPEC.md says, then from the repository root
`spikes/iced/scripts/measure.sh $B <omareel> ~/spike <results dir>` and
`spikes/iced/scripts/summarize.py <results dir>`. On NVIDIA: the spike's decoder tries
`-hwaccel cuda` (NVDEC) first, so 4K decoding should be much cheaper than here; the DRM fdinfo
GPU columns are probably empty with NVIDIA's driver (use `nvidia-smi pmon` instead); and the nested
harness needs a nested Hyprland that starts on the NVIDIA GPU. `setup` takes the wallpaper from
the current Omarchy theme.

## Verdict for omareel

iced can do omareel's hard parts: custom GPU compositing inside a widget at 60 fps with 4K video,
an offscreen export through the same shaders that beats the Qt exporter, and a layer-shell bar,
all in safe Rust with no fork and a type system that caught nearly every mistake before it ran.
What it does not do is help with them. Qt Quick gives omareel MultiEffect, layers, masks,
`Text`, `Image` and hardware video for free; in iced each of those is code to write and maintain
(the scene is 2.4x the QML), video needs its own decoder pipeline and loses hardware decoding
without unsafe Vulkan interop, and the rest of the editor (timeline with thumbnails, panels,
popovers, menus, animated controls, Omarchy theming) would be rewritten against a widget set that
is capable but thinner than Qt Quick Controls. Replacing QML wholesale with iced is feasible but
costly, and the payoff is a pure-Rust build rather than anything users would see. The part worth
taking regardless of toolkit is the design the spike ended up with: one plain-wgpu composition
renderer fed by `omareel_core`, hosted by the UI for the preview and run headless for the export,
which would also fit the GPUI or Slint hosts.
