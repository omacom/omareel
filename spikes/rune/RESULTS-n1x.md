# Rune spike on n1x (aarch64, NVIDIA)

The Rune spike from `REPORT.md`, measured on n1x against the Qt build of the same branch, with
foundry2's numbers beside n1x's.

**In short:** everything runs on NVIDIA and aarch64 with no change to the spike's app code. The
measurement scripts needed three fixes, all on the NVIDIA side, and one test helper gained a drag.
The GPU cost that drives the "not now" verdict mostly disappears on this GPU: Rune's export pass
takes 0.33 ms a frame (3.5 ms on the Xe), and during preview playback Rune draws 9.0–9.3 W of GPU
power against Qt's 9.4–10.5 W. With no hardware video in Arch Linux ARM's FFmpeg, both apps decode
in software here, and they cost the same CPU. Rune exports 1.5× faster than Qt at 1080p and launches
4× faster (0.68 s to a frame against Qt's 2.74 s). Most of Qt's launch time is Qt Multimedia probing
NVIDIA's hardware encoders, which a one-line environment change fixes (0.97 s). Qt still seeks 2× faster and drops fewer
frames. The verdict holds, but on NVIDIA it rests on maturity and video plumbing, not GPU cost.

## The machine

| | n1x | foundry2 |
|---|---|---|
| CPU | NVIDIA N1X, 20 cores, aarch64 | Intel Raptor Lake-P, 20 threads |
| GPU | NVIDIA RTX Spark N1X (Blackwell), driver 615.78.08, Vulkan | Intel Iris Xe, Mesa |
| Displays | eDP-1 2880x1800 @120 (scale 2), DP-6 3840x2160 @144 (2), DP-7 2560x720 @60 (2) | |
| Hyprland | 0.57.0 | 0.57.0 |
| Rust | 1.99.1 (Arch Linux ARM's own; no rustup needed) | 1.99 |
| FFmpeg | 9.0.2: no NVENC, no CUDA hwaccel; VA-API only through NVIDIA's driver, which omareel never uses | VA-API (`h264_vaapi`) |

## What broke, and the workarounds

1. **The nested Hyprland can't make a headless output on NVIDIA.** `in-nested.sh` creates a
   1920x1080@60 headless output; aquamarine can't allocate a buffer for it (`GBM: Failed to
   allocate a GBM buffer: bo null`, with or without modifiers, `AQ_NO_MODIFIERS=1` included), it
   stays 0x0, and Hyprland aborts (SIGABRT) once anything sizes it. Fix: `NESTED_OUTPUT=window`
   keeps the nested compositor's own window output at 1920x1080, and `OUTER_MONITOR=DP-7` floats
   that window (only it) in the outer compositor, sizes it 1920x1080 and places it on DP-7, the
   one 60 Hz monitor here, where it overlaps no other monitor. Frames then pace at 60.9 frames/s,
   median 16.6 ms. The window has to stay 1920x1080: a first try at 640x360 shrank the nested
   output to 640x360 (the window output follows the window) and paced irregularly (gaps of
   3–19 ms). That preview series was thrown away and re-run.
2. **NVIDIA's Vulkan WSI commits twice per frame.** With `wp_fifo_v1` it follows every frame's
   `commit` with an empty `wait_barrier` + `commit`, so `frames.py` counted Rune at twice its
   frame rate. It now counts only commits with a buffer attached since the last one.
3. **NVIDIA reports no DRM fdinfo engine times**, so the GPU % column can't be measured the way
   it was on the Xe. `sample.py` falls back to `nvidia-smi pmon` (per process, SM %) and
   `nvidia-smi dmon` (whole GPU) over the same window, once a second. SM % is not the same
   metric as i915's render-engine busy time, so the GPU columns don't compare across machines;
   GPU power from `dmon` (below) is the comparison that answers "what does it cost a laptop".
4. **No NVENC or CUDA.** omareel_core tries `h264_nvenc` and CUDA decoding first on NVIDIA;
   Arch Linux ARM's FFmpeg has neither, so both apps encode with **libx264** and decode in
   **software** here. The spike's preview never picked VA-API: every run logged
   `preview: screen ... decoded Software`.
5. **Qt's headless export aborts from a desktop shell.** Omarchy's session sets
   `QT_QPA_PLATFORM=wayland;xcb`; with `WAYLAND_DISPLAY` unset, `main.cpp` only switches to
   `offscreen` when `QT_QPA_PLATFORM` is empty, so Qt can't load a platform and aborts
   (core dump, no message). Unsetting it too works. Rune's export doesn't care.
6. **Qt launches in 2.7 s on NVIDIA** (0.58 s on foundry2). gdb shows Qt Multimedia's FFmpeg
   plugin creating hardware device contexts on the main thread before the first frame: VA-API
   through `nvidia_drv_video.so`, which creates a CUDA context, then a Vulkan device.
   `configureVideoDecoding()` already sets `QT_FFMPEG_DECODING_HW_DEVICE_TYPES=cuda` on NVIDIA,
   but the probing is for encoders. With `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` as well, Qt's
   first frame comes at 0.97 s. That's a one-line fix in `src/render/VideoDecoding.cpp`; I didn't
   make it here because it's the Qt app, not the spike. It also means Qt initialises the NVIDIA
   VA-API driver that omareel avoids everywhere else.

Changes on this branch, all in the spike: `scripts/in-nested.sh` (`NESTED_OUTPUT`,
`OUTER_MONITOR`), `scripts/frames.py`, `scripts/sample.py` and `scripts/summary.py` (above),
`scripts/measure.sh` (a variant can carry a decode height: `software-surface-1080`),
`scripts/interact.sh` and `scripts/launch.py` (screenshot whichever output `in-nested.sh` set up),
`scripts/power.sh` (GPU power and clock through `nvidia-smi dmon`),
`src/bin/spike-pointer.rs` (a `drag` mode) and `scripts/editor-interact.sh` (the editor driven by
hand in the nested compositor). The app itself (preview, export, editor, bar) is unchanged.

## Numbers

Same method as `REPORT.md`: each run started once the machine had been under 8 % CPU for 6 s,
median of 3 runs (range), variants one run each. The preview ran in the nested Hyprland on the
1920x1080 window output on DP-7 (above); both apps' canvas is 1102x620, as on foundry2. Built
from 3f9e3f8 for the launch numbers, 6737431 before that (3f9e3f8 only changes Play).

### Preview

On n1x Rune's default decoder is software (VA-API is skipped on NVIDIA), so its main rows and
its "software, NV12" variants are the same configuration; on foundry2 the main rows are VA-API.
"GPU SM %" is `nvidia-smi pmon` for the app's process; "whole GPU %" is `dmon`, which includes
both compositors and the rest of the desktop (an empty nested compositor alone reads 7–10 %).

| | | frames/s | frame ms p95 | max | missed vsyncs | dropped | CPU % | decoder CPU % | RSS MB | GPU | UI-thread frame ms |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **Rune** 1080p | n1x | 60.9 (60.8–63.5) | 18.7 | 29.3 (21.6–30.4) | 2 (0–2) | 7 (6–8) | 31.7 | 57.3 | 569 | SM 4.2, whole 10.8 | 1.14 |
| | foundry2 | 59.6 | 17.0 | 29.6 | 1 | 5 | 12.8 | 19.5 | 226 | render 44.8 | 0.78 |
| **Qt** 1080p | n1x | 60.8 (60.5–61.5) | 18.5 | 36.5 (33.9–44.8) | 2 | 1 | 86.8 (82.3–89.4) | (in process) | 798 | SM 2.0, whole 8.1 | – |
| | foundry2 | 59.5 | 16.9 | 17.6 | 0 | 1 | 15.1 | – | 332 | render 7.5, video 25.4 | – |
| Rune 1080p, software, RGBA `Pixels` | n1x | 60.4 | 21.3 | 36.9 | 5 | 11 | 48.3 | 88.7 | 715 | SM 3.4, whole 9.9 | 2.62 |
| | foundry2 | 59.5 | 17.8 | 31.9 | 3 | 6 | 31.0 | 52.4 | 274 | render 50.0 | 2.74 |
| **Rune** 4K | n1x | 60.6 (60.3–62.4) | 19.0 | 32.0 (26.5–41.2) | 3 (2–10) | 10 (5–11) | 48.9 | 135.5 | 664 | SM 1.8, whole 10.4 | 1.54 |
| | foundry2 (VA-API) | 59.5 | 17.4 | 29.1 | 2 | 4 | 38.0 | 68.8 | 298 | render 31.0 | 3.15 |
| | foundry2 (software) | 57.4 | 18.3 | 33.7 | 23 | 25 | 41.6 | 129.1 | 277 | render 45.7 | 3.12 |
| **Qt** 4K | n1x | 60.7 (60.6–61.2) | 18.6 | 38.3 (23.8–44.1) | 2 (0–3) | 6 (1–10) | 181.4 | (in process) | 1210 | SM 1.4, whole 9.4 | – |
| | foundry2 | 59.7 | 17.0 | 30.4 | 5 | 69 | 20.8 | – | 529 | render 6.0, video 74.0 | – |
| Rune 4K, software, RGBA `Pixels` | n1x | 59.0 | 21.4 | 55.2 | 12 | 16 | 94.7 | 224.4 | 982 | SM 1.2, whole 9.4 | 6.79 |
| | foundry2 | **36.1** | 36.8 | 49.8 | 221 | 240 | 127.6 | 239.6 | 438 | render 39.5 | 13.9 |
| Rune 4K, `--decode-height 1080` | n1x | 60.1 | 18.9 | 39.2 | 2 | 13 | 28.3 | 215.7 | 566 | SM 2.1, whole 8.8 | 0.99 |

GPU power, clock and whole-GPU SM % from `nvidia-smi dmon -s puc`, mean over the same 8 s of
playback, two runs each (`scripts/power.sh`):

| | GPU power W | whole GPU SM % | GPU clock MHz |
|---|---|---|---|
| Nested compositor, empty | 5.3, 3.9 | 9.9, 7.0 | 1620, 1472 |
| **Rune** 1080p | **9.3, 9.1** | 9.4, 9.8 | 1694, 1778 |
| **Qt** 1080p | **10.0, 9.4** | 8.4, 11.2 | 1777, 1825 |
| **Rune** 4K | **9.2, 9.0** | 15.6, 8.9 | 1628, 1734 |
| **Qt** 4K | **10.5, 10.0** | 9.0, 8.2 | 1995, 1893 |

- **The GPU gap is gone on this GPU.** On the Xe, Rune kept the render engine 45 % busy against
  Qt's 7.5 %. Here both apps sit in the same few percent of SM time, and Rune draws slightly
  *less* GPU power than Qt (about 0.5 W less, with a lower clock). Rune's renderer still blends
  every quad every frame; a Blackwell GPU just has an order of magnitude more headroom than the
  Xe (see the export's GPU pass below). On an Intel or AMD laptop the Xe numbers still stand.
- **CPU is a tie, because Qt loses its hardware decoder too.** Qt decodes in software in-process
  here: 87 % of a core at 1080p and 181 % at 4K, against 15 % and 21 % on the Xe with VA-API.
  Rune's UI thread plus its ffmpeg decoders come to 89 % and 184 %: the same.
- **4K holds 60 in software.** foundry2 needed VA-API for a steady 4K (software missed 23
  vsyncs); n1x's cores keep up (3 missed). `--decode-height 1080` isn't worth it here: the UI
  thread drops to 1.0 ms, but ffmpeg's software scaler takes the decoders from 136 % to 216 %.
  The RGBA path that collapsed on foundry2 (36 frames/s) holds 59 here.
- **Qt drops fewer frames**: 1 of 600 at 1080p and 6 at 4K, against Rune's 7 and 10. foundry2's
  4K was the other way round (Qt 69, Rune 4).
- **Memory is higher on NVIDIA for both** (the driver maps a lot more per process): Rune 569 MB
  and Qt 798 MB at 1080p, against 226 and 332 on foundry2. Rune stays 30 % (1080p) to 45 % (4K)
  below Qt.

### Export

The 600-frame bundle to 1920x1080 MP4 at 60 fps, quality "social", wall time, both with a
display. Both apps report `decoder: software` and `encoder: libx264` on n1x; foundry2 encoded
with `h264_vaapi`.

| | n1x seconds | n1x frames/s | foundry2 seconds | foundry2 frames/s |
|---|---|---|---|---|
| **Rune**, 1080p bundle | 4.61 (4.56–4.72) | **130.3** | 5.30 | 113.1 |
| Qt `omareel export`, 1080p bundle | 7.01 (7.01–7.03) | 85.6 | 5.68 | 105.6 |
| **Rune**, 4K bundle | 8.41 (8.21–8.45) | **71.4** | 11.12 | 54.0 |
| Qt `omareel export`, 4K bundle | 10.41 (10.14–10.53) | 57.6 | 10.48 | 57.3 |
| Qt without a display (software renderer), 1080p | 34.73 | 17.3 | 13.97 | 43.3 |

- **Rune is 1.52× Qt at 1080p and 1.24× at 4K** (1.07× and 0.94× on foundry2). The 4K RGBA upload
  that lost to Qt on the Xe costs 2.06 ms a frame here.
- Per frame at 1080p (`--timing`): pixels hand-over 0.10 ms, view and layout 0.79, prepare,
  encode and submit 3.99, readback 0.76 (foundry2: 0.16, 1.35, 5.65, 1.9). At 4K: 2.06, 1.91,
  4.81, 0.75.
- **The GPU pass is 0.33 ms median, 1.38 ms p95** with `--gpu-timing` (4K bundle: 0.32, 0.97),
  against 3.5 and 8.9 ms on the Xe: about 10× faster for the same scene.
- Frames against Qt's: SSIM 0.9934, PSNR 36.3 dB over the 1080p encodes, as on foundry2 (the
  camera blur differs by design).
- Qt's software-renderer export, which is what runs without a display, is 2.5× slower on n1x
  than on foundry2. Rune's export needs no display either way.

### Launch and responsiveness

`scripts/launch.py` as in `REPORT.md`, 6 launches of each app on a copy of the 1080p bundle (the
first, least warm, in brackets), in the nested compositor's window output on DP-7. The click
targets were re-measured from screenshots on n1x: Rune's editor and preview land where they did,
but Qt's ruler puts x=1400 at 7.35 s here, so Qt's second seek is at x=1471 (7.75 s). Every
seek lands at 2.0 s or 7.75 s in all three apps, and Play starts at 2.0 s on the screen clip.

| | Qt `edit`, n1x | foundry2 | Qt, n1x, encoder probing off | Rune `editor`, n1x | foundry2 | Rune `preview`, n1x | foundry2 |
|---|---|---|---|---|---|---|---|
| Launch to first frame | 2.74 s (2.89) | 0.58 s | 0.97 s (0.90) | **0.68 s** (0.62) | 0.24 s | 0.67 s (0.67) | 0.23 s |
| Launch to the video showing | 2.89 s (3.05) | 0.73 s | 1.15 s (1.15) | **0.79 s** (0.76) | 0.35 s | 0.76 s (0.82) | 0.34 s |
| Click to the next frame | 3.3 ms | 1.1 ms | 3.6 ms | 2.9 ms | 0.5 ms | 2.6 ms | 0.5 ms |
| Play to motion, median (range) | 66 ms (36–128) | 91 ms (87–107) | 76 ms (32–133) | 88 ms (45–110) | 97 ms (34–135) | 82 ms (44–202) | 117 ms (37–162) |
| Seek to its frame, median (max) | **42 ms** (66) | 14 ms (20) | 42 ms (81) | 88 ms (133) | 92 ms (118) | 87 ms (157) | 95 ms (114) |
| Idle, paused: CPU % / commits a second | 0 / 0 | 0 / 0 | 0 / 0 | 2.8 / 0 | 0.6 / 0 | 2.7 / 0 | 0.6 / 0 |
| Idle RSS, app / with its decoders (MB) | 741 / 741 | 310 / 310 | 712 / 712 | **463** / 892 | 201 / 502 | 461 / 892 | 199 / 504 |

- "Encoder probing off" is Qt with `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` (item 6 above). Even
  fixed, Qt takes 0.97 s to Rune's 0.68.
- **Rune's own launch is slower on n1x too** (0.68 s against 0.24). From its Wayland log: 0.21 s
  before it connects (process start, decoders spawned), then about 0.16 s creating the Vulkan
  device, then the first frame at ~0.58 s. NVIDIA's `vkCreateInstance` alone takes 65 ms and
  `eglGetPlatformDisplay` 40 ms here.
- **Seek**: Qt's in-process decoder seeks in 42 ms here (software decoding from the keyframe, where
  foundry2 used VA-API), Rune's new ffmpeg process per seek in 88 ms. Same gap, same cause as on
  foundry2.
- Idle GPU: both apps commit nothing while paused; `nvidia-smi pmon` reads 1–2 % SM for both
  over 5 s, which is its noise floor (whole GPU 0.4–3 %).
- Click-to-frame is slower on n1x for all three apps. The window output pays for the outer
  compositor's frame callback, so this row reflects the harness more than the toolkit.

**By hand on the real desktop** (eDP-1, scale 2), three launches each, timed from the Wayland
log and with a quarter-size capture of the monitor about every 0.15 s: Qt's first frame came at
2.60–2.71 s, Rune's editor at 0.56–0.61 s and its preview at 0.57–0.62 s. Qt shows **nothing**
for those 2.6 s: no window, no placeholder, so it looks as though the click didn't take. Then the
tile opens and the editor fills in. Rune's window slides in with Hyprland's open animation at
about 0.4 s and is fully drawn by about 0.6 s, which reads as instant. On n1x, Qt's launch is
the one thing a user would notice, and it's Qt's fix to make, not Rune's.

### Build and size

| | n1x | foundry2 |
|---|---|---|
| Clean release build | **1 min 43 s** (20 cores) | 2 min 19 s |
| One-line change, release | 25 s | 31 s |
| Binary, with line tables / stripped | 245 MB / **29.7 MB** | 237 MB / 34 MB |
| Shared libraries | 6 | 6 |
| Qt `omareel` (cmake + ninja, Release, no plugin) | 57 s; 17 MB | – |

The Qt build needs Corrosion, which n1x didn't have and couldn't install without sudo. I built
Corrosion 0.6.1 (Arch's version) into a temporary prefix and passed `CMAKE_PREFIX_PATH`.

## By hand

- **Preview** (`images/n1x/preview-and-bar-nested.png`): in the nested compositor through
  `spike-pointer`, Play started, Pause held, and the scrub bar landed at 6.2 s on the full-frame
  camera, three runs out of three, as on foundry2. On the real desktop it opened tiled on eDP-1,
  crisp at scale 2 (`images/n1x/editor-desktop.png` is the editor, same renderer).
- **Editor** (`images/n1x/editor-by-hand.png`, `scripts/editor-interact.sh`): Square and
  Rectangle morph the camera on springs and Round brings it back; dragging Padding to 2 % grows
  the frame, Corner radius to 32 px rounds it; clicking a zoom selects it (the inspector shows its
  level), dragging it moves it ("Move zoom" toast); clicking Clip 2 moves the selection ring; a
  click on the ruler seeks to 2.0 s; **S** splits Clip 1 there; **Ctrl+Z** undoes the split;
  **Space** plays and pauses. Nothing looked wrong. The grey block in the playing frames is the
  test pattern's own moving square.
- **Bar** (`images/n1x/bar-edp1.png`, real desktop): it lands on the **focused** monitor
  (eDP-1 here), centred, below Waybar (`hyprctl layers`: x 7150, y 578, 220x52; eDP-1's top is
  y 540, so Waybar's 26 px plus the bar's 12 px margin).
  It is sharp at scale 2: the capture is 2× logical and the text and the Stop icon are crisp.
  On two or three monitors it only ever shows on the one with focus; Rune's overlays can target
  a display (`DisplayId`), but the spike doesn't use that, and omareel would want the recorded
  screen. **Stop quits**: in the nested compositor, three runs out of three. On the real desktop
  I closed it by PID rather than move your pointer while you were working.

## Headless on NVIDIA

Both work, with or without a display, on the NVIDIA GPU through Vulkan:

| | wall time | notes |
|---|---|---|
| `editor ~/spike/b1080 -- --snap editor.png --at 1.5` | 1.16–1.24 s; 1.43 s with no `WAYLAND_DISPLAY` | 2880x1800 PNG (scale 2) |
| `editor ~/spike/b1080 -- --script scripts/editor/demo.json --video demo.mp4` | 10.5 s | 811 frames at 60 fps (13.5 s of demo) rendered in 9.65 s, 84 frames/s; GPU init 250 ms |

`images/n1x/editor-demo-headless.png` is six frames of that video: the same steps as foundry2's
`editor-demo.png`, same output.

## Does any of this change the verdict?

It changes the reason, not the answer.

- **GPU cost, the main reason on foundry2, doesn't show here.** On a Blackwell GPU Rune's
  draw-everything renderer costs no more power than Qt Quick. It still would on the Intel and
  AMD laptops most Omarchy users have, so it stays a reason, but it's a reason about iGPUs, not
  about Rune everywhere.
- **Video is still the weak spot, and NVIDIA makes it plainer.** With no usable hardware
  decoder, both apps decode on the CPU at the same cost, and Qt keeps the edge on seeking
  (42 ms against 88) and dropped frames. In-process decoding with a kept-open decoder, the same
  piece `REPORT.md` asks for, would close both gaps.
- **Rune wins outright on what users feel at launch and on export throughput** on this machine:
  a window in 0.68 s against 2.74 (0.97 once Qt stops probing encoders), and 1.5× the export
  speed at 1080p.
- **Maturity is unchanged.** Rune had no NVIDIA or aarch64 problem of its own: every breakage
  above was in the test harness, Hyprland's headless backend, FFmpeg's packaging or Qt.

**Still "not now; watch it."** Worth doing whatever the UI: set
`QT_FFMPEG_ENCODING_HW_DEVICE_TYPES` next to the decoding one in `VideoDecoding.cpp`, which cuts
the Qt editor's launch on NVIDIA from 2.7 s to 1.0 s.

## Where things are

Raw results (JSON, logs) are in `~/.cache/omareel-spikes/rune-bench/n1x/`; `scripts/summary.py`
on its `results/` prints the preview and export tables. Bundles, the Qt build, Corrosion and the
nested-compositor scratch directories were removed.

Reproduce on n1x from the repository root (Qt build in `<qt build>`):

```sh
export NESTED_OUTPUT=window OUTER_MONITOR=DP-7
B=spikes/rune/target/release
RUNS=3 bash spikes/rune/scripts/measure.sh $PWD/$B/omareel-rune-spike <qt build>/omareel ~/spike <results> export
RUNS=3 bash spikes/rune/scripts/measure.sh $PWD/$B/omareel-rune-spike <qt build>/omareel ~/spike <results> preview
RUNS=1 VARIANTS="software-surface software-pixels software-surface-1080" bash spikes/rune/scripts/measure.sh ... variants
SPIKE_POINTER=$PWD/$B/spike-pointer tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/launch --timeout 880 -- \
  spikes/rune/scripts/in-nested.sh spikes/rune/scripts/launch.py qt.json 6 620,849 887,438,1471 -- <qt build>/omareel edit ~/spike/launch
# ... launch.py rune-editor.json 6 32,868 900,460,1500 -- $B/omareel-rune-spike editor ~/spike/launch
# ... launch.py rune-preview.json 6 35,1051 1051,554,1523 -- $B/omareel-rune-spike preview ~/spike/launch
```
