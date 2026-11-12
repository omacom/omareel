# omareel(1)

## Name

`omareel` — record and edit polished Omarchy screen videos.

## Synopsis

```text
omareel
omareel record [--region|--fullscreen|--window] [options]
omareel edit BUNDLE.omareel
omareel export BUNDLE.omareel -o OUTPUT.mp4|OUTPUT.gif|OUTPUT.srt|OUTPUT.vtt [options]
omareel transcribe BUNDLE.omareel [--backend NAME] [--force]
omareel probe BUNDLE.omareel
omareel describe BUNDLE.omareel
omareel apply BUNDLE.omareel OPS.json|- [--dry-run] [--describe]
omareel skill [install]
omareel help
omareel --version
```

## Description

With no command, Omareel opens its launcher. `record` is a toggle: it starts a recording when
idle and stops the current recording when one is active. Capture omits the system cursor and
stores raw input events beside the video so the editor and exporter can render cursor motion,
clicks, and zooms non-destructively. Hyprland draws a software cursor into the screen image,
where capture would record it, and by default uses one on NVIDIA whenever a monitor has VRR on;
while recording, Omareel sets `cursor:no_hardware_cursors` to 0 and restores it afterwards. If
the recorder is killed before it can, the previous value is kept in
`$XDG_RUNTIME_DIR/omareel/hardware-cursor-hold-<instance>.json` (one per Hyprland instance) and
restored the next time the launcher or a recording starts on that instance (or by
`omareel record --stop` when it has to kill a stuck recorder).

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
  also present. If that microphone isn't plugged in, the default one records instead and the
  start notification says so.
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
A stop that cannot finalize sends a critical notification. The "Recording saved" notification
says so when the recording has no audio, or when the microphone recorded only digital silence
(it is muted, or the audio server stopped delivering it).

## Stopping a recording

An active recording can be stopped in any of these ways:

- Click **Stop** on the recording bar. It reads **Saving…** until the recording is finalized.
- Click the REC indicator in the Omarchy bar.
- Run `omareel record` or `omareel record --stop` again; the toggle form is intended for
  keybinds.
- Run `omareel record --cancel` to stop and discard the recording instead of saving it.

The recording bar is placed at the top center of the recorded monitor. In-process capture omits
the bar from the video. Use `--no-bar` or set `OMAREEL_NO_BAR=1` to suppress it.

## Launcher window

The launcher opens at 380×460 and grows to 380×520 while its floating self-view controls are visible. Its
Wayland app id is `omareel`. It needs no window rule: a toplevel whose minimum and maximum width
match is one Hyprland floats on its own, so the launcher's fixed width keeps it floating and
centred while its height follows the self-view controls.

While the microphone is on, its row shows a level meter, so a muted microphone or a stuck audio
server shows up before recording rather than as a silent video. The microphone and webcam menus
follow devices as they are plugged in and pulled out. An unplugged device stays chosen and is
used again once it is back; meanwhile the default microphone or the first camera stands in.

## Camera self-view

Turning on the webcam in the launcher immediately opens a floating camera self-view on the
focused monitor. Choose S, M, or L in launcher settings and drag the bubble to place it before
recording; its position is saved. Rotate, flip, resize, or hide it from the launcher. The same
warm camera stays through recording, with no camera reconnect. With the exclusion plugin, the
bubble stays in place without a startup blink. Closing the launcher closes its idle self-view;
an adopted host returns to standby after recording. See “Overlays and capture” for fallback placement.

The bubble is 160 px by default and starts at the saved position (bottom-right initially).
Native capture omits both the self-view and recording bar. The recording bar can hide or show
the self-view while recording; a standby bubble hides during recordings that do not adopt it.

The native backend is selected automatically when its display protocol is available. It has the
compositor render frames into dmabufs on its GPU and, except on NVIDIA, crops, converts and encodes
them there with VA-API, so captured frames never reach the CPU. On NVIDIA, or when VA-API can't
import the buffers, linear dmabufs are read back by Omareel and encoded by ffmpeg (NVENC or
libx264). Shared-memory capture, which makes the compositor read every frame back on its render
thread, is the last resort. Recordings are fragmented MP4, playable up to the last second even if
the recorder is killed.
Set `OMAREEL_CAPTURE=native` to require this selection or `OMAREEL_CAPTURE=gsr` to force the
fallback backend for one invocation. The same values may be stored as `captureBackend` in the
settings file. The fallback cannot omit overlays placed inside its capture area.

### `edit BUNDLE`

Opens an `.omareel` bundle. Changes autosave to `project.json` after 500 ms and may also be
saved immediately with `Ctrl+S`. When another program writes `project.json` while the editor
is open (`omareel apply`, an AI agent, a text editor), the editor takes the new project in
within half a second as one undoable change; edits made in the editor just before are one
`Ctrl+Z` away. A file that does not parse yet is ignored until it does.

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

An output ending in `.srt` (SubRip) or `.vtt` (WebVTT) writes the recording's captions instead,
on the edited timeline: cut parts are left out and sped-up clips shorten their captions. It needs
`transcript.json` (see `transcribe`). MP4 and GIF exports draw the captions into the video when
they are turned on in the editor.

### `frame BUNDLE --at SECONDS -o OUTPUT`

Renders frames of the export as images, without exporting the video: the same composition
(background, zoom, cursor, camera bubble, keystrokes, inserted videos) from the same decoded
pictures, so a still matches the export's frame at that moment. Times are on the edited video's
timeline, after cuts and speed changes, as seconds (`12.5`) or `m:ss.s` (`1:02.5`). Each time
shows the frame on screen then; a time past the end shows the last frame, with a note on stderr.
Each still seeks straight to its frame, so one an hour into a recording takes as long as one at
the start, well under a second at 1080p.

- `--at SECONDS` may be repeated. With one time, OUTPUT is the image. With several, the stills
  are numbered from 1 before the extension (`out.png` gives `out-1.png`, `out-2.png`...), or
  replace a `%d` in OUTPUT (`frames/%d.jpg`).
- `--count N` renders N frames spread evenly over the video (the middle of N equal parts)
  instead of `--at`.
- `--sheet` puts the frames on one contact sheet, a near-square grid with each tile labelled
  with its time (`mm:ss.s`).
- `--width W` sets each still's width, or the whole sheet's (1920 by default; a sheet of many
  tiles can come out a few pixels wider, as tiles stay even and at least 16 px wide); the
  height follows the project's aspect. `--fps N` picks the frame grid as for `export`.
  Stills use the MP4 export's frame rate and size; for a GIF's frames, pass its `--fps` and
  `--width`.

OUTPUT ends in `.png`, `.jpg` or `.webp`. Each written path is printed on stdout; errors go to
stderr with a non-zero exit (2 for bad arguments, 1 when rendering or writing fails).

### `transcribe BUNDLE`

Writes `transcript.json` in the bundle with a speech-to-text engine on this computer; nothing is
uploaded by omareel. `--backend` picks the engine:

- `auto` (the default) follows Omarchy's dictation choice
  (`~/.config/omarchy/defaults/dictation`). When that is voxtype on a Whisper model and
  `whisper-cli` is installed, voxtype's model runs through `whisper-cli` first, for measured word
  times. Then the chosen backend, then `whisper-cli` with any model, then voxtype.
- `voxtype` runs `voxtype transcribe` with its configured engine and model. Text only.
- `hex` runs `hex transcribe`, which needs Hex running with Labs > Transcribe files on. Text only.
- `superwhisper` runs `superwhisper transcribe` through its service. Text only. (Superwhisper's
  own cloud mode, if selected there, is Superwhisper's choice, not omareel's.)
- `whisper-cpp` runs `whisper-cli` with voxtype's model, a model in
  `~/.local/share/whisper.cpp/models`, or `OMAREEL_WHISPER_MODEL`. Times every word.

The recording's audio is read with ffmpeg at 16 kHz mono, and a loudness-based speech finder
splits it into phrases. Text-only engines transcribe one phrase at a time, which times the
phrases; their words are spread over the phrase's speech and marked `estimated`. An existing
transcript is kept unless `--force` is given. Without any engine the command says how to set one
up in Omarchy and exits 1.

### `probe BUNDLE`

Prints JSON containing source duration, frame rate, dimensions, input-event counts, and the
auto-generated zoom summary.

### `describe BUNDLE`

Prints the edited timeline as JSON, for scripts and AI agents:

- `source` and `output`: the recording (size, frame rate, audio, camera) and the edited video
  (duration, aspect, export size).
- `clips`: each clip's `id`, `kind` (`recording` or `video`), `output` span, `source` span (or
  `media` and `media_range` for an inserted video), `speed`, and `layout` or `fit`.
- `zooms`: each zoom's `id`, `source` span, the `output` spans where it plays, `level` and
  `target` (`"auto"` or `{x, y}` from 0 to 1).
- `activity`: clicks (with normalized positions), keyboard shortcuts, and `idle` stretches of
  two seconds or more without any input, each with its source time and the output times where
  it plays (an empty list when it was cut).
- `settings`: the rest of `project.json` (background, frame, cursor, keystrokes, audio, camera,
  export settings), whose dotted paths `apply`'s `set` takes.
- `allowed`: the clip speeds, layouts and fits the editor offers.

"Source" seconds are times in `screen.mp4`; "output" seconds are times in the edited video.

### `skill [install]`

Prints the skill that teaches coding agents to edit and export recordings with `describe`,
`apply` and `export`. `skill install` writes it to `~/.agents/skills/omareel/SKILL.md` and links
it from each agent skills directory Omarchy sets up that exists (`~/.claude/skills`,
`~/.codex/skills`, `~/.pi/agent/skills`, `~/.gemini/config/skills`, `~/.hermes/skills`),
leaving alone anything else already named `omareel` there. Run it again after an upgrade to
refresh the skill.

### `apply BUNDLE OPS`

Runs edit operations from a JSON file (or `-` for standard input) through the editor's own
logic, so they are checked as the editor checks them, and saves `project.json`. It is all or
nothing: if any operation fails, nothing is saved and the error names it. `--dry-run` checks
without saving; `--describe` adds the resulting timeline to the report. The report is JSON on
standard output; the exit status is 1 on failure.

The operations are a JSON array (or `{"ops": [...]}`, or a single operation). They run in
order, and output times refer to the timeline as it is when each one runs, so a cut moves what
comes after it. Add `"timeline": "source"` to an operation to give recording times instead,
which no edit moves.

| Operation | Fields |
| --- | --- |
| `split` | `at` |
| `delete` | `from`, `to` |
| `trim` | `clip`, `in` and/or `out` (source seconds; an inserted video's own) |
| `remove-clip` | `clip` |
| `merge` | `clip`, `with`: `next` or `previous` |
| `move-clip` | `clip` (an inserted video), `direction`: `earlier` or `later` |
| `reset-trims` | `clip` |
| `speed` | `clip` or `at`, `speed` (one of `allowed.speeds`) |
| `layout` | `clip` or `at`, `layout`: `default`, `camera` or `screen` |
| `fit` | `clip` or `at`, `fit`: `fit`, `fill` or `framed` |
| `insert` | `file`, `at` (default: the end) |
| `zoom` | `from`, `to`, `level` (1–4, default 2), `target` (`"auto"` or `{"x": 0–1, "y": 0–1}`) |
| `edit-zoom` | `id`, any of `from`, `to`, `level`, `target` |
| `remove-zoom` | `id` |
| `clear-zooms` | |
| `regenerate-zooms` | |
| `music` | `file` (audio, or a video whose sound is used) and/or `volume` (0–1), `fadeIn`, `fadeOut`, `start` (seconds), `duck`, `loop` (true or false) |
| `remove-music` | |
| `set` | `path` (dotted, e.g. `background.blur`), `value` |

```sh
omareel apply Recording.omareel - <<'OPS'
[
  {"op": "delete", "from": 0, "to": 1.5},
  {"op": "speed", "at": 20, "speed": 2},
  {"op": "zoom", "from": 31, "to": 35, "level": 2.5, "target": {"x": 0.7, "y": 0.4}},
  {"op": "set", "path": "keystrokes.enabled", "value": true}
]
OPS
```

## Editor keys

| Key | Action |
| --- | --- |
| `Space` | Play/pause |
| `Left`, `Right` | Previous/next frame |
| `Shift+Left`, `Shift+Right` | Back/forward one second |
| `S` | Split at playhead |
| `Z` | Add a zoom of up to two seconds at the playhead |
| `I` | Insert a video at the playhead |
| `Delete` | Remove the selected time range, selected zooms, or selected clip |
| `Backspace` | Remove the selected time range |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo/redo |
| `Ctrl+S` | Save |
| `Ctrl+E` | Export |

The timeline fits the full recording at 1×. Scroll either wheel axis or touchpad axis to zoom
around the pointer, up to 16×; right-button drag pans the zoomed timeline. The bottom toolbar's
zoom controls keep the visible center in place. Click or drag
on the ruler or video row to seek; scrubbing pauses playback. The video row shows frame
thumbnails sampled from each clip's source interval. Clicking a clip also selects it, and
Ctrl+click selects it without seeking; Ctrl+click it again, click empty video-row space, or press
Escape to deselect it. Ctrl+drag moves its source window, and the selected clip's trim handles
can be dragged to trim. Double-click open space in the zoom row to add a zoom there: up to two
seconds, shortened or moved back to fit before the next zoom.
Shift+drag on the ruler or video row selects a time range. Drag either orange range handle to
adjust it and preview that endpoint. Seeking and Space playback stay inside the range; playback
stops at its end and restarts at its start. Press Delete or Backspace to cut the range and join
the remaining footage, or Escape to clear it.

## Clip layouts

Each clip has a layout, chosen in the Clip panel or from the menu on the clip's label:

- **Screen & camera** shows the screen with the camera bubble set up in the Camera panel.
- **Camera** shows the webcam full screen, for a talking-head intro, outro, or aside.
- **Screen** shows the screen without the bubble.

To open on a talking head, move the playhead to where the screen part starts, press `S`, select
the first clip, and choose Camera. Camera clips show webcam frames on the timeline. Moving between
layouts animates over half a second at the start of the next clip: the bubble grows into the full
frame, or shrinks back into its corner. The Camera panel's Full screen setting picks how camera
clips look: **Fill frame** covers the whole canvas with the uncropped webcam, and **Square + blur**
keeps a centred square over a blurred copy of the camera. Camera clips show the webcam even with
the bubble switched off.

## Inserted videos

Press `I`, click the film button beside the scissors, or use **Insert video at playhead** in the
Clip panel to add a video file (MP4, MOV, MKV, WebM) to the timeline: a demo, a retake, or a
clip exported from another Omareel recording. The video is copied into the bundle's `media/`
directory (reflinked where the filesystem allows, so it takes no extra space) and placed at the
playhead, splitting the clip there. Inserted clips play with their own sound, cut in and out
without a transition, and show no cursor, zooms, keystrokes, or camera.

Select an inserted clip to choose how it meets the frame: **Fit** shows all of it over a blurred
copy of itself, **Fill** covers the frame, and **Framed** places it in the frame over the
background, like the screen. Trim, split, speed, and delete work as for other clips; **Move
earlier** and **Move later** move it past its neighbour. Recording clips keep their order. If an
inserted video goes missing from `media/`, its clip is marked Missing and export stops until it
is restored or deleted.

## Background music

**Add music…** in the Audio panel sets an audio file (MP3, M4A, AAC, Ogg, Opus, FLAC, WAV), or a
video whose sound you want, as background music. Like an inserted video it is copied into the
bundle's `media/` directory; **Replace music…** swaps the file and keeps the settings, and the
trash button removes it (undo brings it back). The music plays under the whole edited video,
from its start to its end, whatever the clips: it starts at the beginning of the output, starts
over when it is shorter than the video (**Loop**; off, it plays once), and fades in over the
first seconds of the video and out over its last seconds (**Fade in**, **Fade out**, 0 to 10 s).
The fades follow the video, not the music: music that doesn't loop and ends before the video
does is cut off where it ends, and the fade-out falls on the silence after it. For a video
used as music, its sound's own length counts, which can be shorter than its picture's.
**Music volume** sets its level without changing the recording's. With **Lower under the recording** on, the music drops
while the recording's own sound (your voice, the desktop, or an inserted video's sound) plays,
and comes back in the pauses. It shows as a thin Music lane under the timeline's tracks, with
its waveform; click it to open the Audio panel. The preview plays it in step with the playhead;
its ducking there is an approximation of the export's, from the recording's measured loudness.
GIF exports have no sound. If the music file goes missing from `media/`, export stops until it is
restored or the music is removed.

## Project bundle

The bundle directory contains `screen.mp4`, its `screen.mp4.ts` first-frame timestamp,
`capture.json`, `input.jsonl`, `project.json`, `thumb.jpg`, and `media/` with any inserted
videos and background music. Video remains unchanged while
clip, style, cursor, zoom, and music edits are stored in `project.json`.

`project.json` holds a `music` object only while there is music (projects without it are
unchanged):

```json
"music": {"file": "media/song.mp3", "volume": 0.25, "fadeIn": 1.0, "fadeOut": 2.0,
          "duck": true, "loop": true, "start": 0.0}
```

`file` is bundle-relative and must stay inside `media/`; `volume` is 0 to 1; the fades are
seconds, 0 to 10; `start` is the offset in seconds into the file where the music begins (later
loops start the file from its beginning; with `loop`, a start past the end of the file wraps
around, so 6 s into a 4 s file begins at 2 s).

`transcript.json` (once transcribed) has every time in seconds of `screen.mp4`, so edits never
invalidate it:

```json
{
  "version": 1, "backend": "voxtype", "model": "base.en", "language": "en",
  "wordTiming": "estimated",
  "segments": [
    { "start": 0.42, "end": 3.1, "text": "Hello there.",
      "words": [ { "start": 0.42, "end": 0.8, "text": "Hello" }, ... ] }
  ]
}
```

`wordTiming` is `measured` when the engine timed each word (whisper-cli), `estimated` otherwise.
Times count from the start of `screen.mp4`, even though its sound track usually starts a little
after its video (the recorder places the sound by its first sample).

`project.json` gains a `captions` object only once captions are changed in the editor, so older
projects keep their bytes: `{ "enabled": false, "position": "bottom", "size": 1 }` (`position`
`bottom` or `top`, `size` 0.5 to 2).

## Speech in the editor

When a recording with sound opens, the editor measures where it speaks, checks the screen
during the silences and, with no transcript yet, transcribes it in the background. That never
blocks editing; while a recording or an export runs it stops what it is running and carries on
afterwards, and closing the editor stops it at once. The Speech section shows its progress, or
why there is no transcript, and offers:

- Show captions, with position and size: drawn in the preview and the export, at most two lines
  of about 42 characters, in the keystroke pills' style.
- Save .srt / .vtt beside the default export.
- Trim pauses: each silence over a second shrinks to 0.4 s, and the silence before the first
  and after the last words shrinks to 0.2 s. A silence is kept when anything changes on screen
  during it (typing, a video, scrolling output; a blinking text cursor does not count) or it
  has clicks, scrolling, pointer travel or shortcuts in it, since on a screen recording that is
  someone showing something. The screen is read from `screen.mp4`, five frames a second, only
  during the silences. Needs no engine. Desktop audio counts as sound, so music under the
  voice hides pauses.
- Remove filler words (um, uh, erm, hmm…), with the hesitation around them. Only with measured
  word times (whisper-cli, from the `whisper-cpp` package): estimated times can miss a short
  "um" and cut the words beside it.

Both trims are one undo step. Engines differ in whether they write fillers out.

## Environment

- `OMAREEL_DEBUG=1` appends `gpu-screen-recorder` stderr to `/tmp/omareel.log`.
- `OMAREEL_PREVIEW_STATS=1` plays an opened recording and logs preview frame rate, average
  frame handling cost, and drops once per second.
- `OMAREEL_MUSIC_TRACE=1` logs each background-music sync in the editor: output time, the
  music's target and reported positions in milliseconds, its gain, and whether it plays.
- `OMAREEL_CAPTURE=gsr` forces the fallback screen capture backend.
- `OMAREEL_NO_TRANSCRIBE=1` stops the editor transcribing recordings on its own.
- `OMAREEL_WHISPER_MODEL=PATH` is the ggml model `whisper-cli` uses.
- `OMAREEL_CAPTURE=native` selects the native screen capture backend when supported.
- `OMAREEL_PLUGIN_PATH` selects the capture-exclusion plugin file. An explicit missing path
  forces overlay fallback placement.
- `OMAREEL_CAPTURE_BUFFERS=shm` makes native capture use shared memory instead of dmabufs.
- `OMAREEL_CAPTURE_ENCODER=ffmpeg` makes native capture read dmabufs back for ffmpeg instead of
  encoding them with VA-API.
- `OMAREEL_CAPTURE_QP=N` sets the VA-API capture encoder's constant quantiser (0–51; 18 by
  default, 12 below 1.5 megapixels).
- `OMAREEL_NATIVE_CONVERSION=cpu` forces CPU colour conversion for native capture through ffmpeg. By default,
  the hardware conversion filter is exercised with a BGRA frame first and CPU conversion is used
  automatically when that exact conversion is unsupported.
- `OMAREEL_EXPORT_ENCODER=nvenc|vaapi|x264` picks the MP4 export encoder (still falling back
  to libx264 when it does not work). By default export uses NVENC, then VA-API on an Intel or AMD
  render node (never NVIDIA's VA-API driver), then libx264, each GPU encoder probed first with
  one frame at the export size. `OMAREEL_DISABLE_NVENC=1` skips NVENC.
- `OMAREEL_EXPORT_DECODER=cuda|vaapi|software` picks how export decodes recordings. By default
  it decodes with CUDA when NVIDIA's driver is loaded and in software otherwise; VA-API decoding
  is opt-in because reading its frames back costs more than it saves. `--timing` reports the
  decoder and encoder used.
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
  first, to exercise playing-state seeks after end-of-media.
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

`OMAREEL_SCREENSHOT_SELFVIEW=/tmp/selfview.png` and `OMAREEL_SCREENSHOT_BAR=/tmp/bar.png`
grab the corresponding overlay window directly, preserving transparency. Use
`OMAREEL_SCREENSHOT_DELAY_MS` to override the default 4000 ms overlay delay.
`OMAREEL_SCREENSHOT_KEEP_OPEN=1` keeps a launcher or editor open after its debug grab.

## Overlays and capture

Omareel automatically loads its matching Hyprland capture-exclusion plugin when
the launcher opens or a recording starts. The self-view can stay where you put
it, and the recording bar stays on the recorded monitor, while capture sees the
live content underneath. No pixel patching or hidden-frame handshake is needed.

The plugin must be rebuilt for the installed Hyprland commit. Omareel checks its
adjacent `.so.hash` file before loading it. `OMAREEL_PLUGIN_PATH` selects an
explicit plugin file; otherwise Omareel checks the build-tree `plugin/` directory
and `/usr/lib/omareel/`. With Hyprland's `ecosystem:enforce_permissions` enabled,
the first load shows a one-time permission dialog.

If the plugin is missing, mismatched, or refused, overlays move to another
monitor during recording. With one monitor, the bar stays hidden and self-view
unmaps; stop with `omareel record --stop`, the REC indicator, or your keybind.
The standby self-view returns to its previous monitor and position after stop.
The launcher displays a short fallback note. The gsr backend also uses this
fallback because it does not use Hyprland's capture path.

`omareel probe <bundle>` reports `overlay_exclusion` as `plugin` or `fallback`;
the same value is saved in recording metadata. The countdown uses the recording
bar's namespace. Omareel disables layer animations before mapping its overlays
so fading snapshots cannot appear in capture.

If a self-view bubble is ever left behind, run `omareel __selfview-ipc quit`.
The host exits once any active recording finishes. Reopen the launcher to bring
the self-view back.
