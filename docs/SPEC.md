# omarecord — engineering spec

A polished screen recorder + editor for Omarchy (Hyprland, Wayland, Arch).
Records the screen **without** the cursor, records raw mouse/keyboard input separately, then
renders a beautiful video: padded/rounded frame on a background, a smooth synthetic cursor,
click effects, and animated zoom-ins that follow the cursor. Exports MP4 or GIF.

This document is the frozen contract. Implementation runs are scoped to it.

## 0. Stack and non-negotiables

- **Language/UI:** C++17, Qt 6.11 (Quick/QML, Controls Material dark, Multimedia, Effects,
  Svg). Same stack as omacut/omasnap so it feels native on Omarchy. Build with **CMake + Ninja**.
  Single binary `omarecord` with subcommands.
- **Capture:** `gpu-screen-recorder` (already on every Omarchy install) with `-cursor no`,
  `-write-first-frame-ts yes`, `-fm cfr`, `-f 60`, `-k auto` (NVENC/VAAPI, cpu fallback).
- **Cursor position:** poll the Hyprland IPC socket
  `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` with request `j/cursorpos`
  (returns logical global coords). One request ≈ 6 µs; poll at 240 Hz on a dedicated thread.
  Dedupe identical positions (only store when moved, but always store at least 1 sample / 250 ms
  as a heartbeat).
- **Buttons / scroll / keys:** libevdev on `/dev/input/event*` (user is in the `input` group on
  Omarchy). Open every device that has `EV_KEY` with `BTN_LEFT` or `EV_REL`/`REL_WHEEL` or
  keyboard keys. If no device can be opened, keep recording without click data and warn.
- **Encode/decode:** `ffmpeg`/`ffprobe` subprocesses. Never link libav directly.
- **Time base:** all recorded events carry `CLOCK_MONOTONIC` microseconds. gpu-screen-recorder
  writes `<out>.mp4.ts` with `monotonic_microsec realtime_microsec` of the first frame. Video
  time `t = (event_us - first_frame_us) / 1e6`.
- **Coordinates:** Hyprland gives logical coords. Recording region is chosen in logical coords
  (slurp format). Recorded pixels are physical: `px = (logical - region.x) * monitor.scale`.
  Store scale and region in the bundle; convert once on import to *video pixel* space.
- **Theme:** read Omarchy accent like omacut: `~/.local/state/omarchy/current/theme/colors.toml`
  keys `accent`, `background`, `foreground` etc. Watch for changes (symlink swap). Fallback
  accent `#7aa2f7`.
- Runtime deps: `gpu-screen-recorder`, `ffmpeg`, `slurp`, `hyprctl`, `jq` (optional),
  `omarchy-notification-send` (optional). Build deps: qt6-base, qt6-declarative, qt6-multimedia,
  qt6-svg, qt6-shadertools, libevdev, cmake, ninja.

## 1. CLI

```
omarecord                       # open launcher (recent projects / open / record)
omarecord record [opts]         # toggle: start if not recording, else stop
    --fullscreen                # focused monitor
    --region                    # slurp pick (default: omarchy-capture-region smart if available, else slurp)
    --window                    # slurp with window rects (hyprctl clients) snapping
    --with-desktop-audio --with-microphone-audio
    --fps N (default 60)  --dir PATH (default $XDG_VIDEOS_DIR/omarecord)
    --no-open                   # don't launch editor after stop
    --stop                      # only stop; exit 1 if not recording
omarecord edit <bundle.omarecord>
omarecord export <bundle> -o out.mp4|out.gif [--preset ...] [--fps] [--width] [--quality]
omarecord probe <bundle>        # print json summary (duration, samples, clicks, zooms)
```

`record` is designed to be bound to a Hyprland key (toggle semantics, like
`omarchy-capture-screenrecording`). Recording state file: `$XDG_RUNTIME_DIR/omarecord/recording.json`
(pid, bundle path, started_us). On stop: SIGINT gsr, wait ≤5 s, finalize, write bundle,
notify via `omarchy-notification-send "Recording saved" --exec omarecord edit <bundle>` when
available, and (unless `--no-open`) spawn `omarecord edit <bundle>` detached.
Also call `omarchy-shell -q omarchy.indicators refresh` if present (it watches gsr pid).

## 2. Bundle format (`Name.omarecord/` directory)

```
screen.mp4          raw capture (no cursor), cfr
screen.mp4.ts       gsr first-frame timestamps (kept verbatim)
capture.json        { "version":1, "fps":60, "width":3840, "height":2160,
                      "region":{"x":1600,"y":0,"w":2400,"h":1350},   # logical
                      "scale":1.6, "monitor":"DP-3", "first_frame_us":182833061901,
                      "started_us":..., "stopped_us":..., "audio":{"desktop":false,"mic":false} }
input.jsonl         one event per line, monotonic us, already converted to VIDEO PIXEL coords:
                    {"t":123456,"k":"m","x":1204.8,"y":301.2}          move sample
                    {"t":...,"k":"d","b":"left"|"right"|"middle","x":..,"y":..}   button down
                    {"t":...,"k":"u","b":"left",...}                            button up
                    {"t":...,"k":"s","dx":0,"dy":-1,"x":..,"y":..}                 scroll (wheel steps)
                    {"t":...,"k":"kd","code":30,"name":"KEY_A","mods":["ctrl"]}   key down (no text)
                    {"t":...,"k":"ku","code":30}
project.json        editor state, see §3. Created on first open with defaults + auto zooms.
thumb.jpg           first-frame thumbnail for launcher.
```

`t` in input.jsonl is **raw monotonic us**; the reader subtracts `first_frame_us`.
Move samples before the first frame or after stop are still stored (trimmed on load).

## 3. Project model (`project.json`, version 1)

```jsonc
{
  "version": 1,
  "name": "Spreadsheet Demo",
  "aspect": "auto",            // auto | 16:9 | 4:3 | 1:1 | 9:16 | 4:5 | 3:2
  "crop": {"x":0,"y":0,"w":1,"h":1},   // normalized source crop
  "clips": [                   // ordered pieces of the source timeline (trim + split)
    {"id":"c1","in":0.0,"out":12.4,"speed":1.0}
  ],
  "zooms": [                   // in SOURCE time seconds
    {"id":"z1","start":2.1,"end":6.8,"level":2.0,"target":"auto"},        // auto = follow cursor
    {"id":"z2","start":8.0,"end":9.5,"level":1.5,"target":{"x":0.62,"y":0.4}} // fixed, normalized
  ],
  "zoomStyle": {"transitionIn":0.7,"transitionOut":0.7,"easing":"easeInOutCubic",
                "followSmoothing":0.85,"followDeadZone":0.12,"lookahead":0.0},
  "background": {"type":"wallpaper",          // wallpaper | gradient | color | image | none
                 "wallpaper":"omarchy:current" ,       // omarchy:current = theme bg, or file path
                 "gradient":{"angle":135,"stops":[["#ff8a00",0],["#e52e71",1]]},
                 "color":"#1a1b26", "image":null, "blur":0},        // blur 0..100
  "frame": {"padding":0.08,       // fraction of min(outW,outH), 0..0.3
            "radius":12,           // px at 1080p reference, scaled
            "shadow":{"enabled":true,"opacity":0.5,"blur":40,"offsetY":20},
            "inset":{"enabled":false,"width":0,"color":"#000000","alpha":0.5}},
  "cursor": {"visible":true,"size":1.5,"smoothing":0.8,       // 0=raw, 1=very smooth
             "clickEffect":"ripple",      // none | ripple | shrink | highlight
             "clickShrink":0.85,"hideWhenIdleMs":null,"style":"macos"},
  "audio": {"desktop":true,"mic":true,"volume":1.0},
  "export": {"format":"mp4","fps":60,"width":1920,"quality":"high","gif":{"fps":20,"width":960}}
}
```

Output size: derived from `aspect` + `export.width`; height = width / aspect (round to even).
For `auto`, aspect = cropped source aspect.

### Auto-zoom generation (on first open, and via "Regenerate zooms" action)

1. Collect click-down events (left/right) in video time.
2. Segment: a zoom starts `0.5 s` before a click and ends `1.75 s` after the last click in the
   group. Clicks closer than `2.0 s` apart join one group.
3. Drop segments shorter than `1.0 s` (extend end instead). Clamp to [0, duration].
4. Merge segments whose gap `< 0.75 s`.
5. Level `2.0`, target `auto`.
6. Key-only sessions (no clicks): no zooms.

### Zoom evaluation (pure, in `core/`, unit-tested)

Given source time `t`:
- `level(t)`: 1.0 outside segments. Inside `[start, end]`: ease in over `transitionIn` from 1 →
  level, hold, ease out over `transitionOut` to 1. If two segments overlap/abut, cross-fade
  levels (no dip to 1 between). Easing functions: `easeInOutCubic` (default), `easeOutExpo`,
  `linear`.
- `center(t)` (normalized 0..1 in cropped source space):
  - fixed target: the target.
  - auto: `c` follows the **smoothed cursor** with a dead-zone: keep `c` while the cursor is
    within `followDeadZone` (fraction of the visible viewport) of `c`; otherwise move `c`
    toward the cursor with exponential smoothing `followSmoothing` (per-frame alpha derived
    from a time constant so it's fps-independent, tau = 0.25 s at smoothing 0.85, tau→0 at 0).
    Clamp `c` so the viewport `[c - 1/(2z), c + 1/(2z)]` stays inside `[0,1]` on both axes.
  - During the ease-in, the center animates from the frame center toward target with the same
    easing; during ease-out back to frame center.
- Result per frame: `{scale, cx, cy}` in normalized cropped-source space.

### Cursor path (pure, unit-tested)

- Raw samples (video px, video time). Resample to output frame times by linear interpolation.
- Smoothing: critically damped spring toward the raw position; stiffness from `smoothing`
  (0 → snap, 0.8 → ~90 ms settle, 1.0 → ~200 ms). Never lags more than one frame at speed 0.
- Clicks: at a down event, scale the cursor to `clickShrink` over 80 ms and back over 160 ms;
  spawn a ripple (circle expanding from 0 → 60 px @1080p ref, fading over 450 ms, accent color).
- `hideWhenIdleMs`: if set and no movement for that long, fade cursor out over 300 ms; fade in
  on movement.

## 4. Renderer (parity between preview and export)

One QML component `Composition.qml` renders a frame given a `FrameSource` and `time`. It is used
by the editor preview (live) **and** by the exporter (offscreen via `QQuickRenderControl`). Do
not implement two renderers.

Layers (bottom → top):
1. Background: color / gradient / image (cover) with optional blur (`MultiEffect`).
2. Frame: the video rect, positioned with padding, rounded corners (`layer.enabled` + mask or
   `MultiEffect` maskSource), drop shadow (`MultiEffect` shadow).
3. Zoom: the *whole stage* (frame + cursor) is scaled about the zoom center. Implement as a
   transform on a container Item: `scale = level`, `transformOrigin` computed from `center`.
   Background stays static (the reference app behavior).
4. Cursor: SVG arrow (ship our own `assets/cursors/arrow.svg`, hotspot (0,0) at the tip),
   size = `24 * cursor.size` px at 1080p ref, scaled with output size. Ripple circles.

`FrameSource` is a C++ `QQuickItem` (`QSGSimpleTextureNode`) that displays the current
`QImage`/`QVideoFrame`. Preview feeds it from `QMediaPlayer` + `QVideoSink`. Export feeds it
from an ffmpeg rawvideo pipe (`-f rawvideo -pix_fmt rgba`), frame by frame, deterministic.

Exporter pipeline (`render/Exporter`):
- Decode: `ffmpeg -hwaccel cuda|auto -i screen.mp4 -vf select+fps -f rawvideo -pix_fmt rgba -`
  read frames sequentially for the clip ranges (seek with `-ss` per clip).
- For each output frame i at output time `T = i / fps`, map to source time via the clip list
  (`speed` respected), set `FrameSource` image, set `time`, render offscreen, `grabToImage`,
  write RGBA to encoder stdin.
- Encode MP4: `ffmpeg -f rawvideo -pix_fmt rgba -s WxH -r fps -i - [audio from screen.mp4 with
  trim/concat filter] -c:v h264_nvenc -preset p5 -cq <q> (fallback libx264 -crf) -pix_fmt yuv420p
  -movflags +faststart out.mp4`. Quality tiers: low/medium/high/best → cq 30/26/22/18.
- Encode GIF: two-pass palette: write frames to a temp raw file, then
  `ffmpeg -i raw -vf "fps,scale,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" -loop 0 out.gif`.
- Progress callbacks (frames done/total), cancel support.
- Headless `omarecord export` must work with `QT_QPA_PLATFORM=offscreen` fallback if no Wayland.

## 5. Editor UI (matches the the reference app layout in the reference screenshot)

- Window: dark, `Material.accent` = Omarchy accent. Min 1100×700.
- **Top bar:** project name (editable), undo/redo, "Presets" (save/load style presets to
  `~/.config/omarecord/presets/*.json`), **Export** button (accent).
- **Center:** preview canvas (Composition, letterboxed, keeps output aspect).
- **Right panel:** icon rail + panel. Sections: Background (Wallpaper | Gradient | Color |
  Image tabs, blur slider), Shape (padding, roundness, shadow, inset), Cursor (visible, size,
  smoothing, click effect, hide-when-idle), Zoom (default level, transition, follow smoothing,
  regenerate), Audio (desktop/mic toggles, volume), Export (format, fps, size, quality, gif
  opts). Wallpaper tab shows the Omarchy theme backgrounds
  (`~/.local/state/omarchy/current/theme/backgrounds/*`) as thumbnails plus "Pick file…".
- **Bottom toolbar:** aspect dropdown (Auto, Wide 16:9, 4:3, Square, Vertical 9:16, 4:5, 3:2),
  Crop toggle (drag handles on preview), transport (⏮ ▶/⏸ ⏭), split (scissors) at playhead,
  timeline zoom slider.
- **Timeline:** ruler; **Clip track** (accent-tinted blocks, waveform of audio if present via
  `ffmpeg -filter astats`-free approach: decode to pcm and draw peaks), trim handles at both
  ends, split at playhead creates two clips, delete selected clip, drag clip edges; **Zoom
  track** (blue blocks labelled "Zoom 2x · Auto"), drag to move, drag edges to resize, click to
  select → inspector shows level (1.25/1.5/2/2.5/3/4) and target (Auto / pick point on
  preview), delete key removes, double-click empty area adds a 2 s zoom.
- Playback: `QMediaPlayer` on screen.mp4, mapping output time ↔ source time through clips;
  skipping over trimmed regions; cursor + zoom overlay driven by player position.
- Keyboard: Space play/pause, ←/→ 1 frame, Shift+←/→ 1 s, S split, Delete, Ctrl+Z/Ctrl+Shift+Z,
  Ctrl+S save, Ctrl+E export, Z add zoom at playhead.
- Autosave project.json on every change (debounced 500 ms). Undo stack in C++ (command pattern
  or snapshot-based JSON diff; snapshots are fine — projects are tiny).

## 6. Code layout

```
CMakeLists.txt
src/main.cpp                 subcommand dispatch
src/core/   Project.{h,cpp}  JSON model, load/save, defaults, undo snapshots
            InputLog.{h,cpp} parse input.jsonl → samples/clicks/keys in video time
            CursorPath.{h,cpp} resample + spring smoothing + click anim state
            ZoomTimeline.{h,cpp} auto-zoom generation + level/center evaluation
            ClipTimeline.{h,cpp} output↔source time mapping, split/trim ops
            Theme.{h,cpp}    Omarchy colors.toml watcher
            Easing.h
src/record/ Recorder.{h,cpp} gsr process, ts file, bundle finalize, state file
            CursorSampler.{h,cpp} Hyprland IPC poller thread
            EvdevListener.{h,cpp} libevdev thread
            RegionPicker.{h,cpp} slurp / omarchy-capture-region wrapper, monitor math
src/render/ FrameSource.{h,cpp} QQuickItem texture node
            Exporter.{h,cpp} offscreen render + ffmpeg pipes
            FfmpegDecoder.{h,cpp} rawvideo pipe reader
            PreviewSink.{h,cpp} QVideoSink → FrameSource bridge
src/ui/     Editor.{h,cpp} QObject facade exposed to QML (project, playback, timeline ops)
            qml/Main.qml Composition.qml Timeline.qml ZoomTrack.qml ClipTrack.qml
                 SidePanel.qml panels/*.qml Launcher.qml
assets/     cursors/arrow.svg, icons (Lucide-style inline SVG), gradients.json
tests/      QtTest: test_zoomtimeline, test_cursorpath, test_cliptimeline, test_project,
            test_inputlog (fixtures in tests/fixtures)
pkg/        PKGBUILD, omarecord.desktop, omarecord.svg, install-omarchy script
docs/
```

## 7. Definition of done per phase

- **Phase 1 (core + recorder):** `cmake -B build -G Ninja && ninja -C build && ctest` green.
  `omarecord record --fullscreen` then `omarecord record` again produces a bundle with
  screen.mp4 (no cursor visible), capture.json, input.jsonl containing move + click samples in
  video px coords; `omarecord probe` prints counts and generated zoom count.
- **Phase 2 (renderer + export):** `omarecord export bundle -o out.mp4` renders padding,
  rounded frame, shadow, wallpaper background, smooth cursor, ripple, animated auto-zooms.
  `-o out.gif` works. Export of a 10 s 1080p60 clip finishes in < 30 s on NVENC.
- **Phase 3 (editor):** full UI above, trim/split, zoom editing, preview parity with export.
- **Phase 4 (Omarchy polish):** PKGBUILD + install script, desktop entry, notification with
  click-to-open, docs with Hyprland keybinding snippet, theme accent live-switching.

## 8. Amendments from the the reference app reverse-engineering (docs/reference-notes.md)

These override §3/§4 where they conflict. Phase 2 implements them in `core/` (with tests) and
in the renderer.

### 8.1 Motion is spring-driven and precomputed

the reference app does not tween zoom with a fixed duration/easing; every animated quantity is a
target fed to a damped spring. We do the same, and to keep preview and export identical **and**
allow random access (scrubbing), `core/MotionTrack` precomputes the whole recording once per
project change:

- Fixed integration step `1/240 s` from source time 0 to duration, semi-implicit Euler,
  state carried across steps. Output: arrays sampled at 240 Hz of
  `zoomScale, zoomCx, zoomCy, cursorX, cursorY, cursorScale, cursorOpacity, cursorRotation`.
- `MotionTrack::sample(t)` linearly interpolates between the two nearest steps.
- Recompute is cheap (a 10 min recording = 144k steps); run it on a worker thread, debounced,
  and swap atomically. The exporter always waits for a fresh track.

Spring parameters (per-project, editable in the Zoom/Cursor panels, defaults from the reference app):

```
screenMovementSpring   { mass: 2.25, stiffness: 200, damping: 40 }   // zoom scale + center
mouseMovementSpring    { mass: 3,    stiffness: 470, damping: 70 }   // cursor position
mouseClickSpring       { mass: 0.3,  stiffness: 300, damping: 30 }   // cursor scale on click
```

Spring form: `a = (stiffness * (target - x) - damping * v) / mass`.

`project.zoomStyle` becomes:

```jsonc
"zoomStyle": {"spring":{"mass":2.25,"stiffness":200,"damping":40},
              "snapToEdgesRatio":0.25,      // auto mode edge snapping, see 8.3
              "instantAnimation":false}
"cursor": { ..., "smoothing": true,  // uses mouseMovementSpring; false = raw
            "spring":{"mass":3,"stiffness":470,"damping":70},
            "clickShrink":0.8, "rotateOnXMovementRatio":0.5 }
```

Drop `transitionIn/transitionOut/easing/followSmoothing/followDeadZone/lookahead`. Keep
`Easing.h` only for UI/ripple animations.

### 8.2 Auto-zoom generation (replaces §3 rules)

For every button-down at video time `t` (seconds):

```
start = max(t - 0.3, 0)
end   = min(t + 2.5, duration - 0.8)
```

- Ignore clicks at or after `duration - 1.0`.
- Union overlapping ranges; merge ranges whose gap is ≤ 2.5 s.
- Level 2.0, target auto. Min editable zoom length 1.0 s; splits leave ≥ 0.1 s each side.
- Zoom level range is continuous 1.0 … 4.0 (slider), step 0.2 via keyboard.

### 8.3 Zoom target evaluation (replaces "dead-zone follow" in §3)

Zoom target for a range at time `t`, in normalized cropped-source coordinates:

- **manual**: `manualTargetPoint`, edge snapping ratio forced to 0.
- **auto**: take the cursor samples inside the range (if none, the nearest sample before/after
  its start). Walk them in time order accumulating a *group* while the group's bounding box
  still fits inside `50%` of the visible width and `70%` of the visible height at that zoom
  level (visible = 1/level of the cropped frame). A sample that no longer fits starts a new
  group. The target at time `t` is the bounding-box center of the group active at `t` (a group
  becomes active at its first sample time). This makes the camera hold while the cursor works
  in one area and glide when it moves away.
- Edge snapping (auto only): `maxRatio = visible/content` per axis; `r = min(snapToEdgesRatio,
  maxRatio)`; map each normalized target coordinate from `[r, 1 - r + 0.0001]` to `[0, 1]`
  with clamping. Finally clamp so the viewport stays inside the frame.
- Outside any range the target is scale 1, center (0.5, 0.5).
- The spring (8.1) chases `(scale, cx, cy)`; `instantAnimation` snaps within 0.1 s of a
  range boundary.

### 8.4 Cursor details

- Load-time decimation of raw samples: keep the first; drop a sample if it is within one frame
  window (`1/fps`) of the last kept one or moved < 1 px.
- Rotation: `clamp((x(t) - x(t - 0.4)) * 0.03 * rotateOnXMovementRatio, -20, 20)` degrees.
- Click: cursor scale target `0.8` from up to 130 ms **before** a button-down (we know the
  future) until button-up, via `mouseClickSpring`.
- Click effect `circle` (the reference app's default ring; our `ripple` name maps to it): ring of
  radius `16 * cursorSize` px @1080p, line width 2, alpha 0.6, lifetime 450 ms; scale
  `0.2 → 3.5` and alpha knots `[0, .05, .8, 1] → [0, 1, 0, 0]` over the first 150 ms … 450 ms.
  Ring color: black at 0.6 alpha (the reference app) — we use the Omarchy accent by default with a
  "ring color" option.
- `hideWhenIdleMs`: hidden style `{alpha 0, scale 0.8}`, pre-reveal 250 ms before the next move.

### 8.5 Frame/shape defaults

```
frame.padding 0.10 (ratio of the shorter output side), radius 12 @1080p,
shadow { enabled: true, intensity 0.75, blur 20, distance 25, angle 90 }   // replaces opacity/offsetY
inset  { enabled:false, width 0, color "#000000", alpha 0.5 }
cursor.size 1.5
background default: type "wallpaper", wallpaper "omarchy:current"
```

### 8.6 Aspect presets and export

Aspect: Auto (null), Wide 16:9, Square 1:1, Classic 4:3, Vertical 9:16, Tall 3:4, Portrait 4:5.

Export:

```
MP4: height 720 | 1080 | 2160, fps 60/50/30/25/24/20/10, quality studio|social|web-high|web-low
     bitrate = floor(w*h*fps*m), m: web-low .0075, web-high .0175, social .05, studio .3
     h264_nvenc -b:v <bitrate> -maxrate <bitrate*1.2> -bufsize <bitrate*2> -profile high, fallback libx264
GIF: height 480 | 720 | 1080, fps 50/30/25/20/15/10, quality studio (palettegen/paletteuse) |
     social (plain gif encoder), loop on/off, then gifsicle --optimize=3 --lossy=30 if installed.
Defaults: MP4 { fps 60, quality social, height 1080 }  GIF { fps 15, quality studio, height 480 }
```

Clip speed choices: 0.5 0.75 1 1.2 1.4 1.6 1.8 2 3 4 8 16 24 (stored as `speed`).
