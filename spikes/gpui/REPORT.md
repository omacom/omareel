# GPUI spike: report

The spike (spikes/SPEC.md) builds omareel's composition in GPUI over `omareel_core`: a preview
window that plays a recording bundle with play/pause, a scrub bar and a time label, an offscreen
1080p export piped to ffmpeg through `omareel_core`'s `ExportJob`, and the recording bar as a
wlr-layer-shell pill. All three work with upstream GPUI and no fork. The scene came out close to
the Qt build's frame for frame, the 1080p preview holds 60 fps, and the layer-shell bar took no
effort at all. What GPUI lacks is a way to put video on the GPU: every frame goes through its
sprite atlas as a new image, which costs about 5 ms per 1080p frame and 17 ms per 4K frame on
the UI thread. The 4K preview therefore runs at 30 fps unless ffmpeg scales the frames down
first, and the export runs at about half the Qt build's speed. Offscreen rendering is only
possible through GPUI's test-support API.

## What was built

| Part | File | Lines (code) |
|---|---|---|
| The composition as an element tree (Composition, RoundedFrame, CursorOverlay, CameraOverlay, BlurredBackdrop, KeystrokeOverlay, Background) | `src/scene.rs` | 538 (478) |
| BGRA conversion, the camera's 24x24 blurred backdrop, wallpaper loading, the preview's decode threads | `src/media.rs` | 272 (224) |
| Preview window: playback clock with pre-roll, play/pause, scrub bar, time label, `--bench` statistics | `src/preview.rs` | 538 (495) |
| Offscreen export into `ExportJob` | `src/export.rs` | 165 (141) |
| Layer-shell recording bar | `src/bar.rs` | 118 (109) |
| `setup`: the spike's project.json, written through the editor's `Document` | `src/setup.rs` | 63 (50) |
| Command line | `src/main.rs` | 54 (45) |
| Click/drag helper for tests (wlr-virtual-pointer) | `src/bin/spike-pointer.rs` | 86 (72) |
| Measurement harness (nested Hyprland, sampling, summaries, export rounds, interaction test) | `bench/` | 328 (287) |

For scale, the QML the scene imitates (Composition, RoundedFrame, CameraOverlay,
BlurredBackdrop, KeystrokeOverlay, Background, CursorOverlay) is 682 lines; the scene here is
538. The spike has none of the editor's panels, timeline or inserted-video layer (MediaLayer).

### The bundle

`spikes/make-bundle.sh` makes the videos; `omareel-gpui setup <bundle>` then writes the
project the spec describes through `omareel_core::editor::Document`, so it is what the Qt editor
would save: the zoom a new project generates from the fixture's clicks (one, 1.2 s to 9.2 s at
2x, following the cursor), the default round camera bubble (6 px white border at 0.7, 45°
shadow), the current Omarchy wallpaper as an explicit path (the nested compositor runs with its
own config directory), keystroke pills on, and the clips split at 6 s and 8 s with the middle one
in the camera's full-frame layout as a centred square over a blurred copy. The shared input log
has no key presses, so `setup` appends three shortcuts (Ctrl+Shift+P at 1.5 s, Ctrl+S at 4.2 s,
Ctrl+Z at 8.6 s) to the bundle's copy; without them no pill ever shows. The Qt measurements use
the same bundles.

## GPUI as used

- **Upstream GPUI from the Zed repository**, pinned at `a5c4ca8` (2026-10-25): `gpui`,
  `gpui_platform` (features `wayland`, `font-kit`) and `gpui_wgpu`. The crates.io release
  (0.2.2, October 2025) predates the wgpu renderer, so git is the only way to get it. It built
  standalone at the first attempt. Zed's `[patch.crates-io]` entries for `async-task`, `calloop`
  and `khronos-egl` are repeated in the spike's Cargo.toml because a git dependency does not
  inherit them; the spike also builds and runs without them. GPUI in turn pins wgpu itself to a
  git revision (wgpu 30, `3b3506e`), and the dependency tree has 431 crates.
- No fork or patch. The one feature beyond the defaults is `test-support`, for the export.
- The Linux renderer is wgpu on Vulkan (Mesa ANV here).

## Each part

### Logic

`Document::open`, `motion_job().run()` / `apply_motion`, `set_output_position` and
`preview_composition` give the preview everything QML's `comp` holds; the export takes
`ExportJob::state()` per frame. Both map into one `scene::Frame`. Nothing needed bridging: it is
Rust calling Rust, and the spike has no logic of its own beyond the drawing and a playback clock.

### Decoding

`omareel_core::export::decoder::FfmpegDecoder` on a thread per video (screen, camera), a few
frames ahead through a bounded channel. GPUI's images are BGRA (in a type named `RgbaImage`), so
each RGBA frame is swizzled on the decode thread. The camera thread also makes the backdrop's
24x24 blurred copy. Playing starts the decoders at the playhead and the clock once the first
frame is in, because the core's decoder tries `-hwaccel cuda` first and, on this Intel machine,
loses a few hundred milliseconds to the fallback at every start. `--decode-height N` has ffmpeg
scale the screen.

### The scene

An element tree rebuilt every frame: absolutely placed `div`s and `img()`s whose rectangles come
from the QML's formulas in output pixels, scaled to the view (1102x620 in the preview, like the
editor's pane; 1920x1080 in the export). Side by side with Qt's export of the same frames the
results are hard to tell apart (wallpaper, padding, rounded frame and its shadow, the zoom, the
arrow cursor with its click ripple, the bubble's border and shadow, the keystroke pill's
keycaps). What each piece took:

- **Rounded video**: `img(ImageSource::Render(..)).rounded(..)`; `ObjectFit::Cover` gives the
  camera's centred crop. GPUI clips images to rounded rectangles natively.
- **Shadows**: `BoxShadow` (GPUI's analytic Gaussian). Qt's MultiEffect blur of `b` x 32 px maps
  to a GPUI blur radius of about a third of that.
- **Zoom and the camera's scale**: GPUI has no transforms on elements. Both are a scale about a
  point, which maps rectangles to rectangles, so each rectangle, radius and blur is transformed
  in the scene code. A rotation would not have been possible this way.
- **Cursor**: a `canvas()` painting the arrow of `assets/cursors/arrow.svg` as a `PathBuilder`
  fill and round-joined stroke, rotated and scaled about its corner. `svg()` would not do: GPUI
  draws SVGs as single-colour masks, and the arrow has two colours.
- **Opacity**: only through `div().opacity()`. `Window::with_element_opacity` is crate-private, so
  a custom paint cannot fade an image; the scene had to be elements rather than one canvas.
- **The full-frame camera's blurred backdrop**: GPUI has no blur, no filters, no shaders of your
  own and no render-to-texture. The camera is shrunk to 24x24 and blurred on the CPU (Qt's
  BlurredBackdrop also captures into 24x24, then blurs on the GPU), and the GPU's bilinear filter
  stretches it over the canvas. This works only because the source is tiny. A blurred wallpaper
  (`background.blur`) would need a CPU blur of the full image once; the zoom's motion blur
  (`zoomStyle.motionBlur`, off by default) is not possible without a fork.
- **Gradient backgrounds**: GPUI's gradients have two stops; omareel's presets have up to three.
- **Keystroke pills**: flex rows of `div`s with text in the monospace family, laid out by GPUI
  as QML's Row and Column lay them out.

### Preview window

Works: play/pause (button or Space), a scrub bar (click or drag), a time label, the camera's
layout change at 6 s. `bench/interact-body.sh` clicked through all of it in the nested
compositor.

**Video is the hard part.** GPUI has no public custom render pass and no external-texture
element: `surface()` / `paint_surface` exist only on macOS (CVPixelBuffer), and the wgpu
renderer's device is private to the platform window. The one route is the image path: each frame
is a new `RenderImage` that `paint_image` puts into the sprite atlas. For a frame that size the
atlas copies the pixels (`to_vec`), creates a new texture sized to the frame, and uploads it with
`write_texture`, all on the UI thread, and the image stays in the atlas until the app calls
`window.drop_image`. Leaving that call out is not an error of any kind: an export without it
grew to 5.7 GiB of GPU memory in ten seconds (200 MiB with it). The least invasive route to
60 fps is this image path plus, at 4K, ffmpeg scaling to 1080p before the frames reach GPUI;
that needs no fork. Showing full-resolution 4K at 60 fps would need a GPUI change: an atlas
entry that can be re-uploaded in place, or a Linux counterpart of `paint_surface` that samples a
texture the app owns.

### Offscreen export

GPUI has no public offscreen API; `gpui_platform::headless()` on Linux gives windows that lay out
but draw nothing. What it has is for tests: with `test-support`, `HeadlessAppContext` runs an app
on the test platform with a real renderer (`gpui_platform::current_headless_renderer`, i.e.
`gpui_wgpu::WgpuHeadlessRenderer`), and after `Window::draw`, `Window::render_to_image` renders
the frame into a texture and reads it back. The spike renders each frame that way and hands the
pixels to `ExportJob::encode`; `ExportJob` does all the rest exactly as `omareel export` does
(probing, the decode thread, the encoder's arguments for the quality, the encode thread, putting
the file in place). Workarounds:

- Test windows have a fixed scale factor of 2 and `HeadlessAppContext` cannot change it. The
  first export read back 3840x2160 frames and ffmpeg, told 1920x1080, silently made a
  40-second video of quarter frames. The window is now opened at half the output size.
- `render_to_image` is synchronous: a new readback buffer each frame, a blocking map, a CPU copy.
  Nothing can be overlapped with it.
- The test platform's dispatcher is deterministic: timers and async work run only when the
  caller drives it. The export uses none, but an offscreen render of anything animated through
  GPUI's own executor would not advance in real time.
- These are test APIs; nothing promises they stay.

### Layer-shell bar

`WindowKind::LayerShell(LayerShellOptions { layer: Overlay, anchor: TOP, margin, .. })` with a
transparent window background, straight from upstream's `layer_shell` example. In the nested
Hyprland it maps as an overlay layer named `omareel-bar`, 240x44, 12 px from the top, centred;
hover works and clicking Stop quits. About 100 lines.

## Numbers

Intel Iris Xe (Raptor Lake-P), 20 threads, foundry2. The Qt build is `omareel` from this branch
(Release). Other agents were building and measuring on the same machine during this work; each
figure below says what the load average was, and the preview's headline runs were taken while it
was about 1.

### Preview

Nested Hyprland (`tests/tools/nested-hyprland.sh`). Its own output follows the outer display,
a 30 Hz TV here, so `bench/nested-body.py` adds a 60 Hz headless output and opens the client
there. Ten seconds from the start of playback: the spike's `--bench 10`, and `omareel edit` with
`OMAREEL_PREVIEW_STATS=1`, which starts playing by itself; the window starts at the last
`preview state=1` either prints. Frame intervals are the client's `wl_surface.commit`s
(`WAYLAND_DEBUG=client`), the same measure for both. CPU and RSS are sampled from /proc four
times a second, for the process and with its children (the spike decodes in ffmpeg child
processes; Qt decodes in-process). GPU is the i915 per-client engine time from
`/proc/<pid>/fdinfo` (there is no nvtop or intel_gpu_top here). Both draw the composition at
1102x620 in a 1878x1038 window. Dropped frames: the spike counts screen frames never shown (of
600); Qt's are the sum of its stats lines' `drops`.

| load ≈ 1 | Qt 1080p | GPUI 1080p | Qt 4K | GPUI 4K | GPUI 4K, ffmpeg → 1080p |
|---|---|---|---|---|---|
| presented frames per second | 58.1 | 59.5 | 57.3 | 33.3 | 58.9 |
| frame interval median / p95 / max (ms) | 16.7 / 17.0 / 50.6 | 16.8 / 19.1 / 31.2 | 16.7 / 17.5 / 37.7 | 33.0 / 43.3 / 46.7 | 16.9 / 20.8 / 33.2 |
| intervals over 20 ms | 3 | 4 | 17 | 244 | 60 |
| dropped video frames | 2 | 6 | 38 | 266 | 14 |
| CPU %, process | 14 | 47 | 19 | 126 | 62 |
| CPU %, with children | 14 | 106 | 54 [a] | 360 | 567 |
| RSS MB, process | 447 | 247 | 668 | 454 | 239 |
| RSS MB, with children | 447 | 917 | 1086 [a] | 2141 | 1274 |
| GPU render engine % | 12 | 42 | 9 | 29 | 37 |
| GPU video engine % (VAAPI decode) | 28 | 0 | 64 | 0 | 0 |

[a] The Qt editor's `ffprobe` pass for frame times ran inside the window.

A second round at load 7 to 18 gave the same picture: Qt 55 to 58 fps with p95 ≤ 18.7 ms at
both sizes, the spike 59 fps at 1080p (one run fell to 47 fps under load 11), 30 to 31 fps at 4K
and 59 fps at 4K scaled to 1080p.

Where the spike's frame goes: building the element tree in `render()` takes 0.04 ms; GPUI's
profiler (`--features gpui-profiler`, a separate build) puts `Window::draw` (layout and paint)
at 1.4 ms median (2.7 ms p95) and dirty-to-present at 3.9 ms (6.2 ms) at 1080p, and at 4.0 ms
(9.3 ms) and 16.4 ms (30.3 ms) at 4K (load 3). The difference between the two sizes is the frame's trip through the
atlas. Leaving the screen out of the scene while still decoding and swizzling it at 4K brings
the 4K preview to 59.5 fps with 7 drops, so the decoders keep up; GPUI's image path does not.

The CPU difference at 1080p is mostly decoding: Qt decodes on the GPU (VAAPI), the core's
`FfmpegDecoder` in software here (it only tries CUDA), and the spike also swizzles every frame
to BGRA. GPUI's render engine time is three to four times Qt's; it redraws the whole window every
frame while playing, samples the 4K wallpaper and evaluates its shadows analytically.

### Offscreen export

1920x1080, quality "social", both through `ExportJob`, so the same libx264 arguments
(`-preset medium`, 6.2 Mb/s); `bench/export.sh`, three rounds, alternating.

| | Qt `omareel export` | GPUI spike |
|---|---|---|
| 1080p screen: frames per second (best of 3, load 6 to 9) | 86.6 (6.9 s) | 45.5 (13.2 s) |
| 4K screen: frames per second (best of 3) | 49.0 (12.2 s) | 24.9 (24.1 s) |
| per frame, 1080p screen | upload 0.4 ms, render 6.6 ms, readback 1.3 ms (pipelined) | to BGRA 2.4 to 3.1 ms, layout+paint 1.3 to 1.6 ms, render+readback 10.6 to 12 ms (synchronous) |

The spike's best 1080p run at a quieter moment was 56 fps. A later set of three rounds, taken
while the load rose from 5 to 30, was too noisy to use (Qt 31 to 60 fps and the spike 23 to 47
fps at 1080p) but kept the order. Without the screen frame in the
scene it exports at 100 fps (render and readback 6.4 ms), so GPUI's renderer and readback alone
would be close to Qt's pipeline; the half that is missing is the video frame's copy into BGRA,
into the atlas, into a new texture.

### Build and size

| | GPUI spike | Qt build, for reference |
|---|---|---|
| clean release build | 2 min 8 s to 2 min 36 s wall, 33 CPU-minutes (load 12 to 14) | 88 s (`ninja -j8`, of which 75 s is cargo building `omareel_qt`) |
| incremental release build (touch scene.rs) | 4.1 s | |
| incremental debug build / `cargo check` | 1.5 s / 0.4 s | |
| binary | 224 MB with line tables, 31 MB stripped, statically linked except system libraries | 9.3 MB stripped, plus the Qt libraries |
| crates in the dependency tree | 431 | |

## Writing it

GPUI is pleasant to write in. Styling is Tailwind-like builder methods (`.absolute().left(..)
.rounded(..).shadow(..)`) that an agent guesses right most of the time, and the element tree is
plain Rust rebuilt every frame from plain data, which suits a composition driven by a document
model. The first full compile of about 1,300 lines had six errors and the second compiled: four
were `object_fit` living on a trait (`StyledImage`) that was not imported, where rustc names the
trait to import; one was `ArenaClearNeeded::clear` needing `&mut App`; one was a field name in
omareel_core.

The documentation is the README, the examples and the source. The examples covered the layer
shell completely. Everything else that mattered had to be read out of GPUI's source: that
offscreen rendering exists only in test-support (and how to drive it), that render images are
BGRA, that atlas entries live until `drop_image`, that test windows are fixed at scale 2, that
`with_element_opacity` is private, that `surface()` is macOS-only. GPUI moves fast and the API
changes between revisions; a spike or an app has to pin a revision and expect breakage when it
moves.

What broke at runtime that the compiler did not catch:

- The export's frames were four times the size it asked for (the test window's scale factor);
  ffmpeg made a 40-second video out of them without complaint.
- Without `drop_image`, every video frame stays in the atlas: GPU memory grows by a frame's size
  each frame, with no warning.
- Pixels must be BGRA in a buffer typed `RgbaImage`; RGBA shows with red and blue swapped.
- A custom paint cannot fade an image (the opacity setter is private), which is why the scene is
  made of elements rather than one canvas.
- The spike's own bug: the preview drained the decode channel into an unbounded queue, so the
  decoders raced ahead and the process reached 4 GB. Nothing in GPUI's or Rust's types would
  have caught it.
- `HeadlessAppContext`'s dispatcher does not run timers in real time (a trap, not hit).

## Running it on n1x (aarch64, NVIDIA)

Needs Rust 1.98 or newer, ffmpeg, the Vulkan loader and NVIDIA's Vulkan driver (nvidia-utils),
libxkbcommon, wayland, fontconfig and freetype. GPUI uses Vulkan through wgpu, not VAAPI, so the
NVIDIA VAAPI crash does not apply; the core's decoder tries CUDA (NVDEC) first, which works there
if the distribution's ffmpeg has it, and `ExportJob` encodes with NVENC when it works, for Qt and
the spike alike.

```sh
cd spikes/gpui
export CARGO_TARGET_DIR=~/.cache/omareel-spikes/targets/gpui
cargo build --release                       # first build: tens of minutes on a laptop
bin=$CARGO_TARGET_DIR/release/omareel-gpui
s=~/.cache/omareel-spikes/scratch-gpui      # anywhere outside /tmp
../make-bundle.sh $s/bundle-1080 1080 && $bin setup $s/bundle-1080
../make-bundle.sh $s/bundle-2160 2160 && $bin setup $s/bundle-2160

$bin preview $s/bundle-1080                 # Space or the button plays; drag the bar to scrub
$bin preview $s/bundle-2160 --bench 10      # plays 10 s after 1.5 s, prints JSON statistics, quits
$bin preview $s/bundle-2160 --bench 10 --decode-height 1080
$bin export $s/bundle-1080 -o $s/out.mp4    # prints frames/s and per-frame times
$bin bar                                    # the layer-shell pill; Stop quits
```

To measure as here (nested Hyprland, 60 Hz headless output; artifacts under /tmp/ogp, which are
small): `bench/previews.sh 1 $s $bin <path to the Qt omareel>` and
`bench/export.sh 3 $s $bin <path to the Qt omareel>`. nvtop shows per-process GPU use on NVIDIA
(the i915 fdinfo numbers here will read zero there). The Qt editor runs under the name
`qt-reference` in those scripts because a recording parity test elsewhere on this machine kills
any `omareel edit` it finds.

## Verdict

GPUI can draw omareel's composition, and drawing it was the easiest part of the spike: the
element model, the shadows, rounded images, paths and text did what the QML does, the layer
shell was a few lines, and the compiler caught almost everything. It is not a good fit for
omareel as it stands, because omareel is a video editor and GPUI has no video path on Linux.
Every frame costs CPU copies on the UI thread that grow with the frame (half the Qt build's
export speed, 30 fps for a 4K preview at full resolution), blur and other effects have to be
faked on the CPU or are impossible, and offscreen rendering, which the exporter needs, exists
only as a test facility. Each of these could be fixed with a modest change to GPUI (an
external-texture element sampling a wgpu texture the app owns, a public offscreen renderer with
pipelined readback, a blur primitive), but that means carrying a fork of a fast-moving framework
whose priorities are Zed's. Replacing QML with GPUI would trade Qt Quick's mature video and
effects pipeline for a nicer programming model and a pure-Rust build, and omareel would pay for
it in exactly the parts that are its reason to exist.
