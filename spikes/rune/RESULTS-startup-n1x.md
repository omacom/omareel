# Opening the editor on n1x: where the start-up time goes

On n1x, Qt Multimedia's hardware encoder probe is still almost all of the Qt editor's start: 1.8 s of
its 2.4 s, spent in the kernel creating a VA-API device and a Vulkan device it never uses. Setting
`QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` takes the editor's first frame from 2.40 s to 0.57 s, and
the launcher and the recording bar from 2.1 s and 2.3 s to 0.48 s and 0.63 s. Past that, both apps
are bounded by NVIDIA's driver setup, which is kernel time perf can't see without root: about 205 ms
for Rune's Vulkan device and about 200 ms of EGL for Qt.

Measured on n1x (NVIDIA N1X: 10 Cortex-X925 + 10 Cortex-A725 cores, NVIDIA 615.78.08, Hyprland
0.57.0 nested, windowed on DP-7) on 2026-11-08, with `t3/startup-perf` at 51743e9 as "after" and
4fd1696 as "before". The recording is `make-real-bundle.sh`'s 3 minutes of 1440p60 with audio,
camera and 21,769 input events. Raw outputs are in `results-n1x/`.

## Results

Medians of runs 2 to 5 of 5 (reopen: runs 2 to 4 of 4), seconds from launch.

| | Qt before | Qt after | Qt after, encoder probe off | Rune before | Rune after |
|---|---|---|---|---|---|
| First frame on screen | 2.59 | 2.40 | **0.57** | 0.94 | **0.50** |
| Camera strip complete | 2.52 | 2.18 | 0.24 | 1.46 | 0.59 |
| Filmstrip complete (78 / 47 frames) | 14.28 | 4.08 | 2.08 | 9.30 | **1.47** |
| Reopening (cached): first frame | 2.56 | 2.23 | 0.51 | 0.95 | 0.50 |

"Encoder probe off" is Qt after with `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` in the environment
(`timings/clean-qt-after-enc.json`). Adding `QSG_RENDER_LOOP=basic` on top takes it to **0.45 s**
(`timings/clean-qt-after-enc-basic.json`, see bottleneck 2).

Before the change Qt's filmstrip took 14.3 s here, so every fresh run used `SECONDS_RUN=20`
instead of 14. Qt's first frame varies more than Rune's: 2.24 to 2.71 s across the five runs.

On foundry2 the same numbers were 0.29 s (Qt) and 0.11 s (Rune) to the first frame, and 1.6 s and
1.0 s for the filmstrip. n1x is 2 to 5 times slower to the first frame, almost all of it in driver
setup, not in our code.

### The recording loop

`loop-timing.py`, 6 runs each, medians in ms. The capture plugin loads on n1x (rebuilt for
Hyprland 0aa6745), so the bar stays on the recorded output.

| | Qt before | Qt after | Qt after, encoder probe off | Rune before | Rune after |
|---|---|---|---|---|---|
| Launcher window | 2188 | 2107 | **477** | 451 | 443 |
| Recording bar, from the click | 2293 | 2274 | **631** | 742 | 737 |
| Editor, from Stop | 2910 | 2551 | **922** | 1033 | 853 |

The encoder probe costs every Qt process the same 1.6 to 1.8 s, not just the editor.

## Top bottlenecks, ranked by milliseconds

### 1. Qt Multimedia probing hardware encoders: 1,830 ms (Qt only)

`QMediaPlayer::QMediaPlayer` → `QPlatformMediaIntegration::instance()`, called from
`PlaybackDriver::PlaybackDriver` in the `Editor` constructor, holds the main thread from 0.15 s to
2.15 s. The CPU profile only sees 190 ms of it (`vaInitialize` → `__vaDriverInit_1_0` 89 ms,
`vkCreateDevice` 59 ms, `cuStreamCreate`, `cuMemsetD8Async`) and the off-CPU profile finds almost
no sleeping in that span, so the rest is kernel time: `perf_event_paranoid` is 2 here, which
limits perf to user space, and kernel time shows up as "off the CPU" in `analyze-startup.py`.

Timing FFmpeg's own device creation on its own (`results-n1x/standalone/ffmpeg-hwdevice.txt`):

| `ffmpeg -init_hw_device` | wall | user | sys |
|---|---|---|---|
| none | 0.04 s | 0.02 | 0.02 |
| vaapi (`nvidia_drv_video.so`) | 0.62 s | 0.05 | 0.57 |
| vulkan | 1.09 s | 0.05 | 1.04 |
| cuda | fails with -12 (ENOMEM) in 0.03 s | | |

VA-API plus Vulkan is 1.7 s, which is the probe. `configureVideoDecoding()` already sets
`QT_FFMPEG_DECODING_HW_DEVICE_TYPES=cuda` on NVIDIA, but not the encoding one, so the encoder probe
still opens every device type. `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` removes it: 2.40 → 0.57 s
for the editor, and the launcher and bar as above.

**Fix:** set `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` beside the decoding variable in
`src/render/VideoDecoding.cpp`, under the same `vaapiDriverIsNvidia()` check. This is the same fix
`RESULTS-n1x.md` proposed (item 6); it isn't in 51743e9.

### 2. EGL setup on two threads: about 200 ms (Qt), 116 ms of it avoidable

With the probe off, the main thread spends 0.375 to 0.47 s in
`QSGRhiSupport::maybeCreateOffscreenSurface`, then waits 140 to 150 ms in
`QWaylandWindow::sendExposeEvent` for the render thread's first frame. The render thread is on
the CPU for only 32 ms of that (`QRhi::create` → `QOpenGLContext::create` 11 ms, NVIDIA's EGL
libraries 21 ms; `analyze/qt-after-encoff-dwarf-1-renderthread.txt`). The rest is kernel.

EGL on its own (`standalone/egl-setup.txt`, a device-platform display, so no window system):

| | wall | sys |
|---|---|---|
| `eglQueryDevicesEXT` | 23 ms | 12 |
| `eglInitialize` | 6 to 11 ms | 6 to 10 |
| `eglCreateContext` | 12 to 19 ms | 10 to 17 |
| first `eglMakeCurrent` | 66 to 74 ms | 64 to 72 |
| second context, same thread | 0.4 ms | 0 |
| first `eglMakeCurrent` on a new thread | 70 to 76 ms | 68 to 72 |

The 70 ms `eglMakeCurrent` is paid per thread, not per process. Qt's threaded render loop touches
EGL on the GUI thread (the offscreen surface) and again on the render thread, so it pays twice.
`QSG_RENDER_LOOP=basic` keeps it on one thread: **0.568 → 0.452 s**.

**Fix ideas:** use the basic render loop on NVIDIA (check playback first: it renders on the GUI
thread, and I haven't measured playback with it); or find what asks for the offscreen surface
before the window is exposed and avoid it. A warm-up context on a helper thread won't help, since
the cost is per thread.

### 3. Rune's Vulkan device: about 250 ms (Rune only)

`for_surface` (Rune's `Host::create` → wgpu instance, adapter and device) runs 0.12 to 0.44 s, on
the main thread, after the bundle has loaded. The CPU profile sees 52 ms of it
(`vkEnumerateInstanceExtensionProperties` 37 ms while the loader opens NVIDIA's ICD,
`vkCreateDevice` 9 ms); the rest is kernel. Only Vulkan is created; there is no GL device.

Vulkan on its own (`standalone/vulkan-setup.txt`):

| | wall | sys |
|---|---|---|
| `vkCreateInstance` | 43 to 60 ms | 33 to 50 |
| `vkCreateDevice` | 162 to 202 ms | 159 to 192 |

That is about 205 ms of kernel time no matter where it runs, but it doesn't have to run in turn
with everything else.

**Fix idea:** start the wgpu instance, adapter and device on a thread at the top of `main`,
overlapping the bundle load, the probes and the fonts (0.02 to 0.12 s, about 100 ms on the main
thread), and only create the surface on the main thread. The device doesn't depend on the window;
check surface support once both exist.

### 4. Second expose after Hyprland sets the scale: 110 to 123 ms (Qt)

Right after the first frame, `QWaylandWindow::setScale` → `sendExposeEvent` blocks the GUI thread
another 110 to 123 ms (`offcpu/qt-after-encoff-2.txt`). On foundry2 this was about 25 ms. It is
after the first frame, so it delays the first interactive frame, not the first visible one. My
guess (not profiled) is the window's buffers being reallocated at the new scale, which on NVIDIA
is more kernel time.

**Fix idea:** if that's it, getting the output's scale before the first expose would leave one
buffer allocation instead of two. Worth a look at whether Qt's Wayland plugin can be told the
preferred scale up front.

### 5. Dynamic linking: 70 to 86 ms (Qt), 25 to 33 ms (Rune)

The first 75 ms of Qt's main thread is `ld-linux` resolving symbols, and the Qt plugins it loads
later add more (`QFactoryLoader::instance`, 47 ms with everything it pulls in). Rune links less.

**Fix idea:** fewer plugin loads (see 6) is the part we control.

### 6. GTK platform theme: 30 to 38 ms (Qt)

Omarchy sets `QT_QPA_PLATFORMTHEME=gtk3`, so `QGuiApplication` loads `libqgtk3.so`, which runs
`gtk_init` (18 ms) and loads GTK's CSS. The app uses the Basic style and its own colours.
`main.cpp` already clears the variable for screenshots and headless runs.

**Fix idea:** clear it for the editor too, if nothing needs GTK's file dialog (the Insert and
background image pickers would fall back to Qt Quick's own).

### 7. QML creation: 58 to 66 ms (Qt)

Spread thin, as on foundry2. The QML profiler's top item before the first frame is `IconImage`
creation in `OmButton.qml` (lines 60 and 92), two per button, 130 buttons; its times are inflated
by the profiler but the count is real (`qml/qt-after-encoff-until0.5.txt`).

**Fix idea:** line 92 is the trailing icon, which every button creates even when
`trailingIconSource` is empty and the item is hidden; line 60 is the leading icon, created even for
buttons with no icon or with `leadingDot`. A `Loader { active: ... }` around each (or creating them
only when a source is set) skips most of the 260, since few buttons have a trailing icon.

### Smaller things

- **Wallpaper picker:** each finished wallpaper thumbnail sends `WallpapersChanged`, and
  `BackgroundPanel.qml:31` `wallpaperEntries` rebuilds the whole list each time: 100 calls,
  286 ms in the QML profile, after the first frame. It only happens while the picker's thumbnails
  are being made (first launch, or after a theme change), since they are cached in
  `~/.cache/omareel/wallpapers`. Coalescing the signal (one per batch) fixes it.
- **Rune waits:** about 100 ms of real sleeping before the first frame, the biggest being
  a `Command::output` on the main thread at 0.517 s (11 ms) and Wayland roundtrips.

## The filmstrip on aarch64

| | frames | first frame at | fill span | rate |
|---|---|---|---|---|
| Qt before | 78 | 2.54 s | 11.74 s | 6.6 frames/s |
| Qt after | 78 | 2.24 s | 1.87 s | 41.7 frames/s |
| Qt after, probe off | 78 | 0.31 s | 1.77 s | 44.7 frames/s |
| Rune before | 47 | 1.09 s | 8.21 s | 5.7 frames/s |
| Rune after | 47 | 0.54 s | 0.93 s | 50.8 frames/s |

`thumbnail_workers` is `20 × 3/4 / 2 = 7` here, so each frame costs a worker 140 to 155 ms: the
decode from the keyframe, up to a second of 1440p60. That is 7 to 9 times faster than before and
the biggest change on n1x after the probe. Two things would take it further:

- The keyframe-first stand-in from `STARTUP.md`: almost all of the 140 ms is decoding the frames
  between the keyframe and the one wanted.
- The clamp to 8 workers doesn't bind here (7), but half of n1x's cores are Cortex-A725s. At nice
  10, the workers likely land on the little cores while the big ones run the UI; I didn't record
  CPU placement (perf here records per thread, not per CPU), so this is a guess worth a
  `taskset` test.

## Regression checks

All on the after tree.

- `cargo test --offline --workspace --exclude omareel_qt`: 356 passed, 0 failed.
- `cargo test --release` in `spikes/rune`: 3 passed.
- `tools/parity/compare-binaries.sh` (before vs after Qt, with screenshots through a venv with
  Pillow): no differences.
- `ctest` (`QT_QPA_PLATFORM=offscreen`): 4 of 5 pass. `cameracapture` fails, **and fails the same
  way on 4fd1696**, so it isn't this change:
  `cameraFramesKeepTheirTimes`, `everyPackedFormatEncodes(NV12)`, `everyPackedFormatEncodes(NV21)`
  and `killedCameraEncoderKeepsCompletedFragments`. The cause is ffmpeg n9.0.2 on aarch64: the
  camera encoder's filter (`scale=in_color_matrix=bt601:in_range=tv:out_color_matrix=bt709:out_range=tv,format=yuv420p`)
  crashes on NV12 input with SIGSEGV or SIGABRT, and the core dump's stack is glibc aborting in
  `sws_freeContext` (heap corruption in libswscale). Plain `format=yuv420p`, the matrix change
  alone, or the range change alone don't crash; both together do. A webcam that delivers NV12
  would hit this when recording on n1x.
- Recording loop: no regression (table above).
- Playback (`measure.sh preview`, 3 runs, `RUNE_MODE=editor`): no regression. Before and after
  match within noise for both apps: 1080p CPU Rune 91.0 → 90.7 %, Qt 73.5 → 76.5 %; 4K Rune
  104.6 → 103.6 %, Qt 166.2 → 170.7 %. **Both ran at 15.1 frames/s**, though, against 59.5 in
  `RESULTS-n1x.md`: since that run DP-7 has become a 1280x360 strip, so `in-nested.sh` can't place
  the 1920x1080 nested window clear of the other monitors and it hangs mostly off screen. The
  comparison holds (same cap both sides); the absolute frame rates and drops don't. Moving the
  window to DP-6 or eDP-1 would cover a screen in use, so I left it.
- Export (`measure.sh export`, 3 runs): no regression. 1080p Rune 4.64 → 4.59 s, Qt 7.15 → 6.98 s;
  4K Rune 8.38 → 8.37 s, Qt 10.47 → 10.54 s.

## How it was run

- perf 7.3.1 from Arch Linux ARM's `perf` and `libpfm` packages (signatures checked), unpacked into
  `~/.cache/omareel-spikes/tools` with a wrapper. **On n1x every `--call-graph` mode fails with
  "Failure to open any events" unless `--user-callchains` is added**, so the runs passed it through
  `PERF_EVENT` (`PERF_EVENT="-F 2999 --user-callchains"`). No script change was needed for that.
- Qt profiles: `build/prof` (RelWithDebInfo, `-DQT_QML_DEBUG -fno-omit-frame-pointer`, Rust with
  frame pointers), 3 runs with `CALLGRAPH=fp` and 3 with `CALLGRAPH=dwarf,8192`, `SECONDS_RUN=4`
  (Qt's first frame is at 2.4 s here). Arch's Qt on aarch64 saves the frame record but doesn't set
  `x29`, so frame-pointer stacks break inside Qt; DWARF covered those, and the two agree.
  NVIDIA's libraries have neither, so stacks stop there either way.
- Rune profiles: the release build with `CALLGRAPH=dwarf,8192`, and a frame-pointer build
  (`RUSTFLAGS=-C force-frame-pointers=yes`, its own target dir) with `CALLGRAPH=fp`, 3 runs each;
  waits with `PERF_EVENT="-F 9999 --switch-events --user-callchains" CALLGRAPH=fp`.
- `qml-profile.py` needs `/usr/lib/qt6/bin` on `PATH` for `qmlprofiler`.
- Script changes: `analyze-startup.py` has a "Qt Multimedia, FFmpeg and CUDA" kind and counts
  `libnvidia-*` as GPU setup, so the NVIDIA time isn't "other". New `scripts/thread-stacks.py`
  sums one thread's stacks over a time window (used for the render thread).
- The standalone timings in `results-n1x/standalone/` come from small ctypes scripts kept there.
