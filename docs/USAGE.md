# omareel(1)

## Name

`omareel` — record and edit polished Omarchy screen videos.

## Synopsis

```text
omareel
omareel record [--region|--fullscreen|--window] [options]
omareel edit BUNDLE.omareel
omareel export BUNDLE.omareel -o OUTPUT.mp4|OUTPUT.gif [options]
omareel probe BUNDLE.omareel
omareel help
omareel --version
```

## Description

With no command, Omareel opens its launcher. `record` is a toggle: it starts a recording when
idle and stops the current recording when one is active. Capture omits the system cursor and
stores raw input events beside the video so the editor and exporter can render cursor motion,
clicks, and zooms non-destructively.

## Commands

### `record`

With no capture mode, `omareel record` uses one smart gesture: drag to select an area, click
a window to snap to it, or click the desktop to record the whole screen. The explicit
`--region`, `--window`, and `--fullscreen` modes remain available for dedicated keybinds. Only
one mode may be supplied.

- With no audio flag, capture uses the saved launcher preferences (system audio and microphone
  both default on).
- `--with-desktop-audio` records only system output unless the microphone flag is also present.
- `--with-microphone-audio` records only the saved microphone device unless the system flag is
  also present.
- `--no-audio` records no audio and overrides the saved preferences.
- `--fps N` sets capture rate from 1 through 240; the default is 60.
- `--webcam-height 720|1080` sets webcam capture resolution; the saved launcher preference
  defaults to 1080p.
- `--no-selfview` records the webcam without showing the live camera self-view.
- `--dir PATH` changes the bundle directory; the default is the `omareel` directory under
  the user's XDG Videos directory.
- `--no-open` does not open the editor after the recording is finalized.
- `--no-bar` does not show the recording bar.
- `--stop` only stops; it returns status 1 when no recording is active.
- `--cancel` stops and permanently discards the in-progress bundle.

The shell's live REC indicator recognizes the fallback capture process. Omareel refreshes the
indicator on both start and stop and always maintains its own recording state. On installations
where the indicator only checks that process name, it may remain off during in-process capture.
A stop that cannot finalize sends a critical notification.

## Stopping a recording

An active recording can be stopped in any of these ways:

- Click **Stop** on the recording bar.
- Click the REC indicator in the Omarchy bar.
- Run `omareel record` or `omareel record --stop` again; the toggle form is intended for
  keybinds.
- Run `omareel record --cancel` to stop and discard the recording instead of saving it.

The recording bar is placed at the top center of the recorded monitor. In-process capture omits
the bar from the video. Use `--no-bar` or set `OMAREEL_NO_BAR=1` to suppress it.

## Launcher window

The launcher opens at 380×460 and grows to 380×580 while its webcam preview is visible. Its
Wayland app id is `omareel`. This optional Omarchy rule keeps it floating, centred,
and at the intended default size:

```lua
o.window("^omareel$", { float = true, center = true, size = "380 460" })
```

## Camera self-view

When webcam capture is enabled, the camera self-view is a separate 160 px overlay by default.
Choose S, M, or L in launcher settings and drag the bubble to persist its position. It starts at
the bottom-right of the recorded monitor and may be moved anywhere on that monitor. In-process
capture omits both the self-view and recording bar without showing a picker. The recording bar
can hide or show the self-view while recording.

The native backend is selected automatically when its display protocol is available. It uses four
shared-memory capture slots and a bounded encoder queue, and crops regions on its writer thread.
Set `OMAREEL_CAPTURE=native` to require this selection or `OMAREEL_CAPTURE=gsr` to force the
fallback backend for one invocation. The same values may be stored as `captureBackend` in the
settings file. The fallback cannot omit overlays placed inside its capture area.

### `edit BUNDLE`

Opens an `.omareel` bundle. Changes autosave to `project.json` after 500 ms and may also be
saved immediately with `Ctrl+S`.

Legacy `.omarecord` bundles remain supported, including bundles under
`~/Videos/omarecord`. The legacy command name is installed as a compatibility alias. If only
legacy settings exist, they are copied to `~/.config/omareel/settings.json` on first run and the
original file is retained.

### `export BUNDLE -o OUTPUT`

Exports `.mp4` or `.gif`. Common options are `--fps N`, `--width W`, and `--quality LEVEL`.
Quality may be `low`, `medium`, `high`, `best`, `web-low`, `web-high`, `social`, or `studio`.
GIF-specific overrides are `--gif-fps N` and `--gif-width W`.
Add `--timing` to print average decode wait, upload, render, readback, encoder-write, and
end-to-end throughput measurements.

### `probe BUNDLE`

Prints JSON containing source duration, frame rate, dimensions, input-event counts, and the
auto-generated zoom summary.

## Editor keys

| Key | Action |
| --- | --- |
| `Space` | Play/pause |
| `Left`, `Right` | Previous/next frame |
| `Shift+Left`, `Shift+Right` | Back/forward one second |
| `S` | Split at playhead |
| `Z` | Add a two-second zoom |
| `Delete` | Remove all selected zooms, or the selected clip |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo/redo |
| `Ctrl+S` | Save |
| `Ctrl+E` | Export |

The timeline scrolls horizontally with either wheel axis (80 px per mouse-wheel notch), and
touchpad horizontal or vertical scrolling. Ctrl+wheel zooms around the pointer.

## Project bundle

The bundle directory contains `screen.mp4`, its `screen.mp4.ts` first-frame timestamp,
`capture.json`, `input.jsonl`, `project.json`, and `thumb.jpg`. Video remains unchanged while
clip, style, cursor, and zoom edits are stored in `project.json`.

## Environment

- `OMAREEL_DEBUG=1` appends `gpu-screen-recorder` stderr to `/tmp/omareel.log`.
- `OMAREEL_PREVIEW_STATS=1` plays an opened recording and logs preview frame rate, average
  frame handling cost, and drops once per second.
- `OMAREEL_CAPTURE=gsr` forces the fallback screen capture backend.
- `OMAREEL_CAPTURE=native` selects the native screen capture backend when supported.
- Native recordings mask the private recording bar and self-view rectangles with desktop pixels
  captured before those overlays appear. The fallback backend records overlays normally.
- `OMAREEL_NATIVE_CONVERSION=cpu` forces CPU colour conversion for native capture. By default,
  the hardware conversion filter is exercised with a BGRA frame first and CPU conversion is used
  automatically when that exact conversion is unsupported.
- `OMAREEL_NO_BAR=1` suppresses the recording bar.
- `OMAREEL_SCREENSHOT=/path/out.png` makes `omareel edit` capture its own window with
  `QQuickWindow::grabWindow()` three seconds after loading, save the PNG, and quit. The launcher
  supports the same capture flag.
- `OMAREEL_SCREENSHOT_PANEL=background|shape|cursor|zoom|clip|camera|keystrokes|audio` selects an editor inspector
  before the debug capture.
- `OMAREEL_SCREENSHOT_SIZE=1440x900` sets the debug-capture window dimensions.
- `OMAREEL_SCREENSHOT_HOVER_HANDLE=clip-right|zoom-left` forces the selected block's named
  trim handle into its hovered visual; `OMAREEL_SCREENSHOT_DRAG_HANDLE` selects its dragging visual.
- `OMAREEL_SCREENSHOT_SEEK=SECONDS` seeks the editor before capture
  (`OMAREEL_SCREENSHOT_TIME` remains accepted for compatibility).
- `OMAREEL_SCREENSHOT_PLAY_TO_END=1` first plays the last 1.5 s of the clip so the player reaches
  its end-of-media state before the seek; `OMAREEL_SCREENSHOT_NO_PAUSE=1` seeks without pausing
  first, the way a playhead drag does. Together they reproduce scrub-after-end rendering.
- `OMAREEL_SCREENSHOT_PROJECT_VALUES=JSON` applies path/value pairs through the editor before
  capture, for example `{"camera.rotation":180}`.
- `OMAREEL_SCREENSHOT_VIEW=export|aspect|background-expanded|background-gradient-3|background-gradient-7|rail-tooltip|camera-proof` opens a transient
  editor surface before capture.

Example:

```sh
OMAREEL_SCREENSHOT=/tmp/editor.png \
OMAREEL_SCREENSHOT_PANEL=cursor \
OMAREEL_SCREENSHOT_SIZE=1440x900 \
omareel edit Recording.omareel
```

## Files

- `${XDG_RUNTIME_DIR}/omareel/recording.json` stores active recording state.
- `${XDG_CONFIG_HOME}/omareel/settings.json` stores launcher recording preferences.
- `${XDG_VIDEOS_DIR}/omareel/` is the default bundle location.
- `/tmp/omareel.log` is the opt-in recorder diagnostic log.

## Exit status

Status 0 means success, 1 is an expected no-recording/cancel condition or runtime failure, and
2 indicates invalid CLI usage, startup failure, or incomplete recording finalization.
