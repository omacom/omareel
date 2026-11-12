# Omareel

Omareel is a native screen recorder and editor for Omarchy and Hyprland. It captures the
screen without the system cursor, records pointer and keyboard activity separately, and then
renders a smooth synthetic cursor, click effects, rounded framing, backgrounds, and automatic
cursor-following zooms. Finished projects export to MP4 or GIF.

> Screenshot placeholder — editor screenshot coming soon.

## Features

- Region, window, or focused-monitor recording through `gpu-screen-recorder` at 60 fps by default.
- Optional desktop and microphone audio in a single compatible audio track.
- Background music under the whole video, with volume, fades, looping, and automatic ducking
  under your voice.
- Non-destructive clip trimming, splitting, speed changes, and timeline zoom editing.
- Rubber-band multi-selection for zoom blocks, with grouped movement and deletion.
- Annotations: text labels, boxes with an optional spotlight, arrows, numbered steps, blur or
  pixelate regions baked into the export, and title cards, edited on the preview and timeline.
- Omarchy wallpaper and live accent-color integration.
- Synthetic cursor smoothing, click rings in theme colours, hiding when idle or while typing, and
  cursor-following auto zooms.
- Optional zoom-punch transitions that hide the jump at each cut, with the sound faded across it.
- Aspect presets, padding, rounded corners, inset borders, and configurable shadows.
- Camera rotation, horizontal flip, independent shadow/border styling, and 720p/1080p capture.
- Hardware-accelerated MP4 export with CPU fallback, plus GIF export.
- `omareel frame` renders any moment of the finished video, or a labelled contact sheet, as an
  image without exporting: a quick check of an edit for people and AI agents.
- Speech to text with the dictation engine chosen in Omarchy: captions, SRT/WebVTT subtitles,
  one-click trimming of pauses and filler words, and speeding up quiet stretches where the screen
  keeps changing (installs, builds, scrolling). Everything stays on this computer.
- Self-contained `.omareel` project bundles that preserve the source and editor state.
- Scriptable editing for AI agents and scripts: `omareel describe` prints the edited timeline
  as JSON, `omareel apply` runs checked edit operations, and an open editor follows along.
  `omareel skill install` teaches your coding agents (Claude Code, Codex, pi, Gemini, Hermes)
  to use them.
- Editing with AI in the editor: Polish (`Ctrl+Shift+P`), "Ask the editor…", and Write
  description run the coding agent chosen in Omarchy (Claude Code or Codex) on the recording,
  only when you click. You watch each change land, and each is one undo step. Chapters, a
  title and a description go in `chapters.json`, show on the timeline, and copy as a YouTube
  description.

## Install

On Omarchy, install from the Omarchy package repository:

```sh
omarchy-pkg-add omareel
```

Prebuilt `x86_64` and `aarch64` packages are also attached to every
[GitHub release](https://github.com/omacom/omareel/releases); install one with
`sudo pacman -U omareel-<version>-<arch>.pkg.tar.zst`.

To build from a checkout on Arch Linux:

```sh
cd pkg
makepkg -si
```

On Omarchy, `./install-omarchy` first adds any missing runtime/build packages with
`omarchy-pkg-add` (falling back to `pacman`) and then installs the same PKGBUILD.

`gifsicle` is optional and can be installed for additional GIF optimization.

Omareel keeps its recording bar and camera self-view out of the captured video with a small
Hyprland plugin that ships in the package and is loaded on demand. The plugin is built for the
exact Hyprland commit of the package repository; after a Hyprland upgrade it waits for a rebuilt
omareel package, and until then the overlays move off the recorded screen instead. See
[docs/USAGE.md](docs/USAGE.md#overlays-and-capture).

## Usage

Run `omareel` to open the launcher, or bind the smart recording toggle:

```lua
o.bind("SUPER + ALT + R", "Record with omareel", "omareel record")
```

The launcher's app id is `omareel`. Its compact 380×460 window floats without a window rule: the
fixed width is Hyprland's cue to float it, and Hyprland centres it on the focused monitor.

The default command uses one gesture: drag to select an area, click a window to snap to it, or
click the desktop to record the whole screen. Explicit modes remain available for dedicated
keybinds. For focused-monitor capture:

```lua
o.bind("SUPER + ALT + R", "Record with omareel", "omareel record --fullscreen")
```

The same command stops an active recording, finalizes its bundle, and opens it in the editor.
Useful commands include:

```sh
omareel record --region --with-desktop-audio
omareel edit ~/Videos/omareel/Recording.omareel
omareel export Recording.omareel -o recording.mp4
omareel frame Recording.omareel --at 12.5 -o frame.png
omareel frame Recording.omareel --count 12 --sheet -o sheet.jpg
omareel transcribe Recording.omareel
omareel export Recording.omareel -o recording.srt
omareel probe Recording.omareel
omareel describe Recording.omareel
omareel apply Recording.omareel edits.json
omareel agent Recording.omareel polish
omareel help
```

Existing installations can keep using the legacy `omarecord` command alias. On first run,
settings are copied from `~/.config/omarecord/settings.json` when the new settings file does not
exist, and existing `.omarecord` bundles remain editable in place.

### Stopping a recording

While recording, use any of these stop paths:

- Click **Stop** on the recording bar.
- Click the REC indicator in the Omarchy bar.
- Run `omareel record` or `omareel record --stop` again. This is especially useful for a
  keybind.
- Run `omareel record --cancel` to stop and permanently discard the recording bundle.

The recording bar appears at the top center of the recorded monitor. Turning on the webcam in
the launcher opens a floating camera self-view right away; drag it where you want it and it
stays there through the recording, using the same warm camera. Native capture keeps both the
bar and the self-view out of the file, wherever they sit. Pass `--no-selfview` to suppress only
the camera bubble. Pass `--no-bar` or set `OMAREEL_NO_BAR=1` before starting if you do not
want the bar to appear.

### Editor shortcuts

| Key | Action |
| --- | --- |
| `Space` | Play or pause |
| `Left` / `Right` | Step one frame |
| `Shift+Left` / `Shift+Right` | Seek one second |
| `S` | Split the selected clip at the playhead |
| `Z` | Add a zoom at the playhead |
| `T` / `R` / `A` / `N` / `B` | Add text, a box, an arrow, a step number, or a blur region |
| `Shift+T` | Add a title card |
| `Delete` | Delete the selected time range, annotation, zoom, or clip |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo |
| `Ctrl+S` | Save now |
| `Ctrl+E` | Open export |

## Bundle format

An `.omareel` bundle is a directory. `screen.mp4` is the cursor-free source recording;
`screen.mp4.ts` anchors input timestamps to the first video frame; `capture.json` describes the
region, scale, dimensions, frame rate, and audio; `input.jsonl` stores pointer, button, scroll,
and key events (keyboard shortcuts and non-text keys only: plain typing is never recorded); `project.json` contains non-destructive editor choices; `media/` holds inserted videos and
background music; and `thumb.jpg` is the
launcher thumbnail. `transcript.json`, once the recording is transcribed, holds its words
with times in recording seconds. Moving the whole directory preserves the editable project.

### Speech, captions and trimming

The editor transcribes a recording with sound in the background when it opens, using the
dictation engine chosen in Omarchy's Setup > Defaults > Dictation (voxtype, Hex or
Superwhisper; `omarchy install dictation voxtype` sets one up). With voxtype on a Whisper model
and `whisper-cpp` installed, the same model runs through `whisper-cli`, which times every word.
The Speech section of the editor turns captions on, saves subtitles, and trims pauses (silences
over a second while nothing changes on screen and the pointer stays put) and filler words (with
whisper-cpp's measured word times), each as one undo step. Pause trimming needs no engine.
"Speed up quiet parts" plays the silent stretches the screen does not stay still through faster
instead of cutting them. The speech and screen measurements are cached under
`~/.cache/omareel/omareel/speech-v1/`, keyed by `screen.mp4`, so a recording opens without
measuring again. See `docs/USAGE.md` for details.

## Troubleshooting

Pointer clicks and keys require membership in the `input` group. Check the current login with:

```sh
id -nG | grep input
```

If the group was added recently, log out and back in. Recording continues without click/key
data when input devices cannot be opened.

If `gpu-screen-recorder` fails, first try Omarchy's own screen recorder to confirm the GPU and
capture backend work, then check that the selected monitor/region is valid. Run the toggle with
debug logging enabled to append GSR stderr to `/tmp/omareel.log`:

```sh
OMAREEL_DEBUG=1 omareel record --region
```

Set the variable on the start invocation so the detached recorder inherits it. For the full CLI
and UI debug screenshot flags, see [docs/USAGE.md](docs/USAGE.md).

## Development

Building needs Rust (`rust` and `corrosion` on Arch) along with Qt 6 and CMake.

```sh
./bin/build
./bin/test
```

The program logic lives in Rust under [`crates/`](crates): `omareel_core` and `omareel_record`
(the recording side) are plain Rust, and `omareel_qt` exposes them to the Qt Quick interface
through CXX-Qt. See
[docs/rust-migration.md](docs/rust-migration.md) for how the code is being moved there.

Every pull request builds, tests, and packages `x86_64` and `aarch64`; tagged releases (`v*`) also attach the Arch packages and
plain tarballs to the GitHub release through `.github/workflows/release.yml`.

The frozen engineering contract is in [docs/SPEC.md](docs/SPEC.md).

## Credits

Interface icons are from Lucide, used under the ISC license. See
[`LICENSES/lucide.txt`](LICENSES/lucide.txt).

The interface is set in Inter, bundled under the SIL Open Font License 1.1. See
[`LICENSES/inter.txt`](LICENSES/inter.txt). Numbers, timecodes, and keys use your Omarchy
monospace font.
