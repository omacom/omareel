# omarecord

OmaRecord is a native screen recorder and editor for Omarchy and Hyprland. It captures the
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
- Self-contained `.omarecord` project bundles that preserve the source and editor state.

## Install

On Arch Linux, build and install the local package from the repository checkout:

```sh
cd pkg
makepkg -si
```

On Omarchy, the installer adds any missing runtime/build packages with `omarchy-pkg-add`
(falling back to `pacman`) and installs the same PKGBUILD:

```sh
./install-omarchy
```

`gifsicle` is optional and can be installed for additional GIF optimization.

## Usage

Run `omarecord` to open the launcher, or bind the smart recording toggle:

```lua
o.bind("SUPER + ALT + R", "Record with omarecord", "omarecord record")
```

The launcher's app id is `omarecord-launcher`. To keep its compact 380×460 window floating and
centred, add this optional Omarchy window rule:

```lua
o.window("^omarecord-launcher$", { float = true, center = true, size = "380 460" })
```

The default command uses one gesture: drag to select an area, click a window to snap to it, or
click the desktop to record the whole screen. Explicit modes remain available for dedicated
keybinds. For focused-monitor capture:

```lua
o.bind("SUPER + ALT + R", "Record with omarecord", "omarecord record --fullscreen")
```

The same command stops an active recording, finalizes its bundle, and opens it in the editor.
Useful commands include:

```sh
omarecord record --region --with-desktop-audio
omarecord edit ~/Videos/omarecord/Recording.omarecord
omarecord export Recording.omarecord -o recording.mp4
omarecord probe Recording.omarecord
omarecord help
```

### Stopping a recording

While recording, use any of these stop paths:

- Click **Stop** on the recording bar.
- Click the REC indicator in the Omarchy bar.
- Run `omarecord record` or `omarecord record --stop` again. This is especially useful for a
  keybind.
- Run `omarecord record --cancel` to stop and permanently discard the recording bundle.

With two monitors, the recording bar appears at the top center of the monitor that is not being
recorded, along with the camera self-view when webcam capture is enabled. Region and window
recordings place the self-view outside the captured rectangle when it fits. With only one
monitor and a full-screen capture, launcher settings can use the one-time share picker so the
self-view is omitted; otherwise a one-time warning explains that it will be visible. Pass
`--no-selfview` to suppress only the camera bubble. Pass `--no-bar` or set
`OMARECORD_NO_BAR=1` before starting if you do not want the bar to appear.

### Editor shortcuts

| Key | Action |
| --- | --- |
| `Space` | Play or pause |
| `Left` / `Right` | Step one frame |
| `Shift+Left` / `Shift+Right` | Seek one second |
| `S` | Split the selected clip at the playhead |
| `Z` | Add a zoom at the playhead |
| `Delete` | Delete the selected zoom, or selected clip |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo |
| `Ctrl+S` | Save now |
| `Ctrl+E` | Open export |

## Bundle format

An `.omarecord` bundle is a directory. `screen.mp4` is the cursor-free source recording;
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
debug logging enabled to append GSR stderr to `/tmp/omarecord.log`:

```sh
OMARECORD_DEBUG=1 omarecord record --region
```

Set the variable on the start invocation so the detached recorder inherits it. For the full CLI
and UI debug screenshot flags, see [docs/USAGE.md](docs/USAGE.md).

## Development

```sh
./bin/build
./bin/test
```

The frozen engineering contract is in [docs/SPEC.md](docs/SPEC.md).

## Credits

Interface icons are from Lucide, used under the ISC license. See
[`LICENSES/lucide.txt`](LICENSES/lucide.txt).
