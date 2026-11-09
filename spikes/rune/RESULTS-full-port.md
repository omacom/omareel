# The full Rune port, measured on foundry2

The whole of omareel drawn with Rune: the launcher, the recording bar and its countdown, the camera
bubble, the editor (side panel, top bar, dialogs, timeline, preview with sound) and the export.
Measured against the Qt build of the same tree on 2026-11-08, at `4fd1696` (branch
`t3/rune-spike-n1x`). `REPORT.md` and `RESULTS-n1x.md` describe the earlier spike, which ported
only the preview, the export, the bar and a slice of the editor. This file supersedes their
numbers for foundry2.

**In short:** the Rune omareel opens its windows in half the time Qt does, at every step of the
recording loop. The launcher appears in 93 ms against 238 ms, the editor in 0.27 s against 0.60 s,
and the editor after Stop in 0.36 s against 0.71 s. Edits to the composition (camera shape,
padding, roundness, zooms) ease into place where Qt's jump. Playback holds 60 frames/s in both;
export is level (1.06× Qt at 1080p, 0.92× at 4K). The app itself uses 20–45 % less memory.

What got worse:
- **GPU.** On the Intel Xe, Rune keeps the render engine 53 % busy during playback against Qt's
  8 %, because Rune redraws the whole window every frame.
- **Seeking.** A seek takes 93 ms against Qt's 13, because the port starts an ffmpeg process per
  seek.
- **Memory with the decoders.** Counting those ffmpeg processes, Rune uses 543 MB idle against
  Qt's 305.

## Setup

- foundry2: Intel Raptor Lake-P (20 threads), Iris Xe, Mesa, Hyprland 0.57.0 (0aa6745), Arch.
- Both apps run in a nested Hyprland with a 1920x1080@60 headless output (`scripts/in-nested.sh`).
  The capture-exclusion plugin built for this Hyprland is loaded (`OMAREEL_PLUGIN_PATH`).
- Qt: `omareel` built from the same tree (Release). Rune: `omareel-rune-spike`, release.
- Both use the same Rust core (`omareel_core`) and the same recorder (`omareel_record`): the region
  picker, the capture daemon and the finishing of a recording are the same code. What differs is
  the UI processes: the launcher, the bar, the editor, and how the preview decodes video.
- Each measurement started once the machine had been under 8 % CPU for 6 s (`scripts/quiet.py`).
  Launch and loop timings are the median of runs 2–6 of 6 (the first, least warm run is listed
  apart). Playback and export are the median of 3 runs, with ranges.

## What you feel: how quickly things appear

### Opening the editor (`scripts/launch.py`, `<app> edit <recording>`)

| | Qt | Rune | |
|---|---|---|---|
| Launch to the window's first frame | 0.60 s (first 0.60) | **0.27 s** (0.27) | 2.2× faster |
| Launch to the video showing | 0.73 s (0.74) | **0.34 s** (0.35) | 2.2× faster |
| Click to the next frame | 1.1 ms | 0.8 ms | |
| Play to motion, median (max) | **88 ms** (92) | 102 ms (130) | |
| Seek to its frame, median (max) | **13 ms** (18) | 93 ms (124) | |
| Idle, paused: CPU % / commits a second | 0 / 0 | 0.6 / 0 | |
| Idle RSS: app / with its decoder processes | 305 / 305 MB | **241** / 543 MB | |

The Rune editor here is the full one: every inspector panel, the timeline with its thumbnails, the
top bar, dialogs. It opens no slower than the spike's slice did (0.24 s to a window then).

### The recording loop (`scripts/loop-timing.py`, new)

A watcher polls `hyprctl clients` and `hyprctl layers` every few milliseconds and logs when each
window or layer surface first maps (Hyprland maps a surface on its first buffer). Each run starts
the launcher, presses Ctrl+R, clicks the desktop in the region picker, records 5 s with the
countdown on, stops with `<app> record --stop`, and waits for the editor the recorder opens.

| | Qt | Rune | |
|---|---|---|---|
| Launcher window appears | 238 ms (first 337) | **93 ms** (190) | 2.6× faster |
| Region picker after Ctrl+R (shared code) | 136 ms | 106 ms | |
| Recording state written after the click (shared) | 175 ms | 164 ms | |
| Recording bar appears after the click | 346 ms | **308 ms** | |
| `record --stop` returns (shared) | 164 ms | 164 ms | |
| Editor window appears after Stop | 709 ms | **356 ms** | 2.0× faster |

Most of the bar's time is the shared recorder starting up (164–175 ms before the bar is even
started). After Stop, the editor's own share is about 190 ms in Rune against 545 ms in Qt.

## Edits that move

Rune animates layout changes by default; the port uses that wherever Qt would jump. Paused, every
change to the composition springs into place: camera shape (round → square → rectangle morphs),
camera size and position, padding, roundness, shadow, zoom level. Qt's `Composition.qml` has no
`Behavior`s, so the same edits land in one frame. During playback and export the composition is
built with `instant_changes()`, so nothing is smoothed that shouldn't be.

Also new: a slider's knob springs to where you click (Qt's jumps), and a toast names each edit
as you make it, sliding up and fading after two seconds (Qt has none). Dialogs, popups, buttons
and the launcher's record button animate as they did in QML.

`images/full/morph.png` shows 8 frames of the camera bubble from round to square (0.6 s);
`images/full/edits.mp4` is 6 s of the editor driven by `scripts/editor/demo.json`, rendered
headless (`omareel-rune-spike edit <bundle> -- --script ... --video ...`).

## Playback (`scripts/measure.sh`, editor playing 10 s)

Rune in its own layout (`--canvas-height 0`: the canvas fills its pane, 1276x718) and Qt's editor
(canvas 1102x620), both with VA-API decoding.

| | | frames/s | frame ms p95 | max | missed vsyncs | dropped | app CPU % | decoder CPU % | RSS MB | GPU render % | UI-thread ms |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1080p | **Rune** | 59.4 | 17.4 | 29.7 | 2 | 7 | 21.8 | 20.7 | **253** | 53.1 | 1.99 |
| | Qt | 59.8 | 16.9 | 22.4 | 0 | 10 | 15.2 | (in process) | 326 | 7.6 (+26.5 video) | – |
| 4K | **Rune** | 59.4 | 17.5 | 32.0 | 3 | **7** | 42.3 | 68.8 | **310** | 46.7 | 3.75 |
| | Qt | 59.7 | 16.9 | 30.6 | 5 | 40 | 21.0 | (in process) | 561 | 5.9 (+72.6 video) | – |

- Both hold 60 with the same median pacing (16.7–16.8 ms). Rune drops fewer frames at 4K.
- Rune at Qt's canvas size (`--canvas-height 620`) changes nothing that matters: 1080p GPU render
  52.9 %, UI thread 1.57 ms; 4K 39.4 %, 3.73 ms.
- Rune's decoders are separate ffmpeg processes (VA-API, NV12 read back to the CPU), so their GPU
  video time isn't in the app's fdinfo and their CPU shows in the "decoder" column. Qt decodes in
  process; its video-engine time is in brackets.

### Where Rune's GPU time goes (1080p, one run each, 6 s window)

| | GPU render % | UI-thread ms |
|---|---|---|
| The full editor playing | 46.0 | 2.07 |
| The editor without the composition (`SPIKE_EXPERIMENT=nocomp`) | 24.6 | 1.51 |
| The composition without the camera blur (`noblur`) | 43.0 | 1.86 |
| The preview window alone, no editor around it | 38.7 | 0.77 |

About half of it is the editor's chrome (side panel, timeline, transport) being redrawn 60 times a
second although none of it changes during playback. Rune has no damage tracking or layer caching
yet; with them, the chrome would cost nothing while the video plays. That's a change in Rune, not
in the port.

## Export (600 frames to 1920x1080 MP4, quality "social")

| | seconds | frames/s | |
|---|---|---|---|
| **Rune**, 1080p recording | 5.38 (5.37–5.44) | **111.5** | 1.06× Qt |
| Qt, 1080p recording | 5.68 (5.64–5.70) | 105.6 | |
| **Rune**, 4K recording | 11.42 (11.18–11.66) | 52.6 | 0.92× Qt |
| Qt, 4K recording | 10.49 (10.45–10.51) | **57.2** | |

Rune's export renders headless (no display needed); Qt's falls back to its software renderer
without one (43 frames/s at 1080p).

## Footprint

| | Qt | Rune |
|---|---|---|
| Binary | 17 MB, plus the Qt 6 runtime (about 42 MB of Qt libraries it links, before QML modules and plugins) | 41 MB stripped, no toolkit runtime |
| Direct shared libraries | 29 | 13 (FFmpeg, VA-API, GBM, DRM, xkbcommon, evdev, libc) |
| UI source, non-blank lines | about 15,300 in three languages: QML/JS 7,100, C++ 1,800, CXX-Qt bridges 5,800 | about 13,700 in Rust, including the spike's measurement code |

## What still needs work

1. **GPU on integrated graphics.** 53 % of an Iris Xe's render engine during playback against
   Qt's 8 %. On n1x's NVIDIA GPU the spike drew slightly less GPU power than Qt (9.2 W against
   9.7 W, `RESULTS-n1x.md`), so this is an iGPU problem. The fix is damage tracking in Rune (half
   the cost is unchanging chrome); the rest is Rune blending every quad each frame.
   Package power couldn't be measured on foundry2 (RAPL needs root).
2. **Seeking: 93 ms against 13.** The port starts an ffmpeg process per seek. An in-process
   decoder kept open (FFmpeg through `omareel_core`, as the export already does) would match Qt.
   The same change fixes Play's start (102 ms against 88) and the paused memory (the decoder
   processes hold about 300 MB).
3. **Maturity.** Rune is weeks old and pinned to a dev revision (8ee3095). The port works around
   `Tx::quit` being ignored in overlays and linear-light colours.
4. **Not yet re-measured on n1x.** `RESULTS-n1x.md` is the spike, not the full port. There, Qt took
   2.74 s to open the editor (Qt Multimedia probing NVIDIA's encoders) against Rune's 0.68 s.

## Reproduce

From the repository root, with the outer `WAYLAND_DISPLAY` and `HYPRLAND_INSTANCE_SIGNATURE` set:

```sh
B=<cargo target>/release; Q=<qt build>/omareel; R=<results>
export OMAREEL_PLUGIN_PATH=<qt build>/plugin/omareel-capture-exclude.so SPIKE_POINTER=$B/spike-pointer
# Opening the editor
tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/launch --timeout 880 -- \
  spikes/rune/scripts/in-nested.sh spikes/rune/scripts/launch.py $R/launch-rune.json 6 772,849 886,437,1466 -- $B/omareel-rune-spike edit <recording>
#   Qt: ... launch-qt.json 6 619,849 886,437,1466 -- $Q edit <recording>
# The recording loop
tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/loop --timeout 380 -- \
  spikes/rune/scripts/in-nested.sh spikes/rune/scripts/loop-timing.py $R/loop-rune.json 6 $B/omareel-rune-spike
# Playback and export
RUNS=3 RUNE_MODE=editor RUNE_FLAGS="--canvas-height 0" bash spikes/rune/scripts/measure.sh $B/omareel-rune-spike $Q <bundles> $R/preview preview
RUNS=3 bash spikes/rune/scripts/measure.sh $B/omareel-rune-spike $Q <bundles> $R/export export
python3 spikes/rune/scripts/summary.py $R/preview
```

Raw results are in `~/.cache/omareel-spikes/rune-bench/full/` on foundry2.
