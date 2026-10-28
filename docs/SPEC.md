# omareel — engineering spec

A polished screen recorder + editor for Omarchy (Hyprland, Wayland, Arch).
Records the screen **without** the cursor, records raw mouse/keyboard input separately, then
renders a beautiful video: padded/rounded frame on a background, a smooth synthetic cursor,
click effects, and animated zoom-ins that follow the cursor. Exports MP4 or GIF.

This document is the frozen contract. Implementation runs are scoped to it.

## 0. Stack and non-negotiables

- **Language/UI:** C++17, Qt 6.11 (Quick/QML, Controls Material dark, Multimedia, Effects,
  Svg). Same stack as omacut/omasnap so it feels native on Omarchy. Build with **CMake + Ninja**.
  Single binary `omareel` with subcommands.
- **Capture:** prefer an in-process `ext-image-copy-capture-v1` output session when advertised.
  Create the session with options `0` so cursors are omitted, negotiate XRGB8888 shared memory,
  and rotate four buffers through capturing, queued, writing, and available states. A dedicated
  bounded writer feeds the encoder without blocking display dispatch and crops regions while
  copying their rows. Generate protocol bindings at build time. The established recorder remains
  the explicit and automatic startup fallback.
- **Cursor position:** on a dedicated thread with its own Wayland connection, listen to an
  `ext-image-copy-capture-v1` pointer cursor session on the recorded output (positions only, no
  cursor images). The compositor pushes `enter`/`leave`/`position` on monitor commits, in logical
  coordinates relative to the output; add the output's logical origin for global coords. Hyprland
  serves IPC on its main thread, so it is not polled while the session works. After `leave`, ask
  `j/cursorpos` on the Hyprland IPC socket
  (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`) once for where the pointer
  went. Without the protocol, with `ecosystem:enforce_permissions` on, or when the session stays
  silent with the pointer on the output (no `cursorpos` permission), poll `j/cursorpos` at the
  recording's frame rate (60–240 Hz). Polling also takes over mid-recording when the session
  misses moves: after each second without session events one `j/cursorpos` checks it, and two
  disagreements 100 ms apart end the session. Store a position when it moved, and at least 1 sample /
  250 ms as a heartbeat.
- **Buttons / scroll / keys:** libevdev on `/dev/input/event*` (user is in the `input` group on
  Omarchy). Open every device that has `EV_KEY` with `BTN_LEFT` or `EV_REL`/`REL_WHEEL` or
  keyboard keys. If no device can be opened, keep recording without click data and warn.
- **Encode/decode:** `ffmpeg`/`ffprobe` subprocesses. Never link libav directly.
- **Time base:** all recorded events carry `CLOCK_MONOTONIC` microseconds. Each capture backend
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
  qt6-svg, qt6-shadertools, libevdev, libxkbcommon, libjpeg-turbo, cmake, ninja.

## 1. CLI

```
omareel                       # open launcher (recent projects / open / record)
omareel record [opts]         # toggle: start if not recording, else stop
    --fullscreen                # focused monitor
    --region                    # slurp pick (default: omarchy-capture-region smart if available, else slurp)
    --window                    # slurp with window rects (hyprctl clients) snapping
    --with-desktop-audio --with-microphone-audio
    --with-webcam [--webcam-device=PATH] --no-webcam   # /dev/videoN or a /dev/v4l link
    --fps N (default 60)  --dir PATH (default $XDG_VIDEOS_DIR/omareel)
    --no-open                   # don't launch editor after stop
    --stop                      # only stop; exit 1 if not recording
omareel edit <bundle.omareel>
omareel export <bundle> -o out.mp4|out.gif [--preset ...] [--fps] [--width] [--quality]
omareel probe <bundle>        # print json summary (duration, samples, clicks, zooms)
```

Compatibility is intentionally narrow: the legacy command remains an alias, first-run settings
are copied from the legacy config directory, and legacy bundle directories remain readable.

`record` is designed to be bound to a Hyprland key (toggle semantics, like
`omarchy-capture-screenrecording`). Recording state file: `$XDG_RUNTIME_DIR/omareel/recording.json`
(pid, bundle path, started_us). On stop: set `stopping` in the state file (the bar and launcher
show "Saving…"), SIGINT gsr, wait ≤5 s, finalize, write bundle,
notify via `omarchy-notification-send "Recording saved" --exec omareel edit <bundle>` when
available, and (unless `--no-open`) spawn `omareel edit <bundle>` detached. The notification
instead says the recording was saved without audio when none could be muxed, or with a silent
microphone when ffmpeg's `volumedetect` saw only zero samples from it (a real microphone always
picks up some noise). A microphone missing from `pactl list short sources` at start is replaced
by the default one, and the start notification says so.
Also call `omarchy-shell -q omarchy.indicators refresh` if present (it watches gsr pid).

## 2. Bundle format (`Name.omareel/` directory)

```
screen.mp4          raw capture (no cursor); each frame at its capture time on the 1/fps grid
screen.mp4.ts       first-frame timestamps
camera.mp4          optional webcam capture, without audio
camera.mp4.ts       webcam first-frame timestamps
capture.json        { "version":1, "backend":"ext-image-copy-capture", "fps":60,
                      "width":3840, "height":2160,
                      "region":{"x":1600,"y":0,"w":2400,"h":1350},   # logical
                      "scale":1.6, "monitor":"DP-3", "first_frame_us":182833061901,
                      "started_us":..., "stopped_us":..., "audio":{"desktop":false,"mic":false} }
input.jsonl         one event per line, monotonic us, already converted to VIDEO PIXEL coords:
                    {"t":123456,"k":"m","x":1204.8,"y":301.2}          move sample
                    {"t":...,"k":"d","b":"left"|"right"|"middle","x":..,"y":..}   button down
                    {"t":...,"k":"u","b":"left",...}                            button up
                    {"t":...,"k":"s","dx":0,"dy":-1,"x":..,"y":..}                 scroll (wheel steps)
                    {"t":...,"k":"kd","code":16,"name":"KEY_Q","label":"A","mods":["ctrl"]}   key down
                    {"t":...,"k":"ku","code":16}
project.json        editor state, see §3. Created on first open with defaults + auto zooms.
thumb.jpg           first-frame thumbnail for launcher.
```

`t` in input.jsonl is **raw monotonic us**; the reader subtracts `first_frame_us`. Device events
carry the kernel's own `CLOCK_MONOTONIC` time of the event, not the time it was read.

Keys are only shortcuts: modifier keys, keys pressed with Ctrl, Alt or Super held, and keys that
type no text (Enter, Escape, Tab, Backspace, Delete, arrows, Home/End/Page Up/Page Down, F-keys,
Insert, media keys...), with or without Shift. Plain typing, Shift+letter included, is dropped
when it is read and never reaches the bundle, so passwords typed while recording are not saved; a
key up is kept only after its key down was. `code` and `name` are the evdev scancode and its
libevdev name (a US key); `label` is what the main keyboard's active Hyprland layout prints on the
key at its base level (AZERTY's `KEY_Q` is "A"), following Hyprland's layout switches. What is
typing is decided by every keyboard's keymap Hyprland lists, all their layouts at once: a key is
typing if it types or picks text in any of them, and a modifier only if it is that modifier in
all of them (Right Alt in "us,fr" is neither Alt nor a shortcut). When Hyprland does not list its
keyboards or a keymap does not compile, nothing is labelled and the US names decide, with Right
Alt as AltGr. Events read together from several devices are handled in time order, and a
`SYN_DROPPED` resync is read so lost key releases are not missed. Older bundles may have plain
typing and no `label`; the keystroke overlay hides their typing and names keys by `name`, and
takes their "alt" as Alt only while Left Alt is down (the old recorder named AltGr "alt" too).
Move samples before the first frame or after stop are still stored (trimmed on load).

### Webcam capture

When enabled, the recording bar owns a Qt Multimedia capture session. One camera stream feeds both
the live bar preview and a bounded ffmpeg pipe worker, so the device is opened only once. The bar is also launched
as a hidden capture host for `--no-bar`; this keeps camera ownership in an existing GUI process
without turning the recording daemon into a GUI application. The first frame admitted for recording records the
monotonic timestamp in `camera.mp4.ts`. Some cameras take seconds to start streaming, so the bubble
says "Starting camera…" until the first frame; if none arrives within ten seconds, the bar releases
the device and the recording continues without the camera. Settings name the camera by its
`/dev/v4l/by-id` link, or its `by-path` link when another node shares the by-id name (a laptop
webcam's IR sensor has the same USB serial), because `/dev/videoN` numbers follow enumeration
order and change when a dock reconnects. `webcam.chosenDevice` keeps the camera last picked in
the launcher while `webcam.device` names the one in use. `capture.json.camera.backend` records
`ffmpeg-pipe`, along with the device, dimensions, frame rate, first-frame
timestamp, rotation, and horizontal flip. Rotation and flip affect previews and rendering but are
not baked into `camera.mp4`.

The worker repacks raw planes (or decodes MJPG directly to planar YUV with libjpeg-turbo)
and uses the cached encoder probe to select NVENC or libx264. Monotonic deadlines pace
30 fps CFR output with no B-frames. A single pending frame replaces superseded input;
slower input repeats the latest packed frame. Pipe writes time out after two seconds,
and encoder shutdown has a five-second deadline. Qt Multimedia remains the preview backend.
A standby host exits when its owner is gone or quit is requested, after any active
recording finishes. A live owner keeps the host in standby for the next recording.

The self-view starts at the persisted bottom-right position on the recorded monitor and remains
draggable with S, M, and L sizes. The bundled Hyprland plugin keeps the bar and self-view out
of the capture: their layers are skipped in the compositor scene while a capture session is
active and drawn onto the display after the mirror copy, so the file sees the live desktop
beneath them. Without a matching plugin, the overlays move to a non-recorded monitor or are
hidden on a single monitor. For in-process audio, a separate pulse capture is timestamped from the
monotonic clock and copy-muxed with the video at stop.

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
            "shadow":{"enabled":true,"intensity":0.25,"blur":40,"distance":10,"angle":90},
            "border":{"enabled":false,"width":7,"color":"#000000","alpha":1.0}},
  "cursor": {"visible":true,"size":1.5,"smoothing":0.8,       // 0=raw, 1=very smooth
             "clickEffect":"ripple",      // none | ripple | shrink | highlight
             "clickShrink":0.85,"hideWhenIdleMs":null,"style":"macos"},
  "audio": {"desktop":true,"mic":true,"volume":1.0},
  "camera": {"enabled":false,"position":"bottom-right","size":0.25,
             "shape":"round",       // round | square | rectangle (the webcam's own aspect)
             "radius":16,           // corners of square and rectangle, px at 1080p reference
             "flipHorizontal":false,"rotation":0,
             "shadow":{"enabled":true,"intensity":0.28,"blur":23,"distance":20,"angle":45},
             "border":{"enabled":true,"width":6,"color":"#ffffff","alpha":0.7},
             "scaleDuringZoom":0.7,
             "offset":{"x":0.035,"y":0.035}},  // gap to the border's outside, × the shorter side
  "export": {"format":"mp4","fps":60,"width":1920,"quality":"high","gif":{"fps":20,"width":960}}
}
```

Projects before `defaultsVersion` 3 kept the camera's `crop` (square | original) apart from a
`round | rounded | square` shape whose only difference was the corners; loading folds the crop into
`shape` and turns the old sharp `square` into a radius of 0. Before `defaultsVersion` 4 the camera's
`offset` was a fraction of the canvas width (x) and height (y), so the gap at the bottom was smaller
than at the side; untouched old camera shadow, border width, and offset values move to the new
defaults, and the shadow and border stay switched as they were.

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

`FrameSource` is a C++ `QQuickItem` (`QSGSimpleTextureNode`) used by deterministic export frames.
Interactive preview uses the multimedia scene-graph video item directly, with crop and rounded
masking on the GPU. A precise 60 Hz timer samples playback position for composition bindings;
camera correction seeks only when drift exceeds 250 ms.

Exporter pipeline (`crates/omareel_qt/cpp/Exporter`, over `crates/omareel_core/src/export`):
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
- Headless `omareel export` must work with `QT_QPA_PLATFORM=offscreen` fallback if no Wayland.

## 5. Editor UI (matches the the reference app layout in the reference screenshot)

- Window: dark, `Material.accent` = Omarchy accent. Min 1100×700.
- **Top bar:** project name (editable), undo/redo, "Presets" (save/load style presets to
  `~/.config/omareel/presets/*.json`), **Export** button (accent).
- **Center:** preview canvas (Composition, letterboxed, keeps output aspect).
- **Right panel:** icon rail + panel. Sections: Background (Wallpaper | Gradient | Color |
  Image tabs, blur slider), Shape (padding, roundness, shadow, outside border), Cursor (visible, size,
  smoothing, click effect, hide-when-idle), Zoom (default level, transition, follow smoothing,
  regenerate), Audio (desktop/mic toggles, volume), Export (format, fps, size, quality, gif
  opts). Wallpaper tab shows the Omarchy theme backgrounds
  (`~/.local/state/omarchy/current/theme/backgrounds/*`) as thumbnails plus "Pick file…".
- **Bottom toolbar:** aspect dropdown (Auto, Wide 16:9, 4:3, Square, Vertical 9:16, 4:5, 3:2),
  Crop toggle (drag handles on preview), transport (⏮ ▶/⏸ ⏭), split (scissors) at playhead,
  timeline zoom slider.
- **Timeline:** ruler; **Clip track** (source-frame thumbnails), trim handles at both
  ends, split at playhead creates two clips, delete selected clip, drag clip edges; **Zoom
  track** (blue blocks labelled "Zoom 2x · Auto"), drag to move, drag edges to resize, click to
  select → inspector shows level (1.25/1.5/2/2.5/3/4) and target (Auto / pick point on
  preview), delete key removes, double-click empty area adds a zoom of up to 2 s there, fitted
  before the next zoom (at least 1 s; zooms never overlap) and selected.
  Either wheel axis zooms around the pointer from fit-to-view to 16×; right-button drag pans.
  The 12 px scrollbar stays visible whenever content overflows.
- Playback: `QMediaPlayer` on screen.mp4, mapping output time ↔ source time through clips;
  skipping over trimmed regions; cursor + zoom overlay driven by player position.
- Keyboard: Space play/pause, ←/→ 1 frame, Shift+←/→ 1 s, S split, Delete selection,
  Backspace selected range, Ctrl+Z/Ctrl+Shift+Z, Ctrl+S save, Ctrl+E export,
  Z add zoom at playhead.
- Autosave project.json on every change (debounced 500 ms). Undo stack in C++ (command pattern
  or snapshot-based JSON diff; snapshots are fine — projects are tiny).

## 6. Code layout

```
CMakeLists.txt
src/main.cpp                 subcommand dispatch, fonts, the editor, launcher and record-bar windows
src/render/ FrameSource.{h,cpp} QQuickItem texture node
            VideoDecoding.{h,cpp} keeps playback off NVIDIA's VA-API driver
src/plumbing/ PlaybackDriver.{h,cpp} the editor's three QMediaPlayers
            ExportRenderer.{h,cpp} offscreen Composition.qml rendering for export
            CameraCapture.{h,cpp} the webcam: QCamera, frame packing, ffmpeg encoder thread
            ThemeBase.{h,cpp} theme file watcher; QtShims.{h,cpp} Qt calls the Rust side makes
crates/omareel_core/src  project (JSON model, load/save, defaults, migrations), input_log
            (input.jsonl → samples/clicks/keys in video time), cursor_path, motion_track
            (spring smoothing, click ripples), zoom_timeline (auto-zoom generation, targets),
            clip_timeline (output↔source time mapping, split/trim ops), camera_timeline,
            layout_timeline, keystroke_track, easing, theme (Omarchy colors.toml),
            omarchy_paths (Omarchy state, wallpapers, recordings directory),
            recording_preferences (settings.json), microphone_meter, compat (Qt's rules)
crates/omareel_core/src/editor  the editor's document: project, history, timeline ops,
            playback decisions (crates/omareel_core/src/export: plan, ffmpeg decode/encode pipes)
crates/omareel_record/src  recorder (daemon, gsr process, ts files, bundle finalize, state
            files), cli (`omareel record` and the IPC commands), screen_capture (Wayland
            capture, encoder), cursor_sampler (Hyprland IPC cursor poller), evdev_listener
            (libevdev thread), region_picker (slurp / omarchy-capture-region wrapper and monitor
            math), capture_exclusion, camera, launcher and recording_bar decisions
crates/omareel_qt/src/editor.rs the `editor`/`comp` object QML uses (CXX-Qt), over
            crates/omareel_qt/cpp/EditorBase.{h,cpp} (players, timers) and Exporter.{h,cpp}
            (offscreen render loop over the Rust export job)
crates/omareel_qt/src/launcher.rs, recording_bar.rs  the `launcher` and `recordingBar` objects,
            over crates/omareel_qt/cpp/LauncherBase and RecordingBarBase (devices, camera,
            timers, watcher); record.rs hands main.cpp's recording commands to omareel_record
src/ui/     qml/Main.qml Composition.qml Timeline.qml ZoomTrack.qml ClipTrack.qml
                 SidePanel.qml *Panel.qml Launcher.qml RecordingBar.qml ui/Om*.qml
            CaptureTestClient.{h,cpp} the capture exclusion test windows
plugin/     the Hyprland capture exclusion plugin
assets/     cursors/arrow.svg, icons (Lucide-style inline SVG), gradients.json
tests/      QtTest: test_editor, test_exporter_smoke, test_cameracapture (fixtures in
            tests/fixtures); the Rust crates' tests and the goldens in tests/golden run as
            ctest's rust test
pkg/        PKGBUILD, omareel.desktop, omareel.svg, install-omarchy script
docs/
```

## 7. Definition of done per phase

- **Phase 1 (core + recorder):** `cmake -B build -G Ninja && ninja -C build && ctest` green.
  `omareel record --fullscreen` then `omareel record` again produces a bundle with
  screen.mp4 (no cursor visible), capture.json, input.jsonl containing move + click samples in
  video px coords; `omareel probe` prints counts and generated zoom count.
- **Phase 2 (renderer + export):** `omareel export bundle -o out.mp4` renders padding,
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
shadow { enabled: true, intensity 0.25, blur 40, distance 10, angle 90 }
border { enabled:false, width 7, color "#000000", alpha 1.0 } // outside the video edge
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

## 9. Clip layouts

These extend §3 and §4.

### 9.1 Model

```jsonc
"clips": [
  {"id":"intro","in":0.0,"out":8.2,"speed":1.0,"layout":"camera"},   // webcam full frame
  {"id":"c1","in":8.2,"out":95.0,"speed":1.0},                        // "default": screen + bubble
  {"id":"c2","in":95.0,"out":110.0,"speed":1.0,"layout":"screen"}     // screen, no bubble
],
"camera": { ..., "fullFrame": "fill" }   // fill | square
```

`layout` is written only when it is not `default`, so projects without layouts are unchanged.
Splitting a clip copies its layout to both halves.

### 9.2 Evaluation (`core/LayoutTimeline`, pure, unit-tested)

`sampleLayout(clips, outputTime, cameraAvailable, bubbleEnabled)` returns `cameraFull` (0 bubble,
1 full frame) and `bubble` (bubble visibility). Targets: `camera` → {1, 0}; `screen` → {0, 0};
`default` → {0, camera.enabled}; without a camera every layout is {0, 0}. Clip `k` eases from
clip `k - 1`'s target to its own over its first `min(0.5 s, clip output length)` with
easeInOutCubic, so every clip ends on its target. Layouts use output time, so transitions are
the same across cuts and speed changes.

### 9.3 Rendering

`CameraOverlay` interpolates position, size and corner radius from the bubble rect to the full
rect by `cameraFull`; border and shadow fade with it. The full rect is the canvas (`fill`) or the
largest centred square (`square`); `square` adds a blurred copy of the camera behind it
(`BlurredBackdrop`, a 24 px capture blurred and scaled to cover; a dark fill under software
rendering). The camera is cover-cropped to its current rect. Opacity is `min(1, cameraFull +
bubble)`, so no layout change dips the camera. Once `cameraFull` reaches 1 the background, stage
and keystrokes are not drawn. A camera layout shows the camera's first frame before the camera
started. Export decodes the camera whenever a clip uses the camera layout, at a size that covers
the output.

Preview: the camera player is re-seeked when it drifts more than 250 ms from the screen, or more
than 80 ms while a camera layout is on screen.

### 9.4 Inserted videos

```jsonc
{"id":"m-…","in":0.0,"out":14.2,"speed":1.0,"media":"media/demo.mp4","fit":"fit"}  // fit | fill | framed
```

`media` is bundle-relative and must stay inside `media/` (`sanitizedMediaPath`); a clip whose
path escapes it is dropped on load. Clips without `media` are pieces of the recording, which stay
in recording order: a recording clip's in/out move between the nearest recording clips
(`ClipTimeline::recordingBounds`), an inserted clip's within its video. At least one recording
clip always remains. Zooms, the motion track, keystrokes and the camera use recording time
(`ClipTimeline::recordingSourceTime`, which inside an inserted clip is the recording time it
interrupts), and `outputRange` never maps recording time into an inserted clip.

Inserting probes the file (`media_probe` in omareel_core: display size after rotation and sample aspect,
video-stream duration), reflinks it (`FICLONE`) or copies it to `media/.name.part`, renames it
to a unique name, then splits the clip at the playhead and inserts the new clip under one undo
snapshot. Undo never deletes media files.

Rendering: `MediaLayer` draws the inserted clip with its own `RoundedFrame` (FrameSource
`mediaFrameSource`, framed only for `framed`) and, for `fit`, a `BlurredBackdrop`. While it is
on screen the stage, camera and keystrokes are hidden; for `fit` and `fill` the background too.
Export decodes each clip from its own file, scaled to cover the output. Audio is one segment
per clip in output order: the recording (gated by the desktop/mic toggles), the inserted file
opened at its in point (`-ss in -t length -i`), or `anullsrc` silence; each is resampled to
48 kHz stereo, tempo-adjusted, padded and trimmed to the clip's exact output length, then
concatenated. Click sounds skip inserted clips. A missing inserted file fails the export.

Preview: a second `QMediaPlayer` plays inserted clips. `playing` is the transport's state, not a
player's, so handing playback from one player to the other (the incoming one starts before the
outgoing one pauses) does not toggle it. While a recording clip is active the inserted clip after
it is loaded and paused on its first frame, and during an inserted clip the screen player waits
where the recording resumes. A player's end of media also ends its clip. The media layer stays
hidden until the loaded file has delivered a frame.

