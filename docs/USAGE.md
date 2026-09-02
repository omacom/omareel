# omarecord(1)

## Name

`omarecord` — record and edit polished Omarchy screen videos.

## Synopsis

```text
omarecord
omarecord record [--region|--fullscreen|--window] [options]
omarecord edit BUNDLE.omarecord
omarecord export BUNDLE.omarecord -o OUTPUT.mp4|OUTPUT.gif [options]
omarecord probe BUNDLE.omarecord
omarecord help
omarecord --version
```

## Description

With no command, OmaRecord opens its launcher. `record` is a toggle: it starts a recording when
idle and stops the current recording when one is active. Capture omits the system cursor and
stores raw input events beside the video so the editor and exporter can render cursor motion,
clicks, and zooms non-destructively.

## Commands

### `record`

`--region` opens Omarchy's smart region picker when available and otherwise uses `slurp`. This
is the default when no capture mode is given. `--window` selects a window-aligned region and
`--fullscreen` records the focused monitor. Only one mode may be supplied.

- `--with-desktop-audio` records system output.
- `--with-microphone-audio` records the default microphone.
- `--fps N` sets capture rate from 1 through 240; the default is 60.
- `--dir PATH` changes the bundle directory; the default is the `omarecord` directory under
  the user's XDG Videos directory.
- `--no-open` does not open the editor after the recording is finalized.
- `--no-bar` does not show the recording bar.
- `--stop` only stops; it returns status 1 when no recording is active.
- `--cancel` stops and permanently discards the in-progress bundle.

The Omarchy shell's live REC indicator watches `gpu-screen-recorder`. OmaRecord refreshes that
indicator on both start and stop. A stop that needs a force-kill or cannot finalize sends a
critical Omarchy notification.

## Stopping a recording

An active recording can be stopped in any of these ways:

- Click **Stop** on the recording bar.
- Click the REC indicator in the Omarchy bar.
- Run `omarecord record` or `omarecord record --stop` again; the toggle form is intended for
  keybinds.
- Run `omarecord record --cancel` to stop and discard the recording instead of saving it.

When two monitors are available, the recording bar is placed at the top center of the monitor
that is not being recorded. On a single-monitor setup, the bar remains accessible at the top
center but is visible inside the recording. Use `--no-bar` or set `OMARECORD_NO_BAR=1` on the
start command to suppress it.

### `edit BUNDLE`

Opens an `.omarecord` bundle. Changes autosave to `project.json` after 500 ms and may also be
saved immediately with `Ctrl+S`.

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
| `Delete` | Remove selected zoom or clip |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo/redo |
| `Ctrl+S` | Save |
| `Ctrl+E` | Export |

## Project bundle

The bundle directory contains `screen.mp4`, its `screen.mp4.ts` first-frame timestamp,
`capture.json`, `input.jsonl`, `project.json`, and `thumb.jpg`. Video remains unchanged while
clip, style, cursor, and zoom edits are stored in `project.json`.

## Environment

- `OMARECORD_DEBUG=1` appends `gpu-screen-recorder` stderr to `/tmp/omarecord.log`.
- `OMARECORD_NO_BAR=1` suppresses the recording bar.
- `OMARECORD_SCREENSHOT=/path/out.png` makes `omarecord edit` capture its own window with
  `QQuickWindow::grabWindow()` three seconds after loading, save the PNG, and quit. The launcher
  supports the same capture flag.
- `OMARECORD_SCREENSHOT_PANEL=background|shape|cursor|zoom|audio` selects an editor inspector
  before the debug capture.
- `OMARECORD_SCREENSHOT_SIZE=1440x900` sets the debug-capture window dimensions.
- `OMARECORD_RECENTS_DIR=/path` overrides the launcher recordings directory (useful for UI testing).

Example:

```sh
OMARECORD_SCREENSHOT=/tmp/editor.png \
OMARECORD_SCREENSHOT_PANEL=cursor \
OMARECORD_SCREENSHOT_SIZE=1440x900 \
omarecord edit Recording.omarecord
```

## Files

- `${XDG_RUNTIME_DIR}/omarecord/recording.json` stores active recording state.
- `${XDG_VIDEOS_DIR}/omarecord/` is the default bundle location.
- `/tmp/omarecord.log` is the opt-in recorder diagnostic log.

## Exit status

Status 0 means success, 1 is an expected no-recording/cancel condition or runtime failure, and
2 indicates invalid CLI usage, startup failure, or incomplete recording finalization.
