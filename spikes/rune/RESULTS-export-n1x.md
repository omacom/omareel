# Exporting on n1x: where the time went, and what changed

Qt's export is 1.3 to 1.8 times as fast on n1x: 4.15 s instead of 5.42 for 10 s of 1080p, 4.79 s
instead of 8.72 for 10 s of 4K, and 48 s instead of 77 for a 3-minute 1440p recording, which now
exports at 3.7 times real time. Qt is now faster than the Rune port at every size. What's left is
x264 itself: on n1x every export encodes with `libx264 -preset medium` on the CPU (Arch Linux ARM's
ffmpeg has no NVENC), and that alone takes about 3.0 s per 600 frames.

Measured on n1x (NVIDIA N1X, 10 Cortex-X925 + 10 Cortex-A725 cores, NVIDIA 615.78.08, ffmpeg
n9.0.2) on 2026-11-09. "Before" is `t3/startup-perf` at 291db4c, "after" is the commit with this
file. Exports ran from the desktop session (the GPU path), `--quality social`, after
`scripts/quiet.py`. Raw outputs are in `results-export-n1x/`.

## Results

Wall time of `<app> export <bundle> -o out.mp4 --quality social`, median of 3 runs (range), one
run for the 3-minute recording. All outputs are 1920x1080 at 60 fps.

| | Qt before | Qt after | Rune before | Rune after |
|---|---|---|---|---|
| `b1080`: 10 s of 1080p, 600 frames | 5.42 s (5.34–5.42) | **4.15 s** (4.12–4.22) | 4.67 s | 4.41 s |
| `b2160`: 10 s of 4K, 600 frames | 8.72 s (8.64–8.73) | **4.79 s** (4.72–4.80) | 8.51 s | 7.07 s |
| `real3m`: 3 min of 1440p, 10,800 frames | 77.1 s | **48.1 s** | 64.9 s | 66.6 s |
| Frames a second, `b1080` / `b2160` / `real3m` | 111 / 69 / 140 | **145 / 125 / 224** | 128 / 71 / 166 | 136 / 85 / 162 |

The floor: the encoder command alone, fed the same 600 rendered 1080p frames from a file, takes
2.94 to 3.04 s over five runs (`results-export-n1x/encoder-floor.txt` has three). Qt's frame loop now takes 3.86 s of its
4.15 at 1080p, the rest being the app starting; the difference to the floor is x264 sharing the
CPU with the decoder and the app, and starting and flushing.

Every run, before and after, both apps, reports `decoder: software` and `encoder: libx264`:
CUDA's device creation fails on this GPU (-12), and the ffmpeg here is built without NVENC
(`Unknown encoder 'h264_nvenc'`). `nvidia-smi dmon` shows the GPU at most 16 % busy before and
15 % after, NVENC at 0 % and NVDEC at 1 to 2 % (not ours; every decoder reports software).

## Where each frame's time went (Qt, 4K, before)

Qt's frame loop runs on the GUI thread: wait for the decoded pictures, upload them, render the
composition with QQuickRenderControl, read it back, hand it to the encode thread. Per frame, from
its own `--timing` and perf (`--user-callchains`, frame pointers, every thread and both ffmpeg
children, `scripts/export-profile.py`):

| Stage | ms a frame | What it was |
|---|---|---|
| Waiting for the decoder | 2.6 | 33 MB RGBA frames through a 64 KiB pipe, and each clip's decoder starting cold |
| Render | 8.9 | Converting each 33 MB frame to premultiplied RGBA (8 pool threads, 5.1 s), a new 33 MB texture each frame, re-evaluating every QML binding, a hidden gradient Canvas repainting |
| Readback | 0.6 (+2 waiting) | Two PBOs in flight; mapping waited for the GPU |
| Encode thread, writing | 6 | 8 MB RGBA frames through a 64 KiB pipe, copied once on the way |

In detail, ranked by what they cost:

1. **The decoder's transport.** ffmpeg converts each 4K frame to 33 MB of RGBA and writes it to a
   pipe; the app read it 64 KiB at a time (a `ppoll`, an `ioctl` and a `read` each, 500 a frame),
   into a buffer it zero-filled first, then copied it into a frame it had also zero-filled. 4K RGBA
   through a pipe tops out at 160 frames a second on n1x even with nothing else running; the same
   frames as yuv420p go at 440 (`decoder-pipe.txt`).
2. **Each clip's decoder starting cold.** The export starts a new ffmpeg at each clip, and every
   one tried CUDA first: 50 ms for the device to fail, then a second ffmpeg in software, which
   takes 0.28 s to its first 4K frame (decoding from a keyframe up to 4 s back). Three frames of
   decode-ahead hid none of it.
3. **The texture upload.** The frames were tagged straight (not premultiplied) RGBA, so the scene
   graph converted all 33 MB to premultiplied on Qt's thread pool before every upload, and
   `createTextureFromImage` made a new texture for each frame. `QQuickRenderControl::endFrame`
   (the driver's copy) was 2.5 s of the main thread.
4. **QML doing the same work every frame.** `CompositionState` had one `changed` signal for
   everything, so the frame's notify re-read the whole project map into JavaScript for every
   binding that touches it (about 1 ms a frame), and `Background.qml`'s gradient `Canvas`
   repainted on every settings change even when hidden: a whole-frame QPainter fill and image copy
   each frame (another 1 ms).
5. **Two readbacks in flight**: mapping the older one still waited for the GPU, 1.2 s in all.

**Why Rune was faster:** the same `omareel_core` export pipeline (the same decoder and encoder
processes and threads), so the same transport and cold starts, but its renderer uploads RGBA
without a format conversion, keeps three readbacks in flight, and has no QML. At 4K both were held
to the shared decoder's pace (8.5 against 8.7 s); at 1080p Rune's lighter render won (4.7 against
5.4 s).

## What changed

In `omareel_core`, for both apps:

- **Pipes.** `compat::qprocess` can ask for 1 MiB pipes (`set_pipe_capacity`) and move frames
  directly: `read_stdout_into` reads straight into uninitialised memory, `write_stdin_direct`
  writes straight from the frame. The decoder reads each frame into a buffer that is never
  zero-filled; the encoder pipe writes without copying into a write buffer.
- **Cold starts.** The next clip's decoder starts a second ahead, with the same arguments, so its
  ffmpeg has seeked and decoded by the time it's needed. An acceleration that gave no frame for a
  file isn't tried again for it in this process (the N1X's CUDA).
- **Queues.** Six decoded frames ahead instead of three, four encoder frames instead of two.
- **YUV frames.** The probe reads the video's matrix and range. A renderer that converts on the
  GPU can ask for the recording as yuv420p (`ExportJob::set_yuv_screen`): no RGB conversion in
  ffmpeg, and 12 MB instead of 33 for a 4K frame through the pipe and to the GPU. Only for BT.601
  (or untagged) and BT.709 videos; the webcam and inserted clips stay RGBA.

In Qt:

- **`FrameSource` converts YUV on the GPU**: three single-channel textures that stay from frame to
  frame (uploaded straight from the decoder's buffer), a small material and shader
  (`src/render/shaders/yuv.*`, through `qt_add_shaders`; `qt6-shadertools` was already a build
  dependency), and the crop as texture coordinates instead of an image copy. The export uses it
  whenever it renders on the GPU; `OMAREEL_EXPORT_RGBA=1` keeps the RGBA path.
- The recording's and the webcam's RGBA frames are tagged premultiplied (alpha is 255, so the bytes
  are the same) and upload without the conversion.
- Four readback buffers instead of two.
- `CompositionState`: the sizes and the project notify on their own `setupChanged`, never emitted
  in the frame loop.
- `Background.qml`'s gradient `Canvas` only repaints while it's shown (this helps the editor too).

How each step moved Qt (median of 2 runs, `bench/r*.jsonl`):

| | `b1080` | `b2160` |
|---|---|---|
| Before | 5.42 s | 8.72 s |
| Pipes, queues, premultiplied, 4 readbacks | 5.06 s | 8.14 s |
| Next clip's decoder ahead, failed CUDA remembered | 4.70 s | 7.30 s |
| One signal per frame, hidden Canvas | 4.30 s | 6.76 s |
| YUV on the GPU | **4.13 s** | **4.78 s** |

## Checks

- **Same output settings:** frame count, duration, size, profile, pixel format, colour tags and
  x264's own settings string (the SEI it writes) are identical before and after for all three
  recordings. The audio is bit-identical (MD5 of the decoded track).
- **Same pictures:** within what I'd call identical, but not bit-exact. x264 here isn't
  deterministic from run to run, so two exports from the same before build already differ. PSNR
  and SSIM of the video against the before export:

  | | after vs before | before vs before (another run) |
  |---|---|---|
  | `b1080` | 46.6 dB (min 42.5), SSIM 0.9973 | 49.9 dB (min 43.8), SSIM 0.9985 |
  | `b2160` | 47.7 dB (min 41.8), SSIM 0.9983 | |
  | `real3m` | 43.5 dB (min 40.5), SSIM 0.9961 | 45.7 dB (min 42.3), SSIM 0.9972 |

  For scale, Qt's and Rune's exports of the same project (both correct) are 36.3 dB, SSIM 0.9935.
  Up to the YUV step, the changes don't touch the pixels (the step before it is 49.9 dB against
  the before build, run-to-run noise). The YUV path differs from swscale's conversion on hard
  colour edges: on frame 3000 of `real3m`, 2.7 % of pixels are more than 4 levels off where another
  before run has 0.8 %, all on the test pattern's saturated bar edges and checkerboard
  (`diff-real3m-frame3000.png`). Bilinear chroma was the closest of three ways to sample it
  (nearest: 46.2 dB; left-sited: 43.6 dB). `OMAREEL_EXPORT_RGBA=1` restores the old conversion.
- **Other paths:** the software-rendering export (no display) works and is unchanged (it never uses
  YUV); GIF export is byte-identical; the editor's preview still shows its video, plays and seeks
  (`launch.py`, 2 runs). Cancelling isn't changed.
- **Tests:** `cargo test --workspace --exclude omareel_qt` 357 passed (one new, direct reads and
  writes through `cat`; the decoder's colour test now also checks its YUV frames in both matrices); `ctest` 4 of 5, with
  `cameracapture` failing the same four checks it fails on 4fd1696 (ffmpeg's swscale crash on
  aarch64, `RESULTS-startup-n1x.md`); `cargo test --release` in `spikes/rune` passes.

## What's left

- **x264 is the limit.** NVENC would change everything here: the N1X has the hardware, but this
  ffmpeg isn't built with it. Worth asking for in the Omarchy ARM package (`--enable-nvenc` needs
  only nv-codec-headers).
- **4K decoding** is 17 s of CPU per 600 frames in software. CUDA fails on this GPU; ffmpeg's
  Vulkan video decode might work, at the price of the 1.1 s Vulkan device start-up measured in the
  startup work. Untested.
- **Rune doesn't use the YUV frames yet.** Its export draws the recording as `rune::Pixels`, which
  are RGBA only; through a native surface (NV12, as the preview already does) it would get the
  same 4K gain. The core side is ready (`set_yuv_screen`).
- **Not measured on foundry2.** The YUV path is QRhi with R8 textures, which every backend has, but
  the VA-API encoder there and Intel's GL should be checked.

## Tools

- `scripts/export-profile.py <perf.data>`: CPU seconds per process (the app, each ffmpeg named by
  what it runs) and per thread, the top functions, and a thread's inclusive costs.
- Profiles: `perf record -k mono -F 2999 --call-graph fp --user-callchains` over the export and
  its children with a frame-pointer build (`build/prof`); waits with `--switch-events`;
  `nvidia-smi dmon -s um` alongside.
