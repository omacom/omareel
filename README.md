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
- Omarchy wallpaper and live accent-color integration.
- Synthetic cursor smoothing, click rings, idle hiding, and cursor-following auto zooms.
- Aspect presets, padding, rounded corners, inset borders, and configurable shadows.
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

Run `omarecord` to open the launcher, or bind the recording toggle in Omarchy's Lua keybinding
configuration:

```lua
o.bind("SUPER + ALT + R", "Record with omarecord", "omarecord record --region")
```

For focused-monitor capture instead:

```lua
o.bind("SUPER + ALT + R", "Record with omarecord", "omarecord record --fullscreen")
```

The same command stops an active recording, finalizes its bundle, and opens it in the editor.
`omarecord record` with no mode flag defaults to region selection. Useful commands include:

```sh
omarecord record --region --with-desktop-audio
omarecord edit ~/Videos/omarecord/Recording.omarecord
omarecord export Recording.omarecord -o recording.mp4
omarecord probe Recording.omarecord
omarecord help
```

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
