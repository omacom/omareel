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

## What is left

- Qt: the first frame is now dynamic linking (~50 ms), EGL (~50 ms), QML creation (~85 ms, spread
  thin: repeaters, layouts, text; 88 tooltips are created up front) and the render thread's first
  frame (~45 ms, plus ~25 ms again when Hyprland sets the scale).
- Filmstrip: each frame still decodes from its keyframe (up to a second of video). Building the
  keyframe's own frame first, as a stand-in, would fill the strip in a fraction of the time.
- Rune: the preview still decodes through ffmpeg processes (seek 93 ms against Qt's 13); an
  in-process decoder like `media.c`'s is the fix.

## Tools

- `scripts/startup-profile.py`: runs `<app> edit` on fresh copies under perf (or not, `PERF=0`),
  records Wayland commits and when filmstrip frames land. `REOPEN=1` for the cached case.
  `CALLGRAPH=fp` with a frame-pointer build; `PERF_EVENT="-F 9999 --switch-events"` for waits.
- `scripts/analyze-startup.py <run>`: first frame, child processes, the main thread's timeline
  and its time by kind of work.
- `scripts/offcpu.py <run>`: what the main thread waited on, and for how long.
- `scripts/qml-profile.py`: the QML profiler (`qmlprofiler`, a `-DQT_QML_DEBUG` build) over the
  start. In Qt 6.12 its binding ranges don't nest cleanly, so it is good for counts (65 buttons,
  88 tooltips), not times.
- perf comes from Arch's package unpacked into `~/.cache/omareel-spikes/tools` (no root needed;
  `perf_event_paranoid` is 2). GammaRay isn't packaged; perf and the QML profiler covered it.
