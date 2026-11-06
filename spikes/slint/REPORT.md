# Slint spike: report

omareel's preview, offscreen export and recording bar built with **Slint 1.18.1** (the current
release) over `omareel_core`, measured against the Qt build of this branch on foundry2 (Intel Iris
Xe, Raptor Lake-P, 20 threads; Mesa 26.2.2, Qt 6.11.2, Hyprland 0.56.2).

**In short:** everything in SPEC.md works, with no fork and no patch of Slint. Slint's markup is
close enough to QML that the composition ported binding for binding (484 lines of `.slint`
against 681 of QML), and its built-in drop shadows, transforms, rounded clipping and opacity
covered everything but the blur. The preview holds 60 fps at 1080p and 4K, and the export is 1.22x
faster than `omareel export` at 1080p (1.07x from the 4K bundle), with frames that match Qt's
(PSNR 39.0 dB, SSIM 0.9955 between the two encodes). The costs: video and offscreen rendering
rest on the `unstable-wgpu-30` feature, which Slint may change in any minor release; there is no
blur, no layer shell and no offscreen API, so the backdrop blur is a CPU blur, the bar is a
300-line custom platform and the export a custom headless platform; and femtovg redraws the whole
scene every frame, which costs about 7.5 ms of CPU on the render thread and 3 to 4 times Qt
Quick's GPU time.

## What was built

| Part | Files | Lines | Qt counterpart (lines) |
|---|---|---|---|
| Composition | `ui/composition.slint` | 484 | Composition, RoundedFrame, CursorOverlay, CameraOverlay, BlurredBackdrop, KeystrokeOverlay, Background QML (681) |
| Rust → composition | `src/scene.rs` (settings and per-frame state into the `Comp` global, video textures, the CPU blur) | 485 | the `comp` context property + FrameSource (156) |
| Preview | `src/preview.rs`, `src/video.rs`, `PreviewWindow` in `ui/app.slint` (98) | 775 | PreviewPane.qml (295, more features) + QtMultimedia playback |
| Offscreen export | `src/export.rs` | 300 | Exporter.cpp + ExportRenderer (533) |
| Layer-shell bar | `src/bar.rs` + `RecordingBar` in `ui/app.slint` (60) | 362 | RecordingBar.qml (412, many more features) + LayerShellQt |
| The same bar through layer-shika | `bar-layershika/` (its own workspace) | 104 | |
| Setup, CLI | `src/setup.rs`, `src/main.rs`, `build.rs` | 182 | (spike only) |
| Test helpers, measurement | `src/bin/spike-pointer.rs`, `scripts/` | 491 | |

- **Logic**: `Document::open`, the motion track built once (`motion_job().run()`), then per frame
  `set_output_position` and `preview_composition` as the CXX-Qt editor reads `comp`. The export
  runs inside `omareel_core::export::ExportJob` (decode thread, per-frame `CompositionState`, the
  ffmpeg encoder with `omareel export`'s arguments; libx264 here since NVENC is absent), so the
  spike supplies only the renderer. `setup` writes the same `project.json` as the iced spike
  through the `Document`: two zooms, the round camera with border and 45° shadow, the Omarchy
  wallpaper, keystrokes, and the middle clip in the camera's "square + blur" full-frame layout;
  it adds three shortcuts to the bundle's input log, which has no key presses.
- **Composition** (`ui/composition.slint`): a port of the QML with the same formulas, reading a
  `Comp` global that plays QML's `comp` (settings once, state per frame). Wallpaper with
  `image-fit: cover`; the frame with padding, rounded corners (`clip` + `border-radius`),
  optional border and its shadow (`drop-shadow-*`, sized from MultiEffect's `shadowBlur` ×
  `blurMax`); the zoom as `transform-scale` about `transform-origin` on the stage, as QML's
  `Scale`; the cursor SVG (`@image-url`, embedded at build time) with `transform-rotation` and
  scale about its top-left; ripples as stroked circles; the camera bubble (round, border under
  the video, 45° shadow, scale and opacity with `presence`, shrinking while zoomed) morphing into
  the full-frame square over the blurred copy; keystroke pills (layouts, `for` repeaters, key caps
  joined by "+") in fontconfig's monospace. Settings that are strings in QML (`camera.shape`,
  `position`, `fullFrame`, the cursor style, the keystroke position) are Slint enums, so a typo is
  a compile error.
- **Not drawn** (not in this bundle): inserted media clips (MediaLayer), a blurred background
  image, zoom motion blur (off by default). Gradient backgrounds are wired up (see workarounds)
  but untested.
- **Preview**: a winit window (Wayland) drawn by femtovg on wgpu (Vulkan) with Play/Pause, a
  scrub `Slider`, the time, and Space. Decoding: one thread per video runs omareel_core's
  `FfmpegDecoder` (CUDA first, then software) and copies each frame into a mapped wgpu staging
  buffer; the UI thread records a buffer-to-texture copy into one of two textures per video and
  sets `Comp.screen` to `Image::try_from(texture)`. The playhead moves in the rendering notifier
  (`RenderingState::BeforeRendering`), just before Slint draws, so each frame shows the time it
  is drawn at; an `animation-tick()` binding keeps Slint rendering while playing. Play, Pause
  and the scrub bar were clicked in the nested compositor through `spike-pointer`
  (wlr-virtual-pointer, `scripts/interact.sh`): play started, pause held, the scrub landed at
  6.2 s on the full-frame camera.
- **Export**: a custom Slint platform whose one window renders with `FemtoVGWGPURenderer` (the
  renderer the preview uses) into an Rgba8Unorm texture on a headless wgpu device; three readback
  buffers in flight; frames handed to `ExportJob::encode`. `--png <frames>` dumps frames.
- **Bar**: the compiled `RecordingBar` on a small custom platform: smithay-client-toolkit makes a
  220x52 surface on the overlay layer anchored to the top with no keyboard interactivity (what
  `main.cpp` asks of LayerShellQt), Slint's software renderer draws into wl_shm buffers, and
  pointer events go to the Slint window. `hyprctl layers` showed `overlay: xywh 850 12 220 52,
  namespace omareel-recording-bar`; Stop, clicked through `spike-pointer`, printed "stop clicked"
  and exited. 5 % CPU (the dot's opacity animation repaints the whole buffer in software every
  frame), 16 MB RSS.

## Workarounds, unstable features, things Slint lacks

Nothing was forked or patched; `Cargo.toml` pins `slint = "=1.18.1"`.

1. **`unstable-wgpu-30`** carries both halves of the video path: `Image::try_from(wgpu::Texture)`
   and `FemtoVGWGPURenderer` (offscreen). Slint says such modules "may be removed or changed in
   future minor releases, as new major WGPU releases become available"; 1.18 already offers
   `unstable-wgpu-29` and `-30` side by side. A texture must be Rgba8Unorm(Srgb) and have
   `RENDER_ATTACHMENT` usage, or `try_from` fails at run time.
2. **Image caching**: Slint keeps what an image item drew, and the layer a rounded clip rendered
   its children into, until the `source` property changes. Writing a new frame into the same
   texture would leave a stale picture on screen, so each video alternates two textures. (Found
   by reading the renderer; nothing in the docs says it.)
3. **4K uploads**: `Queue::write_texture` of a 33 MB frame on the UI thread cost 7.4 ms a frame
   (56.8 fps); calling it from the decode thread holds wgpu's queue lock through the copy and
   stalled the frame submit. Mapped staging buffers filled on the decode threads fixed it (59.8
   fps, 0.13 ms on the UI thread). Same finding as the iced spike: this is wgpu, not Slint.
4. **No blur or shader effects.** The blurred camera backdrop is made on the CPU: the same 24x24
   capture QML takes, upsampled, a small Gaussian, MultiEffect's `saturation: .1` (1.1x), scaled up
   by Slint. A blurred background image and the zoom's motion blur would need the same, or a
   custom wgpu pass into an imported texture. Drop shadows are built in and match MultiEffect's.
5. **Opacity differs from QML.** Slint's `opacity` draws an element and its children into a layer
   and blends that once; QML multiplies the opacity into each child, blended on its own. The
   backdrop's dark fill therefore stopped showing through the half-faded picture, and the camera
   morph was visibly brighter than Qt's until each child took the fade itself (PSNR 36.7 → 39.0
   dB). Any QML that fades a group of overlapping items needs the same thought when ported.
6. **Gradient brushes from Rust**: `Brush::LinearGradient` is public, but `LinearGradientBrush` and
   `GradientStop` are only reachable through `slint::private_unstable_api::re_exports`, the
   generated code's doc-hidden module. In markup, `@linear-gradient` has a fixed number of stops,
   and omareel's presets have 2 to 10.
7. **Fonts**: `font-family: "monospace"` silently fell back to a proportional sans; the spike
   resolves fontconfig's monospace with `omareel_core::theme::monospace_family()` (as the iced
   spike does) and passes the family name.
8. **No offscreen API on the winit backend.** The export installs its own platform. The platform
   is process-wide and the winit backend's type is not public, so an export *inside* the editor
   process, next to its winit window, would need a delegating platform built on the internal
   `i-slint-backend-winit` crate, or the export run as a child process (`omareel export` exists).
   The software renderer cannot do it instead: it draws no box shadows (`draw_box_shadow` is a
   TODO) and no transforms.
9. **No layer shell.** winit 0.30 has none, so neither has Slint's backend. layer-shika 0.3.1
   (the community crate, last release January 2026, built against Slint 1.14) works with 1.18.1
   and in the nested Hyprland (`bar-layershika/`, 0.6 % CPU, 75 MB RSS with femtovg on EGL), but it
   only loads `.slint` files at run time through `slint-interpreter`, with properties and
   callbacks set by name as dynamic values, and it is AGPL-3.0. The spike's own platform keeps the
   compiled, typed component; it ignores the output's scale (buffer scale 1), so on a scaled
   monitor the pill would be upscaled.
10. **Driving frames from the rendering notifier** (setting properties in `BeforeRendering`) works
    and is what makes the preview frame-accurate, but the docs do not say that setting properties
    there is allowed. A window has one notifier.
11. Small things: `WGPU_BACKEND=gl` makes the winit event loop exit at start-up ("Broken pipe"),
    Vulkan is the default and works; `Window::try_dispatch_event` is deprecated in 1.18 in favour of
    `dispatch_event_with_result`. Licensing: Slint is GPL-3.0, royalty-free with attribution for
    desktop apps (an AboutSlint or a "Made with Slint" badge), or commercial; omareel (MIT) would
    use the royalty-free licence.

## Numbers

All on foundry2, each run started once the machine had been under 8 % CPU for 6 s
(`scripts/quiet.py`; other agents share the machine); the load averages are the 1-minute
average at the end of each run, most of it from the measurement itself. Raw results are in
`results/` (`scripts/summary.py results` prints these tables).

### Preview (SPEC 5)

In the nested Hyprland (`tests/tools/nested-hyprland.sh`) on a 1920x1080@60 headless output
(`scripts/in-nested.sh`), both apps auto-playing the bundle from the start; the canvas is
1102x620 in both. Frame times are the intervals between the window's `wl_surface.commit`s
(`WAYLAND_DEBUG=client`, `scripts/frames.py`, 9.5 s of playback); CPU, RSS and GPU (DRM fdinfo
render-engine busy time; neither `nvtop` nor `intel_gpu_top` is installed) from
`scripts/sample.py` over 8 s of playback. "ffmpeg CPU" is the spike's decoder children; the Qt
editor decodes in-process with hardware decoding, so its CPU column includes decoding. "Dropped"
is each app's own count: the spike counts recording frames in the 10 s never shown (600 minus
those shown), the Qt editor `OMAREEL_PREVIEW_STATS`'s `drops` (frames presented short of the
recording's rate). Median of 3 runs (range).

| | frames/s | frame ms median | p95 | max | missed vsyncs (>25 ms) | dropped | CPU % | ffmpeg CPU % | RSS MB | GPU % | load |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Slint 1080p | 59.8 | 16.90 | 19.70 | 24.9 | 0 (0–2) | 32 (26–39) | 69.4 | 56.9 | 191 | 36.4 | 0.6–0.8 |
| Qt 1080p | 59.7 (54.7–59.7) | 16.75 | 16.94 | 22.0 (17.8–239.6) | 0 (0–9) | 1 (1–10) | 16.7 | – | 426 | 10.5 | 0.6–2.7 |
| Slint 4K | 59.8 | 16.89 | 21.47 | 26.6 | 2 (0–2) | 27 (8–33) | 125.7 | 235.9 | 240 | 40.9 | 0.8–1.3 |
| Qt 4K | 59.5 | 16.75 | 16.99 | 33.9 | 5 (2–5) | 67 (66–80) | 21.7 | – | 637 | 9.1 | 2.9–4.8 |

- Both hold 60 fps at both sizes. Qt's commits are steadier (p95 17 ms against 20–21 ms) but
  have the long stalls (one 240 ms hitch at 1080p, 30–35 ms ones at 4K); the spike's never went
  past 28 ms. The spike's p95 is jitter in when Mesa's Vulkan WSI thread commits, not late frames:
  the median interval stays one refresh and the vsync misses stay at 0–2.
- The spike's dropped frames are mostly the output running at 59.1–59.7 Hz against a 60 fps
  recording plus that jitter (the nearest frame is picked each time); Qt's definition differs.
- CPU: Slint's render thread takes **7.5 ms a frame at 1080p and 8.4 ms at 4K** (rendering
  notifier, `BeforeRendering` to `AfterRendering`): femtovg walks and redraws the whole item tree
  each frame, where Qt Quick's scene graph updates only what changed. That is about 45 % of a core
  of the spike's 69 %; the rest is the decode threads copying frames (the 4K figure is mostly that).
  Taking parts away (1080p, one run each, `SPIKE_EXPERIMENT`): square corners (no rounded-clip
  layers) 6.4 ms, no shadows 7.2 ms, neither nor the keys 5.6 ms; rounding the canvas (a layer
  of the whole composition) costs nothing measurable.
- Decoding is the other big difference and is not Slint's: the spike runs ffmpeg in software
  (no CUDA on the Xe; 0.6 cores at 1080p, 2.4 at 4K), where QtMultimedia decodes on the GPU.
- GPU: 36–41 % against Qt's 9–10 %: femtovg's stencil-and-cover paths, the rounded clips' layers
  every frame, the frame copies, and redrawing everything.
- RSS is lower than Qt's (191/240 MB against 426/637 MB), though the Qt editor also loads its
  full interface and players.

How frames reach the GPU (one run each; `--upload`):

| | frames/s | p95 ms | missed vsyncs | UI-thread prepare ms (mean) | render ms (mean) | RSS MB |
|---|---|---|---|---|---|---|
| staged buffers, decode threads (default) 1080p | 59.8 | 19.7 | 0 | 0.09 | 7.5 | 191 |
| `write_texture` on the UI thread 1080p | 59.8 | 20.6 | 0 | 0.83 | 7.4 | 279 |
| `SharedPixelBuffer` image per frame 1080p | 59.8 | 20.7 | 2 | 0.78 | 8.3 | 304 |
| staged buffers 4K | 59.8 | 21.5 | 2 | 0.13 | 8.4 | 240 |
| `write_texture` on the UI thread 4K | 56.8 | 21.8 | 3 | 7.41 | 8.6 | 387 |
| `SharedPixelBuffer` image per frame 4K | 46.5 | 29.2 | 105 | 3.13 | 15.3 | 509 |

The CPU route (a new `SharedPixelBuffer` image each frame, uploaded by Slint) is fine at 1080p
and falls to 46 fps at 4K, where Slint creates and fills a new 33 MB texture every frame.

### Export (SPEC 6)

The 600-frame bundle to 1920x1080 MP4 at 60 fps, quality "social" (libx264, `omareel export`'s
arguments, the same `ExportJob`), wall time of the whole command. Median of 3 runs (range).

| | seconds | frames/s | load |
|---|---|---|---|
| Slint, 1080p bundle | 5.45 (5.43–5.46) | **110.2** | 3.5–4.2 |
| Qt `omareel export`, 1080p bundle | 6.65 (6.63–6.65) | 90.2 | 3.9–5.0 |
| Slint, 4K bundle | 10.84 (10.83–10.86) | **55.4** | 3.3–4.3 |
| Qt `omareel export`, 4K bundle | 11.63 (11.56–11.67) | 51.6 | 4.1–4.5 |

`--timing` (1080p, per frame): Slint render 5.2 ms, upload 2.0, readback 3.2, decode wait 0.9;
Qt render 6.7, upload 0.4, readback 1.3. Slint's femtovg renders the frame faster than Qt Quick
into its OpenGL framebuffer object; its upload (`write_texture` of 8 MB) and
readback (map and de-pad) are slower than Qt's pixel-buffer readback and could be staged as in
the preview. The 4K
export is bound by software decoding in both. With `--upload cpu` the export runs at 105.8 fps.
Frames against Qt's: PSNR 39.0 dB, SSIM 0.9955 over the 1080p encodes; the screen sections
are at 42–44 dB per frame, the worst frames (29 dB) are mid-morph into the full-frame camera,
where the CPU blur and Qt's MultiEffect blur differ.

### Build and size

- Clean release build 82 s (20 threads; 542 crates in the dependency graph), incremental 7 s after touching a `.slint`
  file or any `.rs` file (the generated code is in the same crate, so every change rebuilds it).
  The layer-shika bar, with `slint-interpreter`, builds clean in about 95 s.
- Binary: 151 MB with line tables, **25.7 MB stripped**, statically linking Slint, femtovg, wgpu
  and the image decoders; 13 shared libraries, no Qt. (The Qt `omareel` is 13.5 MB plus 17 Qt
  libraries.) The layer-shika bar alone is 26 MB stripped.

## Writing it (agent ergonomics)

- **The markup is the closest to QML of the three toolkits.** Property bindings, `in`/`out`
  properties, components, `for` and `if` in the tree, `animate` (QML's `Behavior`), states,
  `drop-shadow-*`, `transform-*`, `clip`, opacity: Composition.qml and friends translated nearly
  line for line, and the first build of 650 lines of `.slint` had one error ("Cannot override
  property 'border-width'", a name clash with Rectangle's own property, with file and line).
- **The compiler catches what QML only finds at run time.** Lengths and plain numbers are
  different types (`1px`, `/ 1px`), structs and enums are checked, and the generated Rust API
  (`comp.set_camera(CameraSettings { .. })`, `CameraShape::Round`) is checked against the
  markup at build time; a misspelt property or enum value on either side does not build. With QML
  the same mistakes are `undefined` at run time.
- **The Rust side is plain**: generated structs, a typed global, `VecModel`s for the repeaters.
  Errors from wgpu's API churn (`get_mapped_range` returning `Result` in wgpu 30,
  `BufferViewMut` not indexable) and from the missing gradient types were all compile errors
  with clear messages.
- **The expression language has no loops** (statements are `if`, `let`, `return` and
  expressions), so QML's JavaScript helpers that iterate (20 `for` loops in 14 omareel files) move
  to Rust. Since the logic is Rust already, that is mostly fine.
- **Where the docs ran out**: the language and the wgpu import are documented with examples;
  the custom-platform API is documented for microcontrollers; `FemtoVGWGPURenderer` has one line.
  What Slint caches between frames, how a rounded clip is drawn (a layer per frame), how opacity
  composes, and what the software renderer leaves out were answered only by the renderer's
  source in `~/.cargo/registry`.
- **What broke at run time that the compiler did not catch**: the monospace family silently
  falling back; the opacity difference from QML (a visual diff against the Qt export found it);
  stale frames had a single texture been reused (avoided only by reading the source); `try_from`'s
  texture-usage requirement; `write_texture` from another thread stalling the UI thread (a
  measurement found it); `WGPU_BACKEND=gl` failing at start-up with a bare "Broken pipe".

## Porting omareel's QML to Slint

The 60 QML files (7,171 lines) would go over mostly one for one, since the models match:

- Direct equivalents: Item/Rectangle/Text/Image/MouseArea → Rectangle/Text/Image/TouchArea;
  Row/Column/RowLayout → layouts; `Behavior on` (104 uses) and simple `NumberAnimation`s (112) →
  `animate`; Repeater (30) → `for`; Loader (13) → `if`; Popup/menus → `PopupWindow`,
  `ContextMenuArea`; ListView/Flickable (5) → ListView/Flickable; `Keys.on…`/Shortcut (43) →
  `FocusScope` handlers; Shape/ShapePath (10, in 4 files: ripples, the bar, the export dialog,
  the timeline) → `Path` with SVG commands; `theme` → a global filled from `omareel_core::theme`; the shadows among the 11
  MultiEffects → `drop-shadow-*`.
- Needs rework: `anchors` have no equivalent (explicit geometry or layouts); JavaScript that
  loops or builds arrays (70 functions) moves to Rust callbacks and models; QML's per-item opacity
  (see above); the one Canvas (the gradient background) → a brush or a Rust-drawn image.
- Not in Slint: blur and shader effects (MultiEffect blurs, the one ShaderEffect), file and colour
  dialogs (10 uses; rfd or the XDG portal from Rust), tooltips (a small component), video
  (`VideoOutput`/`FrameSource`, 12 uses → the wgpu path built here), layer-shell windows (the bar
  and countdown → the platform built here), `grabToImage` (the export dialog's thumbnail → render
  offscreen).
- The `editor` object (CXX-Qt, `crates/omareel_qt`) would become a set of Slint globals and models
  that Rust pushes the `Document`'s state into after each operation; the `Effect` queue already
  says what changed. There is no QObject-style automatic binding to Rust objects.

The hard parts would be the timeline (Timeline, ClipTrack, ZoomTrack: 1,400 lines of drag logic
in JavaScript), the effects, and the windows that depend on Hyprland (launcher, bar, countdown).

## Running it on n1x (aarch64, NVIDIA)

Slint 1.18 needs Rust 1.92 or newer (`rustc --version`; use rustup if Arch Linux ARM's is
older). The branch `ui-spikes-slint` is only local on foundry2 (not pushed): fetch it from there,
e.g. `git fetch foundry2:/home/ryan/.cache/omareel-spikes/slint ui-spikes-slint`.

```sh
cd spikes/slint
cargo build --release                   # about 1.5 min on foundry2; slower on n1x
bash ../make-bundle.sh ~/spike/b1080 1080
bash ../make-bundle.sh ~/spike/b2160 2160
B=target/release/omareel-slint-spike
$B setup ~/spike/b1080; $B setup ~/spike/b2160
$B preview ~/spike/b1080                # Play/Space, scrub bar; --upload wgpu-ui|cpu to compare
$B preview ~/spike/b2160 --measure 10 --stats 4k.json   # plays 10 s, prints frame stats, quits
$B export ~/spike/b1080 -o slint.mp4 --quality social --timing
omareel export ~/spike/b1080 -o qt.mp4 --quality social --timing   # the Qt build, to compare
$B bar                                  # the pill at the top of the focused output; Stop exits
```

- wgpu uses the NVIDIA GPU through Vulkan (the proprietary driver's Vulkan ICD must be
  installed); `WGPU_ADAPTER_NAME` picks another adapter. Nothing goes through VAAPI, so the
  `nvidia_drv_video.so` crash does not apply.
- n1x's FFmpeg has no CUDA, so `FfmpegDecoder` falls back to software decoding at each start
  (and each seek). If 4K playback drops frames there, `--decode-height 1080` has ffmpeg scale
  the screen down.
- The bar draws at scale 1; on a scaled monitor the compositor upscales it.
- `scripts/measure.sh` reproduces the tables in a nested Hyprland (it needs `grim`, Python 3,
  and the Qt build's path).

## Verdict for omareel

Slint is the most plausible of the three for **replacing QML as a QML-like toolkit**: the scene
ported almost mechanically, the type checking across markup and Rust is a real gain for
agent-written code, and the preview and export reach Qt's frame rate (the export is faster).
It is not a drop-in, though:

- omareel's hardest parts — video, blur, offscreen export, the layer-shell bar — all fall
  outside Slint's stable API: two rest on `unstable-wgpu-30`, two on hand-written platforms, the
  blur on the CPU. That code is small (about 900 lines here) but it is omareel's to maintain
  through Slint's and wgpu's releases.
- femtovg redraws everything every frame: about 7.5 ms of CPU and 3–4x Qt Quick's GPU time for
  this scene on the Xe. It fits in a 60 Hz frame, but a laptop will feel it, and a richer
  interface around the preview costs more on every frame.
- Decoding, the biggest CPU difference, is a separate problem for any Rust front end (no
  hardware-decoded frames into wgpu without unsafe interop).

So: worth it only if the goal is to drop Qt, C++ and CXX-Qt altogether; for the preview and
export alone, Qt Quick is cheaper per frame and already done. If omareel does move, Slint is the
toolkit to move to, with the video/effects layer written on wgpu first and an export that runs in
a child process.
