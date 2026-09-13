# MP4 export backpressure

The renderer must not accumulate the remainder of a video in RAM while FFmpeg
catches up. `QProcess::write()` accepts data into Qt's userspace buffer, and
`waitForBytesWritten()` signals **partial** progress, not that a frame has been
consumed. Waiting just once per frame bypassed the exporter's bounded queue.

The MP4 pipeline now has:

- Two queued rendered frames and two asynchronous GPU readback slots.
- One encoder worker that drains `bytesToWrite()` to zero before taking another
  frame. Qt can buffer at most one submitted frame, not the remaining video.
- Short polling waits for cancellation, with a 30-second **no-progress** write
  deadline. A frame can take longer than that to transfer if bytes keep moving.
- GUI event processing while waiting for queue space or encoder shutdown, with
  the export GL context restored after processing events.
- An explicit finishing status until encoding and MP4 finalization succeed.
  The progress bar does not reach 100% just because rendering is complete.
- Explicit errors for write stalls, failed/crashed encoders, and finalization
  timeouts, including when FFmpeg writes nothing to stderr. Unfinished encoders
  are killed and reaped before temporary files are removed. Cancellation and
  encoder errors leave an existing destination untouched.

This bounds the application's frame backlog, not FFmpeg's codec reference
frames, lookahead, filter buffers, or thread allocations. At 4K those can still
use several GiB, but should not grow with video length. The GIF raw-file path
is unchanged by this fix.

## Tests (omadev only)

Use an owned nest and a canonical source path with a separate build directory.
A build cache made through another nest's HOME symlink cannot be reused as-is.

```sh
# Run from the repository root.
repo="$(pwd -P)"
omadev ensure 1 --owner omareel-backpressure
omadev run 1 cmake -S "$repo" -B /tmp/omareel-backpressure-build \
  -G Ninja -DCMAKE_BUILD_TYPE=Release -DOMAREEL_BUILD_HYPRLAND_PLUGIN=OFF
omadev run 1 cmake --build /tmp/omareel-backpressure-build -j 4
# Let GUI tests keep their requested window dimensions, instead of being tiled.
omadev hyprctl 1 eval 'hl.window_rule({name="omareel-backpressure-test", match={class="test_editor"}, float=true})'
omadev run 1 env OMAREEL_DISABLE_NVENC=1 ctest \
  --test-dir /tmp/omareel-backpressure-build --output-on-failure --timeout 90
omadev run 1 env QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  OMAREEL_DISABLE_NVENC=1 ctest --test-dir /tmp/omareel-backpressure-build \
  -R 'encoderpipe|exporter_smoke' --output-on-failure --timeout 90
omadev run 1 python3 "$repo/tests/tools/export-memory-proof.py" \
  /tmp/omareel-backpressure-build/omareel
omadev diff 1
omadev stop 1
omadev clean 1
```

`encoderpipe` tests slow reads, byte integrity, a progressing frame exceeding
its stall timeout, blocked-write cancellation, silent failure, process death,
and finishing waits/timeouts/cancellation. `exporter_smoke` exercises a real
rate-limited encoder, cancellation with a full queue, cancellation during
finalization, silent failure, destination preservation, and temporary cleanup.
`editor` checks the actual QML finishing label and progress bar during export.

The memory proof generates its own 4K source, exports 5 and 20 seconds through
FFmpeg at half real-time input speed, samples exporter and process-tree RAM and
swap, and decodes both outputs with ffprobe to verify every frame. Its watchdog
stops its own process group above 1.5 GiB exporter memory, 6 GiB process-tree
memory, or a safety timeout. Logs, samples, and outputs remain under the printed
`/tmp/omareel-memory-proof-*` directory. It refuses to run outside omadev.

## Verified on this machine

- All 13 CTest targets passed under nested Wayland/OpenGL.
- Pipe and export smoke tests also passed with Qt's software renderer.
- 5-second and 20-second 4K exports both settled at approximately 559 MiB exporter
  RSS, with zero exporter swap. Peak total process-tree memory was 5.0 GiB.
- A private copy of the previously stuck recording exported at its saved 4K,
  30 fps settings in approximately 35 seconds: 1,344 frames, 44.8 seconds of
  output (the project's clip runs at 2x speed). Exporter peak RSS was 593 MiB,
  zero swap, and total process-tree peak was approximately 5.2 GiB.

No installed binary, host desktop configuration, or original recording was
changed during these tests.
