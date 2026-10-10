# Omareel

Omareel is a native screen recorder and editor for Omarchy and Hyprland. It captures the
screen without the system cursor, records pointer and keyboard activity separately, and then
renders a smooth synthetic cursor, click effects, rounded framing, backgrounds, and automatic
cursor-following zooms. Finished projects export to MP4 or GIF.

> Screenshot placeholder — editor screenshot coming soon.

## Features

- Region, window, or focused-monitor recording through `gpu-screen-recorder` at 60 fps by default.
- Optional desktop and microphone audio in a single compatible audio track.
- Non-destructive clip trimming, splitting, speed changes, and timeline zoom editing.
- Rubber-band multi-selection for zoom blocks, with grouped movement and deletion.
- Omarchy wallpaper and live accent-color integration.
- Synthetic cursor smoothing, click rings, idle hiding, and cursor-following auto zooms.
- Aspect presets, padding, rounded corners, inset borders, and configurable shadows.
- Camera rotation, horizontal flip, independent shadow/border styling, and 720p/1080p capture.
- Hardware-accelerated MP4 export with CPU fallback, plus GIF export.
- Self-contained `.omareel` project bundles that preserve the source and editor state.

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
omareel probe Recording.omareel
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
| `Delete` | Delete the selected time range, zoom, or clip |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo |
| `Ctrl+S` | Save now |
| `Ctrl+E` | Open export |

## Bundle format

An `.omareel` bundle is a directory. `screen.mp4` is the cursor-free source recording;
`screen.mp4.ts` anchors input timestamps to the first video frame; `capture.json` describes the
region, scale, dimensions, frame rate, and audio; `input.jsonl` stores pointer, button, scroll,
and key events; `project.json` contains non-destructive editor choices; and `thumb.jpg` is the
launcher thumbnail. Moving the whole directory preserves the editable project.

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

```sh
./bin/build
./bin/test
```

Tagged releases (`v*`) build and test on `x86_64` and `aarch64` and attach Arch packages and
plain tarballs to the GitHub release through `.github/workflows/release.yml`.

The frozen engineering contract is in [docs/SPEC.md](docs/SPEC.md).

## Credits

Interface icons are from Lucide, used under the ISC license. See
[`LICENSES/lucide.txt`](LICENSES/lucide.txt).
