# Feature parity

This once-over compares omareel with the documented feature sets of reference app A and
reference app B. “Done” means the editor exposes the feature and the shared preview/export
composition supports it. “Partial” calls out a narrower implementation than either reference.

| Feature | Status in omareel (Done / Partial / Missing) | Notes/effort |
|---|---|---|
| Automatic zooms from clicks | Done | Click grouping and generated zoom ranges are implemented in `src/core/ZoomTimeline.cpp`; spring evaluation is in `src/core/MotionTrack.cpp`. |
| Show or hide cursor | Done | `cursor.visible` is persisted by `src/core/Project.cpp`, exposed in `src/ui/qml/CursorPanel.qml`, and respected by `CursorOverlay.qml`. |
| Cursor smoothing with a spring | Done | The fixed-rate spring track and raw bypass are in `src/core/MotionTrack.cpp`; controls are in `src/ui/qml/CursorPanel.qml`. |
| Cursor size | Done | Persisted as `cursor.size` by `src/core/Project.cpp` and rendered by `src/ui/qml/CursorOverlay.qml`. |
| Cursor sets and styles | Partial | `src/ui/qml/CursorPanel.qml` and `CursorOverlay.qml` provide Light arrow, Dark arrow, Dot, and Hand. There is no context-aware cursor-ID set or custom-set importer. |
| Natural and Smooth cursor motion presets | Done | The segmented presets in `src/ui/qml/CursorPanel.qml` select `{1,900,60}` and `{3,470,70}` springs while preserving custom spring controls. |
| Click ring/ripple effects | Partial | `src/core/MotionTrack.cpp` and `src/ui/qml/CursorOverlay.qml` render the expanding circle. A displacement-style ripple and additional visual variants are not implemented. |
| Click sounds | Partial | `cursor.clickSound` and the bundled soft sound are mixed during MP4 export by `src/render/Exporter.cpp`. Only one sound is available and preview is silent. |
| Hide idle cursor | Done | Idle detection and spring fade are implemented in `src/core/MotionTrack.cpp`; delay controls are in `src/ui/qml/CursorPanel.qml`. |
| Cursor rotation on movement | Done | Horizontal lookback rotation is computed in `src/core/MotionTrack.cpp` and applied in `src/ui/qml/CursorOverlay.qml` (Dot intentionally does not rotate). |
| Motion blur for cursor, zoom, and screen movement | Partial | `zoomStyle.motionBlur` applies velocity-scaled stage blur during zoom-centre movement in `src/ui/qml/Composition.qml`. Cursor-only blur and zoom-scale blur are missing. |
| Background: none | Done | The unframed path is implemented by `src/ui/qml/Composition.qml` and selectable in `src/ui/qml/BackgroundPanel.qml`. |
| Background: wallpaper | Done | Theme wallpaper discovery is in `src/core/OmarchyPaths.cpp`; selection and rendering are in `BackgroundPanel.qml` and `Background.qml`. |
| Background: gradient | Done | Presets and custom stops are stored by `src/core/Project.cpp` and rendered by `src/ui/qml/Background.qml`. |
| Background: plain colour | Done | `background.color` is exposed by `src/ui/qml/BackgroundPanel.qml` and rendered by `Background.qml`. |
| Background: image | Done | File selection, persisted path resolution, and cover rendering are in `src/ui/qml/BackgroundPanel.qml`, `src/ui/Editor.cpp`, and `Background.qml`. |
| Background blur | Done | Image/wallpaper blur is applied in `src/ui/qml/Background.qml`. |
| Frame padding | Done | Persisted in `src/core/Project.cpp`; layout is calculated by `src/ui/qml/Composition.qml`. |
| Frame corner radius | Done | Controls are in `src/ui/qml/ShapePanel.qml`; masked rendering is in `RoundedFrame.qml`. |
| Frame shadow | Done | Direction, distance, blur, and intensity are rendered by `src/ui/qml/RoundedFrame.qml`. |
| Frame inset | Done | Width, colour, and alpha are persisted in `src/core/Project.cpp` and rendered by `RoundedFrame.qml`. |
| Device mockups | Missing | Add a licensed frame-asset catalog, content-safe-area metadata, a Shape panel picker, and an outer composition layer around `RoundedFrame.qml`. |
| Camera bubble | Done | Persistent floating self-view from webcam enable through recording, draggable placement, warm camera adoption, private overlay masking, capture, timestamp alignment, preview/export rendering, positions, shapes, crop, rotation, and mirroring span `src/record/CameraCapture.cpp`, `src/core/CameraTimeline.cpp`, and `src/ui/qml/CameraOverlay.qml`. |
| Camera layouts | Partial | `src/ui/qml/CameraPanel.qml` provides six positions, sizing, and shapes. Time-ranged layout tracks and full screen/camera layout presets are missing. |
| Shrink camera during zoom | Done | `camera.scaleDuringZoom` is applied by `src/ui/qml/CameraOverlay.qml`. |
| Keystroke overlay / show shortcuts | Done | `src/core/KeystrokeTrack.cpp` groups recorded key-down events and calculates hold/fade samples; `KeystrokeOverlay.qml` is shared by preview and export and configured in `KeystrokesPanel.qml`. |
| Captions and transcript | Missing | Add speech-to-text ingestion, word-timed transcript storage, an editable captions track/panel, and a shared caption overlay in `Composition.qml`. |
| Background music | Missing | Add imported-audio project entries, trim/loop/volume controls, waveform/timeline UI, and another timed input in the export audio graph. |
| Microphone noise reduction | Missing | Add an opt-in offline audio cleanup pass with cached output and route the cleaned microphone stem into `src/render/Exporter.cpp`. |
| Speed-up typing suggestions | Missing | Detect sustained typing ranges from key events, present dismissible timeline suggestions, and apply accepted ranges as clip speed edits. |
| Clip speed | Done | Supported speeds, source/output mapping, UI, and audio tempo adjustment are in `src/core/ClipTimeline.cpp`, `src/ui/qml/ClipPanel.qml`, and `src/render/Exporter.cpp`. |
| Split and trim | Done | Non-destructive split/trim operations are implemented by `src/ui/Editor.cpp` with timeline controls in `ClipTrack.qml` and `Timeline.qml`. |
| Aspect-ratio presets | Done | Auto, wide, square, classic, vertical, tall, and portrait ratios are exposed by `src/ui/qml/BottomBar.qml` and persisted by `src/core/Project.cpp`. |
| Export presets | Done | MP4/GIF size, frame-rate, and quality presets are exposed in `src/ui/qml/ExportDialog.qml` and encoded by `src/render/Exporter.cpp`; reusable visual presets are in `src/ui/Editor.cpp`. |
| GIF export | Done | Palette generation, looping, and optional optimization are implemented in `src/render/Exporter.cpp`. |
| Shareable links | Missing | Add a user-selected upload provider, authenticated upload task, progress/cancellation, visibility controls, and returned-link copy UI. |
| Zoom editing | Done | Automatic/manual targets, levels, range editing, and spring presets are implemented in `src/ui/qml/ZoomPanel.qml`, `ZoomTrack.qml`, and `src/ui/Editor.cpp`. |
| Multiple resizable canvas layers | Missing | Introduce ordered layer objects with transforms and time ranges, selection/resize handles over the preview, a layers panel, and matching export composition. |
| Trimming | Done | Clip edge handles and source-bound validation are implemented in `src/ui/qml/ClipTrack.qml` and `src/ui/Editor.cpp`. |
