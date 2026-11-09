# Opening the editor: where the time went, and what changed

Profiled on foundry2 (Raptor Lake-P, 20 threads, Iris Xe, Hyprland 0.57 nested, 1920x1080@60)
on 2026-11-08, for both the Qt app and the Rune port. The recording is a realistic one: 3 minutes
of 2560x1440 at 60 fps with AAC audio, a 720p camera and 21,800 input events
(`~/.cache/omareel-spikes/rune-bench/real3m`). Each run opens a fresh copy, as the editor opens
after Stop: nothing cached, no project.json.

## Results (median of runs 2–5 of 5)

| | Qt before | Qt after | Rune before | Rune after |
|---|---|---|---|---|
| First frame on screen | 0.61 s | **0.29 s** | 0.43 s | **0.11 s** |
| Camera strip complete | 0.63 s | **0.17 s** | 0.93 s | **0.19 s** |
| Filmstrip complete (78 / 47 frames) | 10.7 s | **1.6 s** | 6.9 s | **1.0 s** |
| Reopening (cached): first frame | 0.60 s | **0.26 s** | 0.42 s | **0.12 s** |

On the 10-second bars recording (`scripts/launch.py`, before the in-process media step):
Qt's window 0.63 → 0.29 s and its video 0.78 → 0.42 s; Rune's 0.28 → 0.14 s and 0.36 → 0.20 s.
Click, play and seek latencies did not change.

## What the profiles showed

Qt's main thread up to its first frame (perf with frame pointers, `scripts/analyze-startup.py`):

| | ms |
|---|---|
| Icon theme lookups (`QIconLoader` parsing `index.theme`, stat-ing every directory, for icons that are all bundled SVGs) | 122 |
| QML objects, bindings and layout | 84 |
| Wallpaper WebP decoded on the GUI thread (`asynchronous: false`) | 74 |
| Dynamic linking | 43 |
| EGL setup (`QSGRenderLoop::instance`) | 36 |
| Fonts | 24 |
| Waiting: two ffprobe runs in turn in the Editor constructor, fc-match | 173 |

Rune's main thread: 323 ms of its 491 waiting on ffprobe (the recording and camera probed in
turn, the frame-times ffprobe over every packet run synchronously, then two more ffprobes for
colour), and 75 ms building the motion track.

Both: every filmstrip frame was an `ffmpeg` process, one at a time. Each spent about 60 ms
starting and probing a 540 MB file before decoding up to a second of video to its frame: 130 ms
a frame, 78 frames.

## What changed

Shared (`omareel_core`, both apps):
- `csrc/media.c` + `media_av.rs`: probes, frame times and filmstrip frames through FFmpeg's
  libraries in process. Frame times come from the MP4 index (1 ms, no file read; it was an
  ffprobe reading the whole recording). Frames match ffmpeg's to 49–54 dB PSNR. Any failure falls
  back to the programs, so errors read as before.
- Filmstrip frames build in parallel (`thumbnail_workers`, three quarters of the cores, two
  decoding threads each) at nice 10, so the editor and its players come first.
- `load_bundle` probes the screen and camera at once; `Theme::reload` runs fc-match and hyprctl
  at once, and `theme::prefetch` starts them first thing.
- `Document::preload` / `open_preloaded`: opening the recording starts on a thread at the top
  of `main`, overlapping Qt's own start.

Qt:
- `QIcon::setThemeSearchPaths` to a path with no theme: the 122 ms of icon lookups go.
- The editor preview's wallpaper decodes asynchronously (exports still wait for it).
- The duplicate `fc-match` in `resolveFonts` is gone (the theme asks).

Rune:
- The motion track and the frame times are built on workers in the editor, as Qt does.
- Colour probes run beside the recording's opening, in process.
- Filmstrip frames build in parallel.

## Second round: end-to-end tests, n1x, and seeking

The branch was then tested end to end on foundry2 and on n1x (NVIDIA N1X, aarch64), whose own
write-up is `RESULTS-startup-n1x.md`. Final numbers (3-minute 1440p recording opened fresh; seek
and video on a 10 s 1080p recording with one-second keyframes):

| | foundry2 Qt | foundry2 Rune | n1x Qt | n1x Rune |
|---|---|---|---|---|
| First frame, before | 0.61 s | 0.43 s | 2.59 s | 0.94 s |
| First frame, after | **0.29 s** | **0.13 s** | **0.58 s** | **0.48 s** |
| Filmstrip complete, before | 10.7 s | 6.9 s | 14.3 s | 9.3 s |
| Filmstrip complete, after | **1.6 s** | **1.0 s** | **2.1 s** | **1.4 s** |
| Editor after Stop, before / after | 743 / **419 ms** | 363 / **243 ms** | 2910 / **922 ms** | 1033 / **853 ms** |
| Seek, before / after | 22 / 21 ms | 103 / **35 ms** | – / 43 ms | 170 / **86 ms** |

(n1x's display currently caps the nested window at 15 frames/s, which rounds its seek and play
times to 66 ms steps; first frames aren't affected.)

What the second round changed:

- **Qt on NVIDIA: Qt Multimedia's encoder probe.** The first media player created a VA-API device
  (through nvidia-vaapi-driver, which sets up CUDA) and a Vulkan device for encoding, which
  omareel never does through Qt: 1.8 s of kernel time on the main thread, in every Qt process
  (launcher and bar too). `configureVideoDecoding()` now limits it to CUDA on NVIDIA, as decoding
  already was. Intel and AMD are unaffected.
- **Rune's preview decodes in process** (`omr_stream` in `media.c`), its decoders kept in a pool
  between seeks instead of an ffmpeg process per stream. The idle editor with its decoders holds
  300 MB instead of 540, playback uses 13 to 23 % less CPU on n1x, and the video shows sooner.
- **Rune's seeks.** A click started the decoders twice (press and release), a paused seek waited a
  frame before starting them, and a seek sprang the composition's zoom and cursor to the new time
  instead of landing them. Paused, the keyframe is shown first (11 to 16 ms on n1x) and the
  exact frame follows; forward seeks without a keyframe between decode on rather than seek.
- **Qt's buttons** make their tooltip on first hover and their icons only when they have one
  (about 390 objects fewer in the editor). No change on foundry2; n1x's QML profile ranked them
  first.
- **Qt's loading spinner** shows only after 400 ms. Spinning, it redrew every frame and held the
  GUI thread for each render-thread swap, and the player's set-up queued behind it: the video
  showed 0.6 s after the window on n1x. Now 0.40 to 0.34 s on foundry2 and 1.22 to 0.94 s on n1x.
- **The wallpaper picker** rebuilt its whole list for every thumbnail made (a hundred times after
  a theme change); it now refreshes at most four times a second.
- **Checks:** all Rust (374), Qt (`ctest`, 4/4) and Rune tests pass; `compare-binaries.sh` finds no
  difference in output or screenshots; the recording parity harness finds only the two ffprobe
  runs the editor no longer makes; playback and export are unchanged on both machines. n1x
  showed that an odd NV12 width overran the colour plane (fixed: sizes round to even) and that
  ffmpeg 9.0.2's libswscale corrupts the heap in its threaded frame API with a colour-matrix
  change, which the camera encoder's filter uses (`cameracapture` fails there before and after
  this branch) and `media.c` doesn't.

## Third round: Stop to the editor, every process's start, export

The first two rounds timed the editor opening on its own. This round timed the whole loop as a
user meets it (launcher, picker, countdown and bar, Stop, editor), with system audio recording,
as most recordings have, and profiled export. Before is 291db4c; foundry2 unless noted.
`loop-timing.py`'s bar time includes its own 125 ms click (spike-pointer waits 60 ms, then holds
the button 60 ms).

| | before | after |
|---|---|---|
| Stop to the editor's window, with audio | 848 ms | **174 ms** |
| Stop to the editor's window, without audio | 393 ms | **103 ms** |
| Editor's first frame after the recorder's signal (3-minute 1440p) | 0.273 s (a fresh start) | **0.099 s** |
| The same, an editor started by hand | 0.273 s | **0.227 s** |
| 10 s recording, window / video after the recorder's signal | 0.29 / 0.34 s (a fresh start) | **0.082 / 0.199 s** |
| Every filmstrip tile showing a frame (3-minute 1440p) | 1.56 s | **0.41 s** |
| Launcher's first frame | 0.208 s | **0.178 s** |
| Click's release to the countdown on screen | about 211 ms | **about 102 ms** |
| `omareel record --stop` (and every command's start) | 45 ms | **15 ms** |
| Libraries omareel loads | 196 | **102** |
| Export, 10 s of 1080p / of 4K (VA-API) | 5.89 / 11.68 s | **2.90 / 5.10 s** |
| Export on n1x, 10 s of 4K / 3 minutes of 1440p (x264) | 8.72 / 77.1 s | **4.79 / 48.1 s** |

Rune, the same way: an editor started by hand 0.128 -> 0.106 s, 0.093 s from the recorder's
signal; Stop to the editor with audio 211 ms; its launcher 55 ms.

What it found and changed:

- **Stop waited for ffmpeg's stats timer.** The audio recorder notices the SIGINT that stops it
  only when its main loop wakes, every `-stats_period`: half a second by default, even with
  `-nostats`. Stop waited 150 to 520 ms for it, depending on when the signal landed. At 50 ms it
  exits 55 to 85 ms after the signal.
- **The editor starts while you record.** The recorder starts `omareel edit --wait <pid> <bundle>`
  once the recording runs. That process starts Qt, compiles Main.qml, lays out the theme's fonts,
  and warms the GL driver and Qt Multimedia, then waits: 157 MB, 0.16 s of CPU once, nothing
  after, and no dropped frames. At Stop the recorder sends SIGUSR1 once the recording is saved; a
  recorder that exits without sending it (discarded, failed) ends the editor too. Saving no longer
  waits on two ffprobe runs (in process now) or thumb.jpg (made after the editor is told).
- **The bar starts before the region is picked.** `omareel record` starts `__record-bar --wait`
  before its picker, and the launcher once it is on screen; the recorder adopts it with
  `sigqueue`. The recorder also polls for the bar's countdown every 2 ms, not 20.
- **FFmpeg's libraries are opened, not linked.** libavcodec, libavformat and libavfilter bring
  about 90 more libraries (every codec's), and binding them was most of the 45 ms every omareel
  process spent before `main`. `csrc/ffmpeg_dl.h` routes media.c's and gpu_capture.c's calls
  through a table that `dlopen` fills the first time; the recorder and window processes open it
  on a thread as they start. `omareel record` and the IPC commands no longer make a
  QGuiApplication.
- **The editor's QML builds less.** The filmstrip's and ruler's Repeaters had number models, which
  rebuild every delegate when the count changes, three or four times while the window settled
  (OmCountModel adds only the difference). Popups, tooltips, each clip's speed menu and the trim
  handles' caps are made when first shown; OmButton is lighter; QQmlApplicationEngine's file
  selector is gone. 7,547 to 5,696 objects in the first half second.
- **Filmstrip stand-ins.** A frame decodes from the keyframe before it (75 to 195 ms at 1440p);
  the keyframe's own frame takes 11 to 14 ms. Each tile's keyframe is built first, and the strip
  shows it until the exact frame lands. The strip also waits for the video's first frame (its
  decoders took ten cores in the first quarter second).
- **The launcher** lists audio devices on a thread and gets its hyprctl checks from a thread
  `main` starts.
- **Rune** calls `rune::warm_gpu` at the top of each window process (the device on a thread,
  which Rune already had) and has the same waiting editor and bar.
- **Export** (n1x's write-up, `RESULTS-export-n1x.md`): frames through 1 MiB pipes into buffers
  never zero-filled, decoders started a second ahead without retrying a failed CUDA, YUV frames
  converted on the GPU, opaque frames uploaded unconverted, four readbacks in flight, and QML
  that no longer re-reads the project each frame. Output matches the old build's to 46 dB
  (frames look the same; the conversion moved to the GPU).

Checks: Rust (375), ctest and Rune tests pass; `compare-binaries.sh` finds no difference; the
recording parity harness's differences are all intended ones (the ffprobe runs gone,
`-stats_period`, `edit --wait`, the editor's project.json there sooner). The parity harness
caught a waiting bar that held `omareel record`'s pipes open, and `compare-binaries.sh` a
launcher note that landed after the first layout; both are fixed.

## What is left

- The warm editor's video still shows about 110 ms after its window: Qt Multimedia's player opens
  the file, makes its decoder and hands the frame over, mostly waiting. The editor could show the
  first frame `media.c` decodes until the player's arrives.
- A launcher kept running (hidden, started at login or after the first use) would open in a few
  milliseconds instead of 0.18 s: about 150 MB held all the time.
- The recorder's own start (about 50 ms after the picker: its exec, a Wayland registry, the
  capture's setup) could also run before the pick, as the bar now does.
- Omarchy's picker (`omarchy-capture-region`) costs nothing measurable after the click.
- NVIDIA: `QSG_RENDER_LOOP=basic` shows Qt's video sooner on n1x (0.65 against 0.94 s) but a click
  then takes 20 ms to reach the screen instead of 5. Hyprland's scale change costs Qt a second
  expose (110 ms on n1x). Omarchy's `QT_QPA_PLATFORMTHEME=gtk3` costs Qt 30 to 38 ms on n1x.
- On n1x export is now x264's speed on the CPU: that ffmpeg has no NVENC.

## Tools

- `scripts/startup-profile.py`: runs `<app> edit` on fresh copies under perf (or not, `PERF=0`),
  records Wayland commits and when filmstrip frames land. `REOPEN=1` for the cached case,
  `WAIT=<s>` for the editor the recorder starts (timed from its signal), `NO_BUNDLE=1` for the
  launcher. `CALLGRAPH=fp` with a frame-pointer build; `PERF_EVENT="-F 9999 --switch-events"` for
  waits.
- `scripts/loop-timing.py`: the recording loop's windows; `LOOP_AUDIO=1` records system audio
  (with `OMAREEL_NESTED_AUDIO=1`, which points the nested session at the desktop's PulseAudio).
  `OMAREEL_NESTED_DEBUG=1` keeps `OMAREEL_DEBUG`, whose `stop_*` stages in /tmp/omareel.log time
  each step of saving. `scripts/launch.py` takes `WAIT=<s>` too.
- `scripts/analyze-startup.py <run>`: first frame, child processes, the main thread's timeline
  and its time by kind of work.
- `scripts/offcpu.py <run>`: what the main thread waited on, and for how long.
- `scripts/qml-profile.py`: the QML profiler (`qmlprofiler`, a `-DQT_QML_DEBUG` build) over the
  start. In Qt 6.12 its binding ranges don't nest cleanly, so it is good for counts (65 buttons,
  88 tooltips), not times.
- perf comes from Arch's package unpacked into `~/.cache/omareel-spikes/tools` (no root needed;
  `perf_event_paranoid` is 2). GammaRay isn't packaged; perf and the QML profiler covered it.
