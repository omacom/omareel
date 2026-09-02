# Reference app engineering notes

Static reverse-engineering notes for the extracted macOS Electron build. Values below come from `dist/D4qqfDv_.js`, renderer chunks, `dist-electron/index.js`, and cursor/background assets. Times are milliseconds unless stated otherwise. Property spelling is preserved because these names appear in saved projects.

## 1. Default project settings

This is the complete base settings object (`Iw` in the deobfuscated renderer), regrouped without adding fields.

### Background

```js
backgroundGradient: {
  start: { x: 0, y: 0 },
  end: { x: 1, y: 1 },
  stops: [
    { color: "#3f37c9", at: 0 },
    { color: "#8c87df", at: 1 }, // same HSL, lightness +20 percentage points
  ],
}
backgroundPaddingRatio: 10
backgroundColor: "#3f37c9"
backgroundImage: null
backgroundType: "system"
backgroundSystemName: "macOS/tahoe-light.jpg"
backgroundBlur: 0
```

### Frame, shape, shadow, and inset

```js
insetPadding: { bottom: 0, left: 0, right: 0, top: 0 }
insetColor: "#000000"
insetAlpha: 0.5
windowBorderRadius: 12
shadowIntensity: 0.75
shadowAngle: 90
shadowDistance: 25
shadowBlur: 20
shadowIsDirectional: false
deviceFrameKey: null
enableDeviceMockup: true
adjustDeviceFrameToRecordingSize: true
```

### Cursor

```js
cursorSize: 1.5
cursorSet: { id: "macos", variants: {} }
cursorRotateOnXMovementRatio: 0.5
cursorBaseRotation: 0
useDefaultCursorIfAppCursorHasLowResolution: true
alwaysUseDefaultCursor: false
hideNotMovingCursorAfterMs: null
loopCursorPositionBeforeEndMs: null
removeCurshorShakeTreshold: 500 // typo is part of the persisted key
optimizeOriginalCursorTypes: true
clickEffect: null
clickSoundEffect: null
hideCursor: false
mouseMovementSpring: { stiffness: 470, damping: 70, mass: 3 }
mouseClickSpring: { stiffness: 700, damping: 30, mass: 1 }
disableMouseMovementSpring: false
stopCursorMovementInLastPartMs: 0
```

### Zoom and motion blur

```js
screenMovementSpring: { mass: 2.25, stiffness: 200, damping: 40 }
alwaysKeepZoomedIn: false
motionBlurAmount: 1
motionBlurCursorAmount: 1
motionBlurScreenMoveAmount: 1
motionBlurScreenZoomAmount: 1
```

### Audio

```js
audioVolume: 1
muteMicrophone: false
muteSystemAudio: false
muteExternalDeviceAudio: false
improveMicrophoneAudio: true
backgroundAudioFileName: null
muteBackgroundAudio: false
backgroundAudioVolume: 0.05
clickSoundEffectVolume: 0.25
microphoneInStereoMode: false
```

### Camera and layout

```js
hideCamera: false
mirrorCamera: false
cameraRoundness: 0.25
cameraSize: 0.35
cameraPosition: "bottom-right"
cameraPositionPoint: { x: 1, y: 1 }
cameraScaleDuringZoom: 0.7
cameraAspectRatio: "original"
defaultLayout: {
  type: "both",
  cameraSize: 0.35,
  cameraPositionPoint: { x: 1, y: 1 },
}
```

### Captions and shortcut overlay

```js
showShortcuts: false
hiddenShortcuts: {}
showShortcutsWithSingleLetters: false
showTranscript: true
transcriptSizeRatio: 1
shortcutsSizeRatio: 1
```

### Export/output

```js
defaultOutputAspectRatio: null
```

New-project construction also adds `recordingRange: [0, durationMs]` and `recordingCrop: {x:0,y:0,width:1,height:1}` outside this base object. Full-screen/external-device initialization can override padding and radius to zero.

## 2. Zoom model

### Persisted representation

```js
{
  id: uuid,
  startTime: number,              // persisted source time
  endTime: number,                // persisted source time
  zoom: 2,
  type: "follow-click-groups",    // old/default automatic type
  snapToEdgesRatio: 0.25,
  manualTargetPoint: { x: 0.5, y: 0.5 },
  glideDirection: null,
  glideSpeed: 0.5,
  isDisabled: false,
  isSystem: false,
  hasInstantAnimation: false,
}
```

The model exposes `sourceStartTime`/`sourceEndTime` getters for persisted `startTime`/`endTime`. The editor offers `type: "follow-mouse"` as **Auto** and `type: "manual"` as **Manual**; all non-`manual` values take the automatic path, which preserves old `follow-click-groups` projects.

System-generated gaps use:

```js
{
  id: `auto-mouse-follow-${startTime}`,
  startTime, endTime,
  zoom: 1,
  type: "follow-mouse",
  snapToEdgesRatio: 0.5,
  manualTargetPoint: { x: 0.5, y: 0.5 },
  glideDirection: null,
  glideSpeed: 0.5,
  isDisabled: false,
  isSystem: true,
  hasInstantAnimation: false,
}
```

These fill non-user-zoom intervals. The render path uses them for `alwaysKeepZoomedIn` when the selected output ratio is vertical; `zoom: 1` still retains the output's initial fill zoom.

### Levels and editing constraints

- Default zoom is `2`. The editor slider is continuous from `1` through `4`.
- Increase/decrease commands step by `0.2`, rounded and clamped to `[1, 4]`.
- Number shortcuts select `1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 2.2, 2.4, 2.6`; `0` selects the default (`2`).
- A normal zoom range has a `1000` ms minimum editing duration. A split must leave at least `100` ms on each side.

### Automatic target and edge behavior

1. Events within the zoom interval are put in source-time order. If the interval has none, the closest event before or after its start is used.
2. Events are accumulated into consecutive groups while their bounding box fits inside 50% of the available visible width and 70% of the available visible height at that zoom. An event that no longer fits begins a new group.
3. The target is the current group's bounding-box center; a group becomes active at its first event time.
4. Manual mode instead maps `manualTargetPoint` from normalized crop coordinates and forces edge-snap ratio `0`.
5. For auto mode, compute `maxSnapRatio = visibleAreaAtZoom / contentAreaSize` per axis. Clamp configured `snapToEdgesRatio` to that maximum, then map each normalized cursor coordinate from `[ratio, 1-ratio+0.0001]` to `[0,1]` with clamping. The resulting zoomed body rectangle is finally constrained to the output frame.

The scale actually rendered is `range.zoom * sizes.initialZoom`.

### Transitions

Zoom position and scale do not use a fixed-duration tween or cubic Bézier. They are targets for the physical `screenMovementSpring` default `{mass:2.25, stiffness:200, damping:40}`, so duration is settling-dependent. `hasInstantAnimation` bypasses the spring within 100 ms of either range boundary. Linear clamped interpolation is used for coordinate mapping, not as the zoom-in/out easing.

### Auto-zoom generation from clicks

For each click record at integer time `t`:

```text
start = max(floor(t) - 300, 0)
end   = min(floor(t) + 2500, recordingDuration - 800)
```

- Clicks at or after `recordingDuration - 1000` are ignored.
- Overlapping/adjacent ranges are unioned; gaps of at most `2500` ms are filled/merged.
- External-device recordings generate no automatic zooms.
- There is no second minimum-duration pass in this generator, so an end-of-recording auto range can be shorter than the editor's 1000 ms minimum.

## 3. Cursor model

### Samples and smoothing

Input JSON is split into `mousemoves-<session>.json` and `mouseclicks-<session>.json`. Consumed event fields are:

```js
{ processTimeMs, recordingTimeMs, x, y, cursorId, type }
// observed type values include mouseMoved, mouseDragged, mouseDown, mouseUp
```

On load, moves are decimated at the recorded display refresh rate (window fallback `initialDisplayRefreshRate`, final fallback `60` Hz). `frameMs = floor(1000/fps)`. The first sample and every cursor-ID change are kept. Otherwise a sample is dropped when the next raw sample is still inside the frame window from the last kept sample, or when its Euclidean displacement from the last kept sample is under `1` pixel. This is an effective maximum rate, not proof of the native writer's raw sampling rate.

The cursor position target is run through `mouseMovementSpring = {stiffness:470,damping:70,mass:3}`. Global `disableMouseMovementSpring` and per-slice `disableSmoothMouseMovement` bypass it. Original cursor-ID changes are simplified when `optimizeOriginalCursorTypes` is true.

Rotation looks back `400` ms:

```text
rotation = clamp((x[now] - x[now-400]) * 0.03 * cursorRotateOnXMovementRatio,
                 -20, 20) + cursorBaseRotation
```

### Click and idle animation

- Cursor press: target scale is `0.8`, beginning up to `130` ms before the next `mouseDown`, held until `mouseUp`. The active renderer's spring is `{stiffness:300,damping:30,mass:0.3}`.
- `clickEffect.type === "circle"`: black ring, radius `16 * cursorSize`, line width `2`, graphic alpha `0.6`; it tracks downs for 450 ms. Over normalized elapsed time `0..1` from the first 150 ms: scale `0.2 -> 3.5`, alpha knots `[0, .05, .8, 1] -> [0,1,0,0]`, blur `[0,0,0,20]`; spring `{500,30,0.3}`.
- `clickEffect.type === "ripple"`: downs live for `250` ms. The distortion uses amplitude `0 -> 2 -> 0` at normalized times `[0,.4,1]` with `easeInOutSine`, wavelength `0 -> max(containerWidth,containerHeight)/2`, base wavelength `400`, radius `100`, speed `6.4`, brightness `1`, and filter resolution `3`.
- `hideNotMovingCursorAfterMs` hides after the previous move is older than the configured value, using `{alpha:0, scale:0.8, blur:5}`. It pre-reveals when the next move is under `250` ms away and hides before the first movement when that first event is later than the threshold.
- Per-slice `hideCursor` uses the same hidden style. Global `hideCursor` omits the cursor entirely.
- `stopCursorMovementInLastPartMs` freezes at its cutoff. `loopCursorPositionBeforeEndMs` uses the position at the recording's source start during the final configured interval.

The persisted default `mouseClickSpring: {700,30,1}` was found only in project defaults; the active click-scale renderer imports the hard-coded `{300,30,0.3}` spring above.

### Cursor assets and hotspots

`assets/cursors/sets/macos` contains 42 SVG types. Its filenames encode a normalized percentage hotspot as `__x-y` (inferred from the naming convention; there is no `index.ts` in this directory):

| Hotspot | Cursor names |
|---|---|
| `50%,50%` | `beachball`, `cell`, `closedhand`, `cross`, `crosshair`, `help`, `ibeam`, `ibeamstroke`, `ibeamvertical`, `ibeamverticalstroke`, `move`, `openhand`, `resizedown`, `resizeeast`, `resizeeastwest`, `resizeleft`, `resizeleftright`, `resizenorth`, `resizenortheast`, `resizenortheastsouthwest`, `resizenorthsouth`, `resizenorthwest`, `resizenorthwestsoutheast`, `resizeright`, `resizesouth`, `resizesoutheast`, `resizesouthwest`, `resizeup`, `resizeupdown`, `resizewest`, `screenshotselection`, `screenshotwindow` |
| `23%,0%` | `busybutclickable`, `copy`, `notallowed`, `proof` |
| `26%,21%` | `contextualmenu` |
| `63%,29%` | `makealias` |
| `34%,24%` | `pointer` |
| `39%,26%` | `pointinghand` |
| `43%,43%` | `zoomin`, `zoomout` |

The older `assets/cursors/macos/index.ts` defines 35×35 assets, center hotspot `(17.5,17.5)`, with overrides:

```js
notallowed:[8,0], copy:[8,0], poof:[8,0], busybutclickable:[8,0],
makealias:[21.75,10], contextualmenu:[9,7.4], pointer:[12,8.4],
pointinghand:[13.5,9], zoomout:[15,15], zoomin:[15,15]
```

Its native AppKit cursor names are:

```text
arrow, iBeam, crosshair, closedHand, openHand, pointingHand, resizeLeft,
resizeRight, resizeLeftRight, resizeUp, resizeDown, resizeUpDown,
disappearingItem, iBeamCursorForVerticalLayout, operationNotAllowed,
dragLink, dragCopy, contextualMenu, windowResizeNorthEast,
windowResizeNorthWestSouthEast, zoomIn, windowResizeNorthWest,
windowResizeSouthEast, windowResizeEastWest, windowResizeNorth,
windowResizeEast, windowResizeNorthSouth, windowResizeNorthEastSouthWest,
windowResizeWest, windowResizeSouth, windowResizeSouthWest,
bottomRightResize, bottomLeftResize, topRightResize, topLeftResize,
help, notAllowed, makeAlias, copy, zoomOut, busyButClickable
```

`assets/cursors/common/index.ts` maps both IDs below to type `figma-pointer`; it defines no hotspot:

```text
custom-e72cb45156415a88caffcd4aba7905fd7b107f48
custom-94d183b2b18f909749199da7e8c75bf9d9a68f5d
```

## 4. Aspect ratio and export presets

### Output aspect ratios

| Name | Ratio | UI note |
|---|---:|---|
| Auto | recording + padding | `ratio: null` |
| Wide | 16:9 | Horizontal video not cropped on major social platforms |
| Square | 1:1 | |
| Classic | 4:3 | |
| Vertical | 9:16 | TikTok/Instagram stories |
| Tall | 3:4 | |
| Portrait | 4:5 | Instagram/Facebook feeds |

### Export settings

| Format | Heights | FPS choices | Quality modes |
|---|---|---|---|
| MP4 | 720, 1080, 2160 (4K) | 60, 50, 30, 25, 24, 20, 10 | `studio`, `social-media`, `web-high`, `web-low` |
| GIF | 480, 720, 1080 | 50, 30, 25, 20, 15, 10 | `studio`, `social-media` |

Web MP4 qualities are restricted to at most 1080p. Stored export fields are `{format, quality, fps, height, gifDisableLoop, shareableLinkVisbility}` (the misspelling is persisted). Targets are `file`, `clipboard`, and `link`.

Defaults:

```js
GIF preference:  { format:"gif", fps:15, quality:"studio", height:480 }
MP4 preference:  { format:"mp4", fps:60, quality:"social-media", height:720 }
shareable link:  { format:"mp4", fps:60, quality:"social-media", height:1080 }
```

The active MP4 frame encoder is WebCodecs H.264 High profile, codec string `avc1.640033`, `latencyMode:"quality"`, `bitrateMode:"constant"`, preferring hardware and falling back to software. No end-user HEVC/ProRes choice was found. The muxer library contains generic AVC/HEVC/VP9/AV1 support, but the export path requests AVC. Bitrate is:

```text
floor(width * height * fps * multiplier)
web-low: 0.0075; web-high: 0.0175; social-media: 0.05; studio: 0.3
```

When audio is present, FFmpeg mixes inputs with `amix ... normalize=0`, pads to the video, and writes AAC at `192k`; video is copied. With no additional audio inputs, FFmpeg remuxes with `-c:v copy -c:a copy`.

GIF compilation pipes FFmpeg into gifsicle:

```text
FFmpeg normal:      -i <rendered.mp4> -f gif - -progress pipe:2
FFmpeg Studio:      -i <rendered.mp4> -filter_complex
                    "[0:v] split [a][b];[a] palettegen [p];[b][p] paletteuse"
                    -f gif - -progress pipe:2
gifsicle:           --optimize=3 --lossy=30 [--no-loopcount]
                    --careful --verbose -o <output.gif>
```

`--no-loopcount` is added when `gifDisableLoop` is true.

## 5. Project file format on disk

`.project-bundle` is a macOS package/directory, not a monolithic file:

```text
Example.project-bundle/
├── project.json
├── meta.json
├── recording-markers.json          # optional/read tolerantly
├── assets/                         # copied custom backgrounds/audio/etc.
├── voiceovers/                     # generated/recorded voice-over media
└── recording/
    ├── metadata.json
    ├── metadata-raw.json            # optional backup of raw metadata
    ├── cursors.json
    ├── cursors/<cursor-id>.png
    ├── keystrokes-<session>.json
    ├── mouseclicks-<session>.json
    ├── mousemoves-<session>.json
    ├── <display/window/device video and audio files>
    └── <optional window/device bounds-change JSON>
```

`meta.json` is written as:

```js
{ version: app.getVersion(), requiredVersion: "2.4.0-beta", createdAt: Date }
```

The loader rejects projects older than app version `2.0.13` and projects whose `requiredVersion` is newer than the running app.

Media/event filenames are authoritative in `recording/metadata.json`, under recorder session properties such as `outputFilename`, `keyStrokesFilename`, `mouseClicksFilename`, `mouseMovesFilename`, and `changesFilename`; do not hard-code them. The import-video converter's canonical session-0 names are:

```text
channel-1-display-0.mp4
channel-1-system-audio-0.m4a       # optional
keystrokes-0.json
mouseclicks-0.json
mousemoves-0.json
cursors.json
metadata.json
```

`project.json` holds the project ID/name/dates/config plus `scenes`. A new recording scene starts with `{id,name:"Default",type:"recording",sessionIndex:0,slices,zoomRanges,layouts,masks,resolvedTypingSpeedIncreaseSuggestions,voiceOvers}`.

## 6. Background presets

Wallpaper directories (file counts): `Energy` (23), `Glassmorphism` (10), `Iridescent` (17), `Midnight` (18), `Radiant` (20), `Raycast` (31), `Spring` (24), `Sunset` (21), and `macOS` (16).

The gradient catalog contains 69 ordered color arrays. Twenty are generated two-stop palettes `[base, HSL-lighten(base,20)]` from:

```text
#111111 #888888 #eeeeee #FCBF49 #3f37c9 #e9edc9 #cdb4db #ffc8dd
#ffafcc #bde0fe #a2d2ff #264653 #2a9d8f #f4a261 #e76f51 #00b4d8
#5e548e #bc4749 #7209b7 #ff0054
```

The other 49 ordered palettes are:

```text
[#ffcdb2,#ffb4a2,#e5989b,#b5838d,#6d6875]
[#111111,#222222]
[#03045e,#023e8a,#0077b6,#0096c7,#00b4d8,#48cae4,#90e0ef,#ade8f4,#caf0f8]
[#fec5bb,#fcd5ce,#fae1dd,#f8edeb,#e8e8e4,#d8e2dc,#ece4db,#ffe5d9,#ffd7ba,#fec89a]
[#ffadad,#ffd6a5,#fdffb6,#caffbf,#9bf6ff,#a0c4ff,#bdb2ff,#ffc6ff,#fffffc]
[#03071e,#370617,#6a040f,#9d0208,#d00000,#dc2f02,#e85d04,#f48c06,#faa307,#ffba08]
[#cb997e,#eddcd2,#fff1e6,#f0efeb,#ddbea9,#a5a58d,#b7b7a4]
[#cdb4db,#ffc8dd,#ffafcc,#bde0fe,#a2d2ff]
[#ccd5ae,#e9edc9,#fefae0,#faedcd,#d4a373]
[#006d77,#83c5be,#edf6f9,#ffddd2,#e29578]
[#7400b8,#6930c3,#5e60ce,#5390d9,#4ea8de,#48bfe3,#56cfe1,#64dfdf,#72efdd,#80ffdb]
[#ede0d4,#e6ccb2,#ddb892,#b08968,#7f5539,#9c6644]
[#d8f3dc,#b7e4c7,#95d5b2,#74c69d,#52b788,#40916c,#2d6a4f,#1b4332,#081c15]
[#22223b,#4a4e69,#9a8c98,#c9ada7,#f2e9e4]
[#f08080,#f4978e,#f8ad9d,#fbc4ab,#ffdab9]
[#ffcbf2,#f3c4fb,#ecbcfd,#e5b3fe,#e2afff,#deaaff,#d8bbff,#d0d1ff,#c8e7ff,#c0fdff]
[#007f5f,#2b9348,#55a630,#80b918,#aacc00,#bfd200,#d4d700,#dddf00,#eeef20,#ffff3f]
[#03045e,#0077b6,#00b4d8,#90e0ef,#caf0f8]
[#10002b,#240046,#3c096c,#5a189a,#7b2cbf,#9d4edd,#c77dff,#e0aaff]
[#dad7cd,#a3b18a,#588157,#3a5a40,#344e41]
[#582f0e,#7f4f24,#936639,#a68a64,#b6ad90,#c2c5aa,#a4ac86,#656d4a,#414833,#333d29]
[#012a4a,#013a63,#01497c,#014f86,#2a6f97,#2c7da0,#468faf,#61a5c2,#89c2d9,#a9d6e5]
[#006466,#065a60,#0b525b,#144552,#1b3a4b,#212f45,#272640,#312244,#3e1f47,#4d194d]
[#0466c8,#0353a4,#023e7d,#002855,#001845,#001233]
[#edf2fb,#e2eafc,#d7e3fc,#ccdbfd,#c1d3fe,#b6ccfe,#abc4ff]
[#ecf8f8,#eee4e1,#e7d8c9,#e6beae,#b2967d]
[#e9f5db,#cfe1b9,#b5c99a,#97a97c,#87986a,#718355]
[#ff7b00,#ff8800,#ff9500,#ffa200,#ffaa00,#ffb700,#ffc300,#ffd000,#ffdd00,#ffea00]
[#590d22,#800f2f,#a4133c,#c9184a,#ff4d6d,#ff758f]
[#f7d1cd,#e8c2ca,#d1b3c4,#b392ac,#735d78]
[#0d1b2a,#1b263b,#415a77,#778da9,#e0e1dd]
[#b7094c,#a01a58,#892b64,#723c70,#5c4d7d,#455e89,#2e6f95,#1780a1,#0091ad]
[#004b23,#006400,#007200,#008000,#38b000,#70e000,#9ef01a,#ccff33]
[#6b9080,#a4c3b2,#cce3de,#eaf4f4,#f6fff8]
[#ff0a54,#ff477e,#ff5c8a,#ff7096,#ff85a1,#ff99ac,#fbb1bd,#f9bec7,#f7cad0,#fae0e4]
[#641220,#6e1423,#85182a,#a11d33,#a71e34,#b21e35,#bd1f36,#c71f37,#da1e37,#e01e37]
[#ff4800,#ff5400,#ff6000,#ff6d00,#ff7900,#ff8500,#ff9100,#ff9e00,#ffaa00,#ffb600]
[#231942,#5e548e,#9f86c0,#be95c4,#e0b1cb]
[#048ba8,#0db39e,#16db93,#83e377,#b9e769,#efea5a,#f1c453,#f29e4c]
[#757bc8,#8187dc,#8e94f2,#9fa0ff,#ada7ff,#bbadff,#cbb2fe,#dab6fc,#ddbdfc,#e0c3fc]
[#250902,#38040e,#640d14,#800e13,#ad2831]
[#99e2b4,#88d4ab,#78c6a3,#67b99a,#56ab91,#469d89,#358f80,#248277,#14746f,#036666]
[#dec9e9,#dac3e8,#d2b7e5,#c19ee0,#b185db,#a06cd5,#9163cb,#815ac0,#7251b5,#6247aa]
[#ea698b,#d55d92,#c05299,#ac46a1,#973aa8,#822faf,#6d23b6,#6411ad,#571089,#47126b]
[#ffe0e9,#ffc2d4,#ff9ebb,#ff7aa2,#e05780,#b9375e,#8a2846,#602437,#522e38]
[#ffe169,#fad643,#edc531,#dbb42c,#c9a227,#b69121,#a47e1b,#926c15,#805b10,#76520e]
[#b76935,#a56336,#935e38,#815839,#6f523b,#5c4d3c,#4a473e,#38413f,#263c41,#143642]
[#e3f2fd,#bbdefb,#90caf9,#64b5f6,#42a5f5,#2196f3]
[#5465ff,#788bff,#9bb1ff,#bfd7ff,#e2fdff]
```

The default persisted gradient is the two explicit stops shown in section 1, from `(0,0)` to `(1,1)`.

## 7. Timeline editing and clip model

The main recording clip is called a **slice**:

```js
{
  id: uuid,
  timeScale: 1,
  sourceStartMs: 0,
  sourceEndMs: recording.durationMs,
  volume: 1,
  systemAudioVolume: 1,
  externalDeviceAudioVolume: 1,
  hideCursor: false,
  disableSmoothMouseMovement: false,
}
```

Derived timing is:

```text
sourceDurationMs   = sourceEndMs - sourceStartMs
playbackDurationMs = sourceDurationMs * timeScale
speed              = timeScale === 0 ? Infinity : 1 / timeScale
playbackStartMs     = sum(previous slices' playbackDurationMs)
```

- Trim edits `sourceStartMs`/`sourceEndMs`, clamps against adjacent slices so they cannot overlap, and refuses a shortening operation below the scene minimum of `100` ms.
- A split at source time `t` is allowed only when both resulting sides are at least `100` ms. The right slice receives a new ID and copies time scale, all three volume fields, and both cursor flags.
- `removeTrim("start"|"end")` expands to the adjacent slice boundary, or to source `0`/recording end at the outer edges.
- Merge-with-next adopts the next slice's `sourceEndMs`; merge-with-previous adopts the previous slice's `sourceStartMs`, then deletes the merged neighbor.
- Removing the last remaining slice is forbidden. An optional ripple-removal mode joins the previous and next source boundaries around a removed middle slice.
- Speed choices are `0.5, 0.75, 1, 1.2, 1.4, 1.6, 1.8, 2, 3, 4, 8, 16, 24`× and are stored inversely in `timeScale`.

Other timeline range types (`zoomRanges`, `layouts`, `masks`, and `voiceOvers`) live on the scene beside `slices`. Zoom/layout/mask items use source `startTime`/`endTime` and an `isDisabled` flag; voice-over items add file/source/attachment and timing data.

## Uncertain / not found

- The native input helper's raw event-writer cadence/schema was not present as readable source. The fields and display-rate decimation above are proven from the loader/renderer consumers.
- No fixed zoom-in/out duration or Bézier easing exists in the active path; spring settling is the transition. `glideDirection` and `glideSpeed` remain persisted but were not used by that path.
- The `sets/macos` percentage hotspots are inferred from filenames because that directory has no index file; the separate legacy `macos/index.ts` pixel hotspots are explicit.
- Gradient preset arrays contain ordered colors but no `at` values in the catalog constant; only the default gradient's `at: 0/1` positions are explicit.
- No selectable HEVC or ProRes export path was found. Generic muxer support should not be mistaken for an exposed encoder preset.

## Reproducibility

String-table lookup and constant-expression deobfuscator:

```text
/tmp/ss/tools/deobfuscate-renderer.js
```

Run it with:

```bash
node /tmp/ss/tools/deobfuscate-renderer.js \
  /tmp/ss/asar/dist/D4qqfDv_.js \
  /tmp/ss/tools/D4qqfDv_.deobfuscated.js
```
