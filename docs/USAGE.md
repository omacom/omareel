# omareel(1)

## Name

`omareel` — record and edit polished Omarchy screen videos.

## Synopsis

```text
omareel
omareel record [--region|--fullscreen|--window] [options]
omareel record --marker [LABEL] | --retake [SECONDS] | --pause
omareel edit BUNDLE.omareel
omareel export BUNDLE.omareel -o OUTPUT.mp4|OUTPUT.gif|OUTPUT.srt|OUTPUT.vtt [options]
omareel export BUNDLE.omareel --preset youtube|shorts|x|github-gif|slack [-o OUTPUT]
omareel transcribe BUNDLE.omareel [--backend NAME] [--force]
omareel probe BUNDLE.omareel
omareel describe BUNDLE.omareel
omareel apply BUNDLE.omareel OPS.json|-|'[...]' [--dry-run] [--describe]
omareel agent BUNDLE.omareel polish|chapters|ask REQUEST...
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
- `--marker [LABEL]`, `--retake [SECONDS]` and `--pause` mark the running recording (see
  [Markers, retakes and pauses](#markers-retakes-and-pauses)); each returns status 1 when no
  recording is active.

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

## Markers, retakes and pauses

While a recording runs, these commands (also the flag, undo and pause buttons on the recording
bar) leave marks the editor uses when it first opens the recording:

- `omareel record --marker [LABEL]` drops a marker at that moment. Markers show as tags on the
  editor's ruler; `M` and `Shift+M` jump to the next and previous one, and double-clicking (or
  right-clicking) a tag renames or deletes it.
- `omareel record --retake [SECONDS]` says "that didn't go well". The take since the last
  marker, retake or resume is cut, or the last 5 seconds when there is none; with `SECONDS`,
  exactly that much. Seconds count recorded time only: a retake of 5 seconds made 2 seconds
  after resuming also cuts the 3 seconds before the pause (the command prints how many recorded
  seconds it cuts). The new project starts with that span cut out of its clips and a retake
  marker where the cut ends; nothing is deleted, so dragging the clips' trim handles brings it
  back.
- `omareel record --pause` pauses the recording, and run again resumes it. The bar shows
  **Paused** and its timer stops. With in-process capture no frames are taken while paused: the
  video holds the frame from before the pause, so nothing on screen meanwhile is recorded. With
  the gpu-screen-recorder fallback the paused time is still captured. Either way the new project
  starts with the paused time cut, and input while paused (clicks, keys, scrolls, pointer moves)
  is left out of `input.jsonl`, so it makes no clicks, keystrokes or zooms. A marker or retake
  made while paused belongs to the moment the pause began.

  **Paused time is cut, not erased.** The microphone and desktop audio and `camera.mp4` keep
  recording while paused (they stay in sync with the video that way), and so does the screen
  with the gpu-screen-recorder fallback. The new project never plays a paused span, but the
  bundle still holds it: someone with the bundle can hear what was said, and see the camera (and
  with the fallback the screen), during a pause. Pause to leave something out of the edit, not to
  keep it private; stop the recording for that. Exports only contain what the project plays.
  When a recording was paused from its first frame to the stop, the new project keeps just its
  first tenth of a second, with the audio and camera turned off.

Bind them like the toggle, for example:

```lua
o.bind("SUPER + ALT + M", "Omareel marker", "omareel record --marker")
o.bind("SUPER + ALT + BACKSPACE", "Omareel retake", "omareel record --retake")
o.bind("SUPER + ALT + P", "Omareel pause", "omareel record --pause")
```

The shortcut that ran a command, or the click on a bar button, is left out of `input.jsonl` too,
so it never shows in the keystroke overlay or makes a zoom. To leave real input alone, only
these count:

- for a command: the last key pressed with **Super** held in the second before the command ran,
  with the modifiers held for it and their releases (Hyprland's global binds use Super, apps'
  shortcuts almost never do). A command run any other way, such as from a bind without Super, a
  macro key, or typed in a terminal, leaves every key in `input.jsonl`;
- for a bar button: the click on it, the last press on the bar's box (from the pointer's
  position) released at most three seconds before the mark, however long it was held. When the
  bar's monitor is unknown, no click is left out.

Input while paused is left out either way. A command run after the recording began stopping is
refused (status 2, "The recording is stopping"). Generated zooms only come from clicks that a
clip still shows, and **Regenerate zooms** follows the current clips the same way.

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

**Editing with AI.** The top bar's Polish button (`Ctrl+Shift+P`) and the speech-bubble button
beside it open a drawer that runs the coding agent chosen in Omarchy (Setup › Defaults › Agent;
Claude Code, and Codex as experimental) on the recording, the way `omareel agent` does. Nothing
runs until you start it. The first time, Polish only opens the drawer: it says which agent runs,
that it reads the timeline, the transcript and frames of the video and sends them to its
provider, and has a Start button. Once started, an agent is remembered
(`~/.config/omareel/agent.json`), and Polish and `Ctrl+Shift+P` run it at once; the drawer's
"Ask first" takes that back. Its actions: Polish (trim the ends, cut idle stretches and pauses,
fix zooms, show keystrokes when shortcuts matter, then check a contact sheet), "Ask the editor…"
(any request, such as "make it under 60 seconds"), or Write description (chapters, a title and
a description). The project is saved before the agent starts. It edits through `omareel
apply`, so each change appears in the editor as it lands and is one `Ctrl+Z` away; chapters
and the description are saved on their own (`chapters.json`) and can't be undone. While it
runs, the editor's own editing is off ("The agent is editing… Stop to edit" over the timeline),
so nothing you change is lost to its next change; it comes back when the agent finishes or
stops. The drawer lists its messages, the commands it runs, each change, and the frames it
looked at; Stop ends it at once. Closing the editor stops it too. Chapters show as flags on
the timeline's ruler (hover for the title, click to seek), and the drawer lists them with
"Copy YouTube description".

Legacy `.omarecord` bundles remain supported, including bundles under
`~/Videos/omarecord`. The legacy command name is installed as a compatibility alias. If only
legacy settings exist, they are copied to `~/.config/omareel/settings.json` on first run and the
original file is retained.

### `export BUNDLE -o OUTPUT`

Exports `.mp4` or `.gif`. Common options are `--fps N`, `--width W`, and `--quality LEVEL`.
Quality may be `low`, `medium`, `high`, `best`, `web-low`, `web-high`, `social`, or `studio`.
GIF-specific overrides are `--gif-fps N` and `--gif-width W`.
`--aspect A` (`auto`, `16:9`, `9:16`, `1:1`, `4:5`, `4:3`, `3:4`) and `--reframe follow|off`
change the canvas and [auto-reframe](#auto-reframe) for this export only; the project keeps its own.

`--preset ID` exports for one platform. It sets the format, size, frame rate, quality, aspect and
reframing, and without `-o` names the file `<project name>-<suffix>` in the recordings directory
(created if needed). The options above still override it, and the preset never changes
`project.json`.

| Preset | File | Size | Rate | Quality | Canvas |
| --- | --- | --- | --- | --- | --- |
| `youtube` | `-youtube.mp4` | 1920×1080 | 60 | social | 16:9 |
| `shorts` | `-shorts.mp4` | 1080×1920 | 60 | social | 9:16, reframed |
| `x` | `-x.mp4` | 1080 high | 30 | social | the recording's |
| `github-gif` | `-github.gif` | 480 high | 15 | studio | the recording's |
| `slack` | `-slack.mp4` | 720 high | 30 | web-high | the recording's |

A preset's format must match `-o`: `github-gif` writes `.gif`, the others `.mp4`.

Add `--timing` to print average decode wait, upload, render, readback, encoder-write, and
end-to-end throughput measurements. A project with loudness normalisation (see
[Audio cleanup](#audio-cleanup)) prints `Measuring loudness N%` before the frames.

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
- `--preset ID`, `--aspect A` and `--reframe follow|off` shape the stills as they shape an
  export, for these stills only: `--preset shorts` shows the vertical, reframed video.

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
- `allowed`: the clip speeds, layouts and fits the editor offers, and the transition styles,
  overlay kinds, blur styles, loudness targets, aspects, and export presets (`id`,
  `description`, `format`, `aspect`, `reframe`).
- `overlays` (when there are any): each annotation's `id`, `kind`, `anchor`, its `source` span
  (screen items) and the `output` spans where it shows, and its fields as `project.json` has
  them, an arrow's ends named `tail` and `head`.
- `markers` (when there are any): `id`, `label`, `kind` (`marker` or `retake`), the `source`
  time and the `output` times where it plays.
- `recording_cuts` (when the bundle has `markers.jsonl` with retakes or pauses): what the
  recorder cut out of the first project, each with its `kind`, `source` span, and the `output`
  spans where it plays again (normally none).
- `transitions`: the project's `style` and every cut (`clip` after it, `output` time, `style`).
- `reframe`: `mode` (`follow` or `off`) and whether it is `active` (the canvas has another shape
  than the recording).
- `audio_cleanup`: `denoise`, `strength`, `loudness`, and `denoise_available` (the recording has
  microphone sound).
- `transcript` (once the recording is transcribed): the engine, whether word times are
  `measured` or `estimated`, and each phrase's `text` with its `source` span and the `output`
  spans where it plays.
- `music` (when set): its file and length.
- `chapters` (when the bundle has `chapters.json`): `title`, `description`, and each chapter's
  `source` time, `title`, and `output` time on the edited timeline (`null` once its stretch was
  cut).

"Source" seconds are times in `screen.mp4`; "output" seconds are times in the edited video.

### `skill [install]`

Prints the skill that teaches coding agents to edit and export recordings with `describe`,
`apply` and `export`. `skill install` writes it to `~/.agents/skills/omareel/SKILL.md` and links
it from each agent skills directory Omarchy sets up that exists (`~/.claude/skills`,
`~/.codex/skills`, `~/.pi/agent/skills`, `~/.gemini/config/skills`, `~/.hermes/skills`),
leaving alone anything else already named `omareel` there. Run it again after an upgrade to
refresh the skill.

### `apply BUNDLE OPS`

Runs edit operations from a JSON file, standard input (`-`), or the argument itself (anything
starting with `[` or `{`, for agents allowed to run commands but not to write files), through the editor's own
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
| `delete` | `from`, `to`; or `ranges`: `[[from, to], ...]` (one undo step with `"timeline": "source"`) |
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
| `trim-pauses` | optional `min` (seconds of silence, default 1), `keep` (seconds left, default 0.4) |
| `remove-fillers` | (needs a transcript with measured word times) |
| `set` | `path` (dotted, e.g. `background.blur`), `value` |
| `set-chapters` | any of `chapters` (`[{"at", "title"}, ...]`, replacing the list), `title`, `description` |
| `add-overlay` | `kind` (`text`, `box`, `arrow`, `badge`, `blur`, `card`), `from`, `to`, optional `anchor` (`screen` or `frame`), `rect` (`{"x", "y", "w", "h"}`), and the kind's fields (below); returns the overlay with its `id` |
| `edit-overlay` | `id`, any of `from`, `to`, `anchor`, and the kind's fields (`rect` may give only some of x, y, w, h) |
| `remove-overlay` | `id` |
| `blur` | `from`, `to`, `rect`, optional `style` (`pixelate` or `blur`), `amount` (0–1): a blur overlay |
| `add-marker` | `at`, optional `label`; returns its `id` |
| `rename-marker` | `id`, `label` |
| `remove-marker` | `id` |
| `transition` | `style`: `none` or `zoom-punch`, at every cut |
| `cut-transition` | `clip` or `at` (the clip after the cut), `style`: `none`, `zoom-punch`, or `null` to follow the project |
| `speed-up-quiet` | optional `target` (seconds each quiet part plays in, default 2), `min` (the shortest part, default 3 s) |
| `reframe` | `mode`: `follow` or `off` |
| `audio-cleanup` | any of `denoise` (true or false), `strength` (0.1–1), `loudness` (`"off"`, `-16`, `-14`) |

Overlay fields by kind: `text`: `text`, `size` (8–400 px at 1080p), `color` (`#rrggbb`, or `""`
for the theme's), `background` (`pill` or `none`); `box`: `color`, `width`, `radius`,
`spotlight` (0–1); `arrow`: `tail` and `head` (`{"x", "y"}`), `color`, `width`; `badge`:
`number`, `color`; `blur`: `style`, `amount`; `card`: `title`, `subtitle`. All but `blur` take
`fadeIn` and `fadeOut` (0–5 s). A field the kind doesn't have is an error. `screen` items
(the default for box, arrow, badge and blur) belong to the recording: their `rect` and arrow
ends are 0–1 of the recording's full frame, before the crop, so they follow the crop, zooms and
reframing, and they are stored at source times; given output times, they get the recording span
those times show. `frame` items (the default for text and card) are 0–1 of the canvas and stay
at output times. A blur is always `screen` and a card always `frame`. The export hides a blur's
region in the decoded recording before anything is drawn, from a frame before its span to a
frame after it.

`trim-pauses` and `speed-up-quiet` measure the speech and the screen as the editor does, and
share the editor's measurement cache. A run that makes footage cut while recording (a retake or
a pause in `markers.jsonl`) play again says so in `warnings`.

`set-chapters` writes `chapters.json`, storing each `at` as a source time whichever timeline
it is given in, and only once the whole list has applied; with everything empty it removes the
file. The file is written aside first, then `project.json` is saved, then the chapters are
moved into place, so a failure to write them saves nothing. Chapters aren't part of the
editor's undo.

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

### `agent BUNDLE polish|chapters|ask REQUEST...`

Runs the coding agent chosen in Omarchy (`~/.config/omarchy/defaults/agent`, what
`omarchy-default-agent` prints) once, non-interactively, on the recording, as the editor's AI
drawer does, and prints what it does as JSON lines (`started`, `message`, `command`, `apply`,
`image`, `problem`, `finished` with its cost and duration when the agent reports them). The exit
status is 0 when the agent finished. `polish` tightens the recording, `chapters` writes
`chapters.json`, and `ask` does what the rest of the line asks. Ctrl+C stops the agent.

The agent runs in the bundle's directory with the omareel skill as instructions. What it reads
(speech, text on screen) can carry instructions of its own, so it is treated as untrusted:

- Every `omareel` it runs is held to this recording: the run sets
  `OMAREEL_AGENT_SCOPE=BUNDLE<newline>SCRATCH`, and omareel then allows only `describe BUNDLE`,
  `apply BUNDLE …` (operations inline, on stdin, or in a file in the bundle or scratch
  directory; files the operations name must be there too), `frame BUNDLE … -o FILE` with the
  file in the scratch directory, `transcribe BUNDLE`, and `help`. Anything else (`record`,
  `export`, `edit`, `skill`, `agent`, the launcher, another bundle, an image elsewhere) is
  refused with exit status 126. Paths are compared with symlinks and `..` resolved.
- Claude Code: `claude -p --output-format stream-json --verbose --restricted --tools Bash,Read
  --allowedTools "Bash(omareel describe:*)" "Bash(omareel apply:*)" "Bash(omareel frame:*)"
  "Bash(omareel transcribe:*)" "Bash(omareel help)" Read --permission-mode dontAsk
  --strict-mcp-config --no-session-persistence`. Its settings files (and any broader allow
  rules in them) are ignored, any other command is refused, it has no MCP servers, and its
  file tool reads only the bundle and the scratch directory.
- Codex (experimental): `codex exec --json --sandbox workspace-write --ignore-user-config
  --ignore-rules --ephemeral --skip-git-repo-check -c sandbox_workspace_write.network_access=false
  -c sandbox_workspace_write.writable_roots=[] -c web_search="disabled"`. Your `config.toml`
  isn't loaded (no MCP servers, hooks, features or extra writable roots from it; your model
  choice there doesn't apply either, use `OMAREEL_AGENT_MODEL`). Its commands may write only to
  the bundle and the scratch directory, without network, but **they can read any file you
  can**, and what they read goes to OpenAI; its shell isn't limited to `omareel` either.

Each run gets a scratch directory of its own, `~/.cache/omareel/agent/NAME-XXXXXX/`, for the
frames the agent renders and the run's raw output (`stream.jsonl`, `stderr.txt`); it is removed
when the next run starts in that editor, when the editor closes, or when `omareel agent` exits.
Its `bin/omareel` points at the running omareel, first on the agent's `PATH`. The agent dies
with the editor or `omareel agent`, even when either is killed, and at the end of every run
(finished, failed, stopped, or after 30 minutes) everything it started is killed: its process
group, and any process still carrying the run's `OMAREEL_AGENT_SCOPE`, which also finds what
left the group. Omarchy's account selection is kept: `CLAUDE_CONFIG_DIR` or `CODEX_HOME` comes
from `omarchy-agent-account-home` when unset. Other agents Omarchy offers aren't supported yet;
`agent` says so and runs nothing.

## Editor keys

| Key | Action |
| --- | --- |
| `Space` | Play/pause |
| `Left`, `Right` | Previous/next frame |
| `Shift+Left`, `Shift+Right` | Back/forward one second |
| `S` | Split at playhead |
| `Z` | Add a zoom of up to two seconds at the playhead |
| `I` | Insert a video at the playhead |
| `T`, `R`, `A`, `N`, `B` | Add a text label, box, arrow, numbered step, or blur region at the playhead |
| `Shift+T` | Add a title card at the playhead |
| `M`, `Shift+M` | Next/previous marker |
| `Ctrl+M` | Add a marker at the playhead |
| `Delete` | Remove the selected time range, annotation, zooms, or clip |
| `Backspace` | Remove the selected time range |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo/redo |
| `Ctrl+S` | Save |
| `Ctrl+E` | Export |
| `Ctrl+Shift+P` | Polish with the coding agent |
| `?` | Show the shortcuts |

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

## Annotations

The **Annotate** button in the bottom toolbar, the Annotate panel, and the keys above add an
annotation at the playhead, three seconds long:

- **Text** (`T`): a label with a font size, a colour, and a theme-coloured pill behind it or none.
- **Box** (`R`): a rounded outline around a region; **Spotlight** dims everything outside it.
- **Arrow** (`A`): from one point to another.
- **Step** (`N`): a numbered circle; each new one takes the next number.
- **Blur** (`B`): pixelates or blurs the recording under a rectangle, to hide a password, key,
  or message. The export bakes it into the recording's own pixels before anything is drawn, so
  it covers the place the preview shows on the GPU and the software path alike, and it never
  fades. It errs on the safe side in time: the export blurs from a frame of the recording plus
  two of the export (times the clip's speed) before its span to as long after, and the preview
  shows it 0.05 s early and late, so no frame of what it hides slips through at either end. Even
  the lowest strength makes text unreadable: cells (or the blur's radius) are an eighth of the
  region's height, and 4 % of the recording's height, at the least.
- **Card** (`Shift+T`): a full-frame title card with a title and subtitle over the project's
  background, styled from the Omarchy theme.

Click an annotation on the preview to select it; drag it to move it, and drag its corner handles
(an arrow's two ends) to resize it. Annotations get their own **Overlay** row on the timeline once
there is one: drag a block to move it in time and its edges to trim it. The Annotate panel edits
the selected one's text, size, colour, line, spotlight, number, blur style and strength, and fades.
`Delete` removes it, and each drag or slider gesture is one undo step.

Annotations either follow the recording or sit on the frame (the panel's **Follow the
recording** switch). Boxes, arrows, steps and blur follow the recording by default: they move
with zooms and the crop, keep to the moment they mark through cuts, and hide while an inserted
video plays. Text and cards sit on the frame by default: they stay put on the canvas and keep
their place on the edited timeline. A card is an overlay rather than a clip of its own, so the
recording keeps playing (and its sound with it) underneath; put it over a still moment, or at the
start or end.

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

## Transitions

Where the recording jumps from one moment to another (two recording clips whose source times do
not meet: a deleted range, a trimmed pause), the Clip panel's **Cuts** setting picks what plays:

- **None** (the default): a plain jump cut.
- **Zoom punch**: the screen and its cursor grow 6% over the 0.08 s before the cut and settle
  back over the 0.32 s after it, so the eye follows the movement instead of the jump. The sound
  fades out and in over 0.02 s on each side of the cut, so the join does not click.

Selecting the clip after a cut shows **Cut before**, which overrides the setting for that cut
alone. A split (clips that meet) and an inserted video are not cuts. The preview and the export
draw the same transition, in GPU and software rendering. The preview's sound is not faded.

A crossfade is not offered yet: it needs both clips' frames at once, while the preview plays the
recording with one player and the export decodes one clip at a time. It would take a second
screen layer in the composition, fed by a second preview player seeking ahead to the next clip
and, in the export, the next clip's decoder (already started ahead) handing over its first
frames during the overlap, plus an `acrossfade` of the two segments in the audio mix.

## Auto-reframe

A 16:9 screen squeezed into a 9:16 or 1:1 canvas is a thin, unreadable strip. **Reframe**
(beside Crop in the bottom bar, available once the canvas has another shape than the recording)
fills the canvas with a window of the screen that follows the action instead:

- while a zoom is active, the window tightens with the zoom and centres on what it is about (a
  fixed target, or the click), even near a screen edge; an automatic zoom also keeps the cursor
  in the window as it moves;
- otherwise it follows the smoothed cursor, once the cursor leaves the middle half of the window
  (a dead zone, so small movements don't pan);
- it moves on the zoom spring (Zoom panel), so pans feel like the zooms;
- near a screen edge it rests flush against the edge, and it never leaves the recording.

The window is worked out per moment from the recording and project alone, so the preview, the
export, and the software renderer show the same frame. In a reframed vertical canvas the camera
bubble moves to the middle of its edge, and keystroke pills at the bottom sit above it. Padding,
corners, and the background still apply (the frame keeps the canvas's shape inside the padding);
choose **Remove background** for edge-to-edge video. A canvas of the recording's own shape (within
2%) has nothing to reframe: the toggle is unavailable, and a project set to reframe one looks
exactly as if it weren't.

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

## Audio cleanup

The Audio panel's **Cleanup** section makes a laptop-microphone recording sound better:

- **Reduce noise** suppresses fan noise, hiss, keyboard clatter, and room tone in your voice with
  RNNoise, a speech denoiser. **Strength** (10–100%, default 85%) sets how much of the
  denoised sound replaces the original. It is offered when the recording has microphone sound.
  The recording keeps the microphone and the system audio in one track, so system audio is
  filtered too, which can dull music or effects; inserted videos are never filtered. The
  recording is denoised as one continuous stream before it is cut into clips, so cuts lose
  nothing.
- **Loudness** evens the export out to a standard level: **−16 LUFS** (podcasts, YouTube) or
  **−14 LUFS** (social video). It normalises the *program*: the clips' sound, the recording's
  and inserted videos' alike, as edited. Export measures the program first (the export shows
  "Measuring loudness"), then applies one gain to all of it, keeping the true peak under
  −1.5 dBTP after encoding (loudnorm limits at −2 dBTP, and only when the gain alone would pass
  it). The click sound is not part of the program: it is mixed in afterwards at its usual level
  (peaks of −22 dBFS), so normalising never makes the clicks louder; where a click lands on a
  peak the two can reach about −1.2 dBTP. Background music is not part of it either: it is mixed
  in after normalisation at its own volume (ducked under the normalised program when **Lower under
  the recording** is on), so it never changes the gain the program gets.
- **Volume** (the Audio panel's slider) applies after normalisation, as a trim on the normalised
  level: at 100% the export sits at the target, at 50% 6 dB under it, and above 100% it stays at
  the target (its peaks are already at the limit). With loudness off it sets the clips' level as
  before. It never changes the click sound.

The preview plays the cleaned sound once it has been made in the background (a few seconds for
a few minutes of recording); until then it plays the recording as it was. It is made the way
export makes it: the recording denoised as a whole, and, with loudness on, the program measured
as export measures it and that gain applied to the recording's sound and to each inserted
video's, with Volume on top. After an edit that changes the program's sound the preview keeps
playing what it has while it measures again, and keeps those files when the new measurement is
within 0.5 LU of theirs. **Hear original** switches the preview back to the recorded sound for
comparing, without changing the project.

Cleaned sound is cached in `~/.cache/omareel/omareel/audio-cleanup/`, beside the filmstrips:
the last three versions per file and their loudness measurements. Anything unused for 30 days
goes, a deleted recording's included, and the oldest entries go first once the cache passes
2 GB. The RNNoise model is written to `$XDG_RUNTIME_DIR`, or to `~/.cache/omareel/` (made
private to you) without one, and is checked byte for byte before each use.

`project.json` stores the choice as `audio.cleanup`, written only once something is switched on:

```json
"audio": {"desktop": true, "mic": true, "volume": 1,
          "cleanup": {"denoise": true, "strength": 0.85, "loudness": "-16"}}
```

`denoise` is a boolean, `strength` a number from 0.1 to 1 (clamped), and `loudness` one of
`"off"`, `"-16"`, `"-14"` (a number such as `-16` is read too; anything else is `"off"`).
Without `cleanup`, or with everything off, export is exactly as before.

Measured on 20 s of espeak-ng speech with synthetic noise. "Floor" is how far the level fell
where nobody speaks; SI-SDR (scale-invariant signal-to-distortion ratio against the clean speech)
goes from the noisy input to the output; "Speech" is the level change while speaking:

| Noise | Filter | Floor | SI-SDR | Speech |
| --- | --- | --- | --- | --- |
| fan + hiss, 10 dB SNR | RNNoise 85% (default) | −9.6 dB | 8.4 → 12.8 dB | −0.6 dB |
| | RNNoise 100% | −11.1 dB | 8.4 → 12.5 dB | −0.7 dB |
| | afftdn, eight settings | 0 to −2.5 dB | 8.4 → 7.5–8.7 dB | 0 to −1.0 dB |
| fan + hiss, 3 dB SNR | RNNoise 85% | −9.6 dB | 1.4 → 7.8 dB | −2.3 dB |
| | RNNoise 100% | −10.8 dB | 1.4 → 8.0 dB | −2.5 dB |
| | afftdn, eight settings | 0 to −1.2 dB | 1.4 → 0.9–1.6 dB | 0 to −0.8 dB |
| hiss 15 dB + keyboard | RNNoise 85% | −13.6 dB | 8.7 → 14.9 dB | −0.6 dB |
| | RNNoise 100% | −19.6 dB | 8.7 → 14.7 dB | −0.6 dB |
| | afftdn `nf=-20` | −7.3 dB | 8.7 → 11.6 dB | −1.0 dB |

On clean speech RNNoise keeps an SI-SDR of 20.4 dB at 85% (19.0 dB at 100%); afftdn with
`nf=-20` drops it to 14.1 dB. `anlmdn` crashes ffmpeg 9.0.2 with heap corruption.

## Project bundle

The bundle directory contains `screen.mp4`, its `screen.mp4.ts` first-frame timestamp,
`capture.json`, `input.jsonl`, `project.json`, `thumb.jpg`, `markers.jsonl` when the recording
was marked, and `media/` with any inserted videos and background music. Video remains unchanged
while clip, style, cursor, zoom, marker, and music edits are stored in `project.json`.

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

`chapters.json` (once written by `set-chapters`) keeps chapters at source times, so later edits
move them with the video; a chapter whose start was cut begins where the video picks up again,
and one whose whole stretch was cut is left out:

```json
{
    "chapters": [{"source": 0, "title": "Opening the terminal"}, {"source": 14.2, "title": "Installing the theme"}],
    "description": "How to install and switch themes in Omarchy.",
    "title": "Omarchy themes in two minutes",
    "version": 1
}
```

The YouTube description the editor copies is the title, the description, and a `00:00 Title`
line per chapter (`H:MM:SS` past an hour), with the first chapter at 00:00 and chapters under
10 seconds left out, as YouTube requires.

`project.json` gains a `captions` object only once captions are changed in the editor, so older
projects keep their bytes: `{ "enabled": false, "position": "bottom", "size": 1 }` (`position`
`bottom` or `top`, `size` 0.5 to 2).

Other fields written only once set, so older projects keep their bytes:

- `"transition": "zoom-punch"` at the top level: the transition at every cut (`none` or
  `zoom-punch`; `none` is left out).
- `"transition": "none"` on a clip: the transition at the cut before that clip, overriding the
  project's (`none` or `zoom-punch`; absent follows the project).
- `"hideWhileTyping": true` in `cursor`: the cursor fades out after a key press (Enter, Backspace,
  arrows, shortcuts: the keys `input.jsonl` has, since plain typing is never recorded) once the
  pointer has been still for half a second, and is back just before it moves or clicks again.

Speeding up quiet parts writes nothing new: it splits clips and sets their `speed`.

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

- Speed up quiet parts: the silences pause trimming keeps because the screen changes during
  them (an installer running, a build, scrolling, typing) play faster instead. Stretches of at
  least 3 s, less 0.3 s next to speech, are split into clips of their own at 2×, 3×, 4×, 8×,
  16× or 24×, whichever plays each closest to 2 s. Clips already at another speed are left
  alone. The button shows how many and the seconds saved.

Each is one undo step. Engines differ in whether they write fillers out.

The speech measurement and the screen check read the whole recording, minutes for a long 4K
one, so their results are cached in `${XDG_CACHE_HOME}/omareel/omareel/speech-v1/` (never in the
bundle), one `<key>.speech.json` and one `<key>.screen.json` per recording. The key is a hash of
`screen.mp4`'s size, modification time and first MiB: any change to the file is a miss, and the
next measurement replaces the stale entry. Deleting the directory is always safe.

Annotations are `project.json`'s `overlays` list, written only when there is one (older projects
stay byte for byte as they were). Each item:

| Field | Meaning |
| --- | --- |
| `id`, `kind` | `kind` is `text`, `box`, `arrow`, `badge`, `blur` or `card` |
| `anchor` | `screen`: follows the recording; `frame`: fixed on the canvas. `blur` is always `screen`, `card` always `frame` |
| `start`, `end` | Seconds: of the recording (source) for `screen` items, of the edited output for `frame` items |
| `rect` | `{x, y, w, h}`, 0–1 of the recording's full frame (before the crop) for `screen` items, of the canvas for `frame` items; not for `arrow` or `card` |
| `fadeIn`, `fadeOut` | Seconds of fading (not for `blur`) |
| `text`, `size`, `background` | `text`: the label, its pixel size at 1080 p, and `pill` or `none` |
| `color` | `text`, `box`, `arrow`, `badge`: `#rrggbb`; absent means the Omarchy theme's colour |
| `width`, `radius`, `spotlight` | `box`: line width and corner radius in pixels at 1080 p, and dimming outside it (0–1); `arrow`: `width` |
| `from`, `to` | `arrow`: tail and head, `{x, y}` like `rect` |
| `number` | `badge`: the number shown |
| `style`, `amount` | `blur`: `pixelate` or `blur`, and strength 0–1: cells (or blur radius) of an eighth (0) to a quarter (1) of the region's height, and at least 4 % (0) to 8 % (1) of the recording's height |
| `title`, `subtitle` | `card` |

Deleting a range of the timeline shrinks or removes annotations: `screen` items keep what still
shows of their source span, as zooms do; `frame` items after the cut move back with it, and those
inside it go. Blur regions are the exception: a cut never shrinks or removes one, so footage that
comes back later (resetting a clip's trims, a trim or a merge) comes back hidden; one whose
footage is all cut stays in the list, unseen. Any other edit that changes the video's length
(trims, speed, inserted videos) leaves `frame` items at their times, except that one that ended
at the end of the video stays at the end (an end card keeps its length and moves with it), and
one that would run past the end moves back to end there.

`markers.jsonl` is appended while recording, one compact JSON object per line. `t` is the
moment in monotonic microseconds, the clock of `input.jsonl` and of `capture.json`'s
`first_frame_us` (so `(t - first_frame_us) / 1e6` is the time in `screen.mp4`), and `k` its kind:

```text
{"k":"marker","label":"good take","t":81234567}
{"from":84000000,"k":"retake","t":86500000}
{"k":"pause","t":90000000,"via":"bar"}
{"k":"resume","t":95000000}
```

A retake cuts `from`..`t`; a pause runs to the next resume, or to the end without one. `label`
is left out when empty, and `"via":"bar"` marks one made with a bar button. `at` is when the
mark was made, when that differs from `t`: a marker or retake made while paused has the pause's
start as `t`, and a pause or resume has the moment it was asked for as `at`. Marks made after
`capture.json`'s `stopped_us` are ignored. The editor reads the file only when it makes the
project (as does `omareel export` of a bundle without `project.json`); bundles without it open
as they always did.

`project.json` keeps markers in `markers`, left out while there are none:
`[{"id": "m1", "time": 3.5, "label": "good take"}, {"id": "m2", "time": 9.0, "label": "",
"kind": "retake"}]`. `time` is in recording (source) seconds, like zoom times, so trimming or
cutting never moves a marker; a marker whose moment no clip shows stays in the project but is
hidden from the timeline. `kind` is left out for plain markers.

`project.json` keys that are left out when unset, so older projects stay byte for byte as they
were: `"reframe": "follow"` turns on [auto-reframe](#auto-reframe) (any other value, or none,
fits the whole recording into the canvas). The canvas shape is `"aspect"` (`"9:16"` and so on;
none for the recording's own).

A zoom's fixed `"target"` (`{"x": …, "y": …}`, or `"auto"`) is normalised to the whole recording,
0..1 across `screen.mp4` like the positions in `input.jsonl`, whatever the crop or reframing. Picking
a target in the preview stores it that way.

### After an export

The export dialog's **Preset** menu applies the presets above to one export; **Save to project**
writes the preset's aspect, reframing and format settings into the project (one undo step) so the
editor previews them. When an export finishes, **Show in folder** opens its folder, **Copy** puts
the file on the clipboard (a `file://` URI as `text/uri-list` through `wl-copy`), to paste into
a chat or file manager, and **Copy path** copies its path as text.

## Environment

- `OMAREEL_DEBUG=1` appends `gpu-screen-recorder` stderr to `/tmp/omareel.log`.
- `OMAREEL_PREVIEW_STATS=1` plays an opened recording and logs preview frame rate, average
  frame handling cost, and drops once per second.
- `OMAREEL_MUSIC_TRACE=1` logs each background-music sync in the editor: output time, the
  music's target and reported positions in milliseconds, its gain, and whether it plays.
- `OMAREEL_CAPTURE=gsr` forces the fallback screen capture backend.
- `OMAREEL_NO_TRANSCRIBE=1` stops the editor transcribing recordings on its own.
- `OMAREEL_AGENT=claude|codex` runs that agent instead of Omarchy's default;
  `OMAREEL_AGENT_MODEL=NAME` asks it for a model (`haiku`, `sonnet`, `gpt-5-codex`…);
  `OMAREEL_AGENT_PROGRAM=PATH` runs another program in its place (tests use
  `tests/tools/fake-agent.py`, which speaks both agents' streams).
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
- `OMAREEL_SCREENSHOT_PANEL=background|shape|cursor|zoom|clip|camera|keystrokes|audio|annotate` selects an editor inspector
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
- `OMAREEL_SCREENSHOT_VIEW=export|export-shorts|aspect|background-expanded|background-gradient-3|background-gradient-7|rail-tooltip|camera-proof|agent|agent-polish` opens a transient
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
- `${XDG_CACHE_HOME}/omareel/omareel/` holds filmstrips and the speech measurements
  (`speech-v1/`); all of it can be deleted.
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
