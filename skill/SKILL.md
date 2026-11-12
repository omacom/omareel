---
name: omareel
description: |
  Edit and export Omareel screen recordings (.omareel bundles) from the command line:
  cut, trim, speed up, zoom, restyle, and export to MP4 or GIF. Use for ANY request to
  edit, tighten, shorten, polish, review, or export an Omareel recording or a
  .omareel/.omarecord bundle, or a screen recording made with Omareel on Omarchy.
---

# Omareel

An Omareel recording is a bundle directory (`Name.omareel`), usually in
`~/Videos/omareel/`. The video is never changed: every edit lives in the bundle's
`project.json`, which the `omareel` command reads and writes for you. No command
here needs a display.

## Workflow

1. Read: `omareel describe <bundle>` prints the edited timeline as JSON. Read it
   before you edit, and again after.
2. Edit: write the operations as JSON and run
   `omareel apply <bundle> ops.json` (or `-` to read stdin). Check first with
   `--dry-run`; add `--describe` to get the new timeline back. A failing
   operation saves nothing and the report names it, so fix it and run the whole
   list again.
3. Export when asked: `omareel export <bundle> -o out.mp4` (or `out.gif`).
   Options: `--width W` (MP4 or GIF; height follows the aspect), `--fps N`,
   `--quality low|medium|high|best|web-low|web-high|social|studio`. Without
   them, the project's export settings apply (`settings.exportSettings`).

`apply` reports JSON like this:
`{"ok": true, "saved": true, "output_duration": 42.5, "clips": 3, "zooms": 4,
"results": [{"op": "delete", "output_duration": 44.1}, {"op": "zoom", "id": "z-…"}],
"warnings": [], "describe": {…}}`. `describe` is only there with `--describe`.
Each result's `output_duration` is the video's length after that operation,
not the amount cut. On failure the report is `{"ok": false, "error":
"operation 2 (speed): …"}` and nothing is saved.

`omareel help` lists every command in this version. Commands exit 0 on
success and 1 on failure. `describe` and `apply` print JSON on stdout.

## Times

- **Output** seconds are times in the edited video. They are the default.
- **Source** seconds are times in the original recording (`screen.mp4`).

Operations run in order, and output times refer to the timeline as it is when
that operation runs, so a cut moves everything after it. When a batch cuts
several things, either work from the end of the video backwards, or add
`"timeline": "source"` to each operation and give source times, which no edit
moves. `"timeline": "source"` works on any operation that takes times (`at`,
`from`, `to`). Every click, shortcut, idle stretch and zoom in `describe` lists both
its source time and the output times where it plays (an empty list means it
was cut).

## Reading `describe`

- `output.duration` is the edited length. `source` describes the recording:
  its size, frame rate, audio, and camera.
- `clips` are in play order. A clip is either a piece of the recording (`source`
  span, `speed`, `layout`) or an inserted video (`media`, `fit`). Its `output`
  is where it plays.
- `zooms` have a `source` span, the `output` spans where they play, a `level`,
  and a `target`: `"auto"` follows the cursor, or `{x, y}` from 0 to 1.
- `activity` is what the person did:
  - `clicks`: positions normalized 0–1.
  - `shortcuts`: key combinations. Plain typing is never recorded.
  - `idle`: stretches of 2 s or more with no input at all. Idle input doesn't
    mean nothing happens on screen (a build running, a page loading), so treat
    idle stretches as candidates to cut or speed up, not certainties.
- `settings` holds the rest of `project.json`: background, frame, cursor,
  keystrokes, audio, camera, and export settings. Their dotted paths are
  what `set` takes.
- `allowed` lists the clip speeds, layouts, and fits the editor accepts.

## Operations

| op | fields |
| --- | --- |
| `split` | `at` |
| `delete` | `from`, `to` |
| `trim` | `clip`, `in` and/or `out` (source seconds; an inserted video's own) |
| `remove-clip` | `clip` |
| `merge` | `clip`, `with`: `next` or `previous` |
| `move-clip` | `clip` (inserted videos only), `direction`: `earlier` or `later` |
| `reset-trims` | `clip` |
| `speed` | `clip` or `at`, `speed` (one of `allowed.speeds`) |
| `layout` | `clip` or `at`, `layout`: `default` (screen + camera bubble), `camera` (webcam full frame), `screen` (no bubble) |
| `fit` | `clip` or `at`, `fit`: `fit`, `fill`, `framed` (inserted videos) |
| `insert` | `file` (a video), `at` (default: the end) |
| `zoom` | `from`, `to`, `level` 1–4 (default 2), `target` `"auto"` or `{"x", "y"}` |
| `edit-zoom` | `id`, any of `from`, `to`, `level`, `target` |
| `remove-zoom` | `id` |
| `clear-zooms` | |
| `regenerate-zooms` | (rebuilds the automatic zooms around clicks) |
| `set` | `path`, `value`, e.g. `{"op": "set", "path": "keystrokes.enabled", "value": true}` |

Rules the editor enforces, which `apply` reports as errors:
- Zooms can't overlap, cover at least a second of the recording, and have a
  `level` from 1 to 4.
- Output times must be inside the video (0 to `output.duration`).
- A clip can't be shorter than 0.1 s, and a cut can't remove all of the recording.
- Speeds must be one of the allowed values.

A `set` that changes nothing comes back as a warning: the path doesn't exist,
or the value is the same.

```sh
omareel apply ~/Videos/omareel/Demo.omareel - <<'OPS'
[
  {"op": "delete", "from": 41.2, "to": 47.9, "timeline": "source"},
  {"op": "delete", "from": 0, "to": 1.4, "timeline": "source"},
  {"op": "speed", "at": 20, "speed": 2},
  {"op": "zoom", "from": 12, "to": 15.5, "level": 2.5, "target": "auto"},
  {"op": "set", "path": "keystrokes.enabled", "value": true}
]
OPS
```

## Good edits

- Trim the start and end: the first second or two before the first action, and
  the moment the person reaches for the stop button.
- Cut or speed up long idle stretches. Leave a beat, about 0.3–0.5 s, on each
  side so the cuts don't feel abrupt.
- Speed up stretches of repetitive work (2× to 4×). Keep explanations and
  results at 1×.
- Zoom where something small matters: a menu, a field, a line of code. Zoom a
  second before the click it explains. Prefer `"auto"`, which follows the
  cursor, unless the target is fixed. `regenerate-zooms` rebuilds the automatic
  zooms after big cuts.
- New recordings get automatic zooms around clicks, and clicks close together
  share one long zoom. To change one moment inside such a zoom, shorten it with
  `edit-zoom` (`to`, or `from`) and add a `zoom` for that moment, then another
  for the rest if it should stay zoomed. Changing `level` on the long zoom
  changes all of it. Cuts shrink the zooms they pass through and remove the
  ones they cut entirely.
- Turn on `keystrokes.enabled` when shortcuts are part of what is shown.
- Make one change at a time when the person is watching, and describe what
  changed in output times they can find on the timeline.

## Working with the person

- If they have the recording open in the Omareel editor (`omareel edit
  <bundle>`), it takes in what `apply` saves within half a second and shows
  "Updated from project.json". You don't need to close the editor. Each apply
  is usually one step they can undo with Ctrl+Z. Two applies within half a
  second can land as a single step, so pause between applies when the person
  wants to review them one by one.
- Don't edit `screen.mp4`, `input.jsonl`, or `capture.json`. They are the
  recording itself.
- Prefer `apply` to writing `project.json` by hand. `apply` checks every edit,
  and a hand-written mistake can make the editor ignore the file until it
  parses.
- Exports can take a while for long recordings. Tell the person where the file
  went (`export` prints the path on stdout).
