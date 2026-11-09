# Opening the editor on n1x: where the start-up time goes

On n1x, Qt Multimedia's hardware encoder probe is still almost all of the Qt editor's start: 1.8 s of
its 2.4 s, spent in the kernel creating a VA-API device and a Vulkan device it never uses. Setting
`QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` takes the editor's first frame from 2.40 s to 0.57 s, and
the launcher and the recording bar from 2.1 s and 2.3 s to 0.48 s and 0.63 s. Past that, both apps
are bounded by NVIDIA's driver setup, which is kernel time perf can't see without root: about 205 ms
for Rune's Vulkan device and about 200 ms of EGL for Qt.

Follow-up at f831c8a: with 0662299 Qt's editor shows its first frame at 0.58 s and Rune's at
0.48 s. b81b10a's in-process decoder doesn't crash here: the libswscale corruption is in the
multithreaded frame API with a colour-matrix change, which `media.c` doesn't use. Its NV12 path is
clean at every even size, but an odd width would overrun the UV plane by a byte; omareel only
passes even sizes today.

Measured on n1x (NVIDIA N1X: 10 Cortex-X925 + 10 Cortex-A725 cores, NVIDIA 615.78.08, Hyprland
0.57.0 nested, windowed on DP-7) on 2026-11-08, with `t3/startup-perf` at 51743e9 as "after" and
4fd1696 as "before". The recording is `make-real-bundle.sh`'s 3 minutes of 1440p60 with audio,
camera and 21,769 input events. Raw outputs are in `results-n1x/`.

## Results

Medians of runs 2 to 5 of 5 (reopen: runs 2 to 4 of 4), seconds from launch.

| | Qt before | Qt after | Qt after, encoder probe off | Rune before | Rune after |
|---|---|---|---|---|---|
| First frame on screen | 2.59 | 2.40 | **0.57** | 0.94 | **0.50** |
| Camera strip complete | 2.52 | 2.18 | 0.24 | 1.46 | 0.59 |
| Filmstrip complete (78 / 47 frames) | 14.28 | 4.08 | 2.08 | 9.30 | **1.47** |
| Reopening (cached): first frame | 2.56 | 2.23 | 0.51 | 0.95 | 0.50 |

"Encoder probe off" is Qt after with `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` in the environment
(`timings/clean-qt-after-enc.json`). Adding `QSG_RENDER_LOOP=basic` on top takes it to **0.45 s**
(`timings/clean-qt-after-enc-basic.json`, see bottleneck 2).

Before the change Qt's filmstrip took 14.3 s here, so every fresh run used `SECONDS_RUN=20`
instead of 14. Qt's first frame varies more than Rune's: 2.24 to 2.71 s across the five runs.

On foundry2 the same numbers were 0.29 s (Qt) and 0.11 s (Rune) to the first frame, and 1.6 s and
1.0 s for the filmstrip. n1x is 2 to 5 times slower to the first frame, almost all of it in driver
setup, not in our code.

### The recording loop

`loop-timing.py`, 6 runs each, medians in ms. The capture plugin loads on n1x (rebuilt for
Hyprland 0aa6745), so the bar stays on the recorded output.

| | Qt before | Qt after | Qt after, encoder probe off | Rune before | Rune after |
|---|---|---|---|---|---|
| Launcher window | 2188 | 2107 | **477** | 451 | 443 |
| Recording bar, from the click | 2293 | 2274 | **631** | 742 | 737 |
| Editor, from Stop | 2910 | 2551 | **922** | 1033 | 853 |

The encoder probe costs every Qt process the same 1.6 to 1.8 s, not just the editor.

## Follow-up: 0662299 (Qt encoder probe) and b81b10a (Rune in-process decoding)

Rebuilt Qt after and Rune after at f831c8a and measured again, same machine and setup.

### libswscale on aarch64: the decoder's conversions are safe at the sizes omareel uses

`results-n1x/standalone/sws-test.c` replays `media.c`'s conversions with every destination plane
ending at a page boundary followed by a no-access guard page (a write past it faults at once) and
canary bytes before it; results in `sws-test.txt`.

- **The crash I saw is the frame API, multithreaded, with a matrix change.** `sws_scale_frame`
  (what ffmpeg's `scale` filter uses) converting NV12 to yuv420p with BT.601 in and BT.709 out
  aborts with heap corruption when swscale runs several threads (`threads` 0, which is auto, or
  20), at 64x48 and at 1280x720. With 1 or 4 threads, or without the matrix change, it doesn't.
  That is the camera encoder's filter, and why `cameracapture` fails.
- **`media.c` uses neither.** It calls the legacy `sws_getContext` / `sws_scale`, single
  threaded, and the stream path makes no matrix change (the thumbnail path changes only the range).
- **Stream path (yuv420p to NV12, bilinear):** clean at 1920x1080 (300 frames), 2560x1440 (200),
  3840x2160 (120), and scaled 1080p to 720p and 4K to 1080p, with `MALLOC_CHECK_=3`. **But every
  odd destination width faults**: 28 of 28 cases (1517, 1001, 999, 1919, 641, 333 and 17 wide,
  from 720p, 1080p, 1440p and 4K sources); even widths never do, odd heights are fine. That is the
  caller's buffer, not swscale: an NV12 row of chroma for an odd width `w` is `w + 1` bytes
  (`2 × ceil(w / 2)`), but `omr_stream_next` passes a UV stride of `w` and
  `media_av::VideoStream::next` sizes the UV buffer `w × h / 2`, so the last row writes a byte
  past the end (and each row overlaps the next).
- **Thumbnail path (yuv420p to yuv420p, bicubic, cover and crop, full range out):** clean in all
  60 sweep cases, odd widths included, and at the real sizes (2560x1440 and 3840x2160 to 284x160,
  300 and 100 frames).
- **In the app:** 3 runs each of 1080p and 4K playback with `MALLOC_CHECK_=3`, plus 30 launches with
  plays and seeks (below), no crash and no omareel core dump.

Every size omareel passes today is even: recordings are trimmed to even sides
(`even_capture_region`, `crates/omareel_record/src/recorder/mod.rs:54`), the camera is libx264
yuv420p, and inserted media is rounded down to even (`preview.rs:654`). So **NV12 output is
safe as used; no need to switch to yuv420p.** The odd-width case was a latent bug in the API;
2d4f5a6, pushed while this ran, fixes it (the stream rounds down to even and `omr_stream_open`
refuses an odd size). The measurements here are from f831c8a, before it.

### Qt after with 0662299

| | Qt after (51743e9) | Qt after, env var | **Qt after (0662299)** | Rune after (b81b10a) |
|---|---|---|---|---|
| First frame on screen | 2.40 | 0.568 | **0.581** | **0.475** |
| Camera strip complete | 2.18 | 0.237 | 0.236 | 0.524 |
| Filmstrip complete | 4.08 | 2.08 | 2.10 | 1.43 |
| Reopening: first frame | 2.23 | 0.51 | 0.555 | |

0662299 lands where the environment variable did. Rune after's first frame moved 0.503 → 0.475 s
with in-process decoding (`timings/clean-rune-after2.json`).

### Opening, playing and seeking (`launch.py`, 6 runs, warm median of runs 2 to 6)

The stock `launch` bundle is `~/spike/b1080` (10 s of 1080p60, keyframes at 0, 4.17 and 8.33 s,
B-frames). `launch-g60` is the same with `screen.mp4` re-encoded `-g 60 -keyint_min 60
-sc_threshold 0 -bf 0` (keyframes every 1.00 s). Each app got a fresh copy of each.

| | Rune after (b81b10a) | Rune before (4fd1696) | Qt after (0662299) |
|---|---|---|---|
| **Stock bundle** | | | |
| Window | **0.505 s** | 0.757 s | 0.557 s |
| Video showing | **0.548 s** | 0.881 s | 1.249 s |
| Play to motion, median (max) | 213 ms (356) | 236 ms (374) | **169 ms** (220) |
| Seek to its frame, median (max) | 86 ms (131) | 170 ms (200) | **43 ms** (67) |
| Idle CPU / RSS (with decoders) | 3.0 % / 681 MB | 3.1 % / 506 (933) MB | 0 % / 751 MB |
| **1 s keyframes** | | | |
| Window | **0.515 s** | 0.784 s | 0.544 s |
| Video showing | **0.549 s** | 0.883 s | 1.243 s |
| Play to motion, median (max) | 229 ms (896) | 218 ms (376) | **136 ms** (226) |
| Seek to its frame, median (max) | 85 ms (127) | 163 ms (188) | **46 ms** (74) |

- Rune since 4fd1696 (the startup work and in-process decoding together) shows its video 0.33 s
  sooner (0.88 → 0.55 s) and seeks twice as fast (170 → 86 ms; the seek is b81b10a's, since
  51743e9 didn't change seeking on foundry2). Qt still seeks twice as fast as Rune.
- Keyframe spacing doesn't change Rune's seek any more: the paused seek shows the keyframe first.
  `OMAREEL_DECODE_LOG=1` shows the keyframe 11 to 16 ms after the click and the exact frame 40 to
  54 ms later when the keyframe is 2 s back, 88 to 90 ms when it's 3.5 s back
  (`followup/rune-decode-log-launch-run.txt`).
- Play doesn't reopen a decoder (no decode lines at Play; it reuses the paused stream), so
  Rune's extra ~60 ms over Qt isn't decoder start. It is one frame interval at the 15 Hz the
  window gets (below); I didn't pin down where it goes. The 896 ms max is one outlier in 18 plays.
- Qt shows its window at 0.56 s but its video only at 1.25 s, warm runs included (where the
  filmstrip is cached, so it isn't contention with the filmstrip workers). That 0.7 s is the next
  thing to look at in Qt.
- All play and seek times are quantised to the 66 ms the nested window gets between frames here
  (see "Regression checks"), for both apps; the comparisons hold, the absolute milliseconds don't
  match foundry2's.

### Rune playback with the in-process decoder (`measure.sh preview`, 3 runs)

Against 51743e9 (ffmpeg processes), same 15 frames/s cap on both:

| | 51743e9 | b81b10a |
|---|---|---|
| 1080p: CPU, app + decoder processes | 91.3 + 43.6 = **135 %** | **117 %** |
| 4K: CPU, app + decoder processes | 104.1 + 131.5 = **236 %** | **182 %** |
| 1080p / 4K RSS | 628 / 691 MB, decoders not counted | 788 / 1185 MB, decoders included |
| UI-thread frame | 1.72 / 2.20 ms | 1.67 / 2.09 ms |

Frame pacing and drops match (both bound by the cap). I can't compare with `RESULTS-n1x.md`'s
software-decode numbers: those ran at 60 frames/s before the monitor change.

### The new Rune after under perf (frame pointers, `--user-callchains`)

Nothing new before the first frame (0.535 s in the profiled run): the same order and the same
`for_surface` span, 0.10 to 0.44 s (`analyze/rune-after-b81b10a-fp-1-until0.8.txt`). Two things
after it, neither from b81b10a (the 51743e9 profile has both):

- **The event loop spins while the editor is busy.** From the first frame to 0.85 s the main
  thread is on the CPU 238 ms of 300, 100 ms of it in `clock_gettime` (`Timespec::now`) and most
  of the rest in calloop's `epoll` wait returning almost at once, so the loop is polling rather
  than sleeping until the next frame. Worth checking what `busy()` asks the loop for (a wait until
  the next frame rather than a poll), since it competes with the decoders for a big core.
- **Wallpaper picker thumbnails** (`inspector::generate_thumbnail`, the `image` crate's WebP
  decoder and resize) take two threads at 100 % while they're being made. They are cached on disk,
  so this is the first launch after a theme change, not every launch; my profile runs cleared the
  cache.

## Qt's gap between the window and the video: the render loop, not CUDA

Qt after at 4f44393 on the `launch` bundle shows its window at 0.6 s but its video at 1.2 s. The
cause isn't hardware decoding. The GUI thread queues each step of starting playback behind a
render-thread swap, and every swap here waits 66 ms for the 15 Hz nested window. Raw logs and
timings are in `results-n1x/gap/`.

**Qt never tries CUDA for decoding.** With `qt.multimedia.ffmpeg*` and `qt.multimedia.playback*`
logged (`qt-log-nested-default.txt`; on n1x Qt sends its log to journald unless
`QT_FORCE_STDERR_LOGGING=1` is set, which also means `startup-profile.py`'s scene graph timings
were never captured here), the decoder is chosen in 2 ms:

| s | |
|---|---|
| 0.005 | `setSource` (`mediadataholder`) |
| 0.466 | window's first frame (render thread: `frame rendered in 89ms ... swap=76`) |
| 0.594 | `Create PlaybackEngine`, screen and camera |
| 0.595 | `No hw acceleration found for codec id h264`, `"No HW decoder found"`, software `h264` |
| 0.860 | `Delay of demuxer initialization: 266130us` (and 263801 us) |
| 0.988 | `Seeking to offset 0us from media start.` |

Offscreen (`QT_QPA_PLATFORM=offscreen`, no compositor), the same build creates the engines at
0.135 s, initialises the demuxers in 13 to 14 ms and seeks 20 ms after creation
(`qt-log-offscreen-default.txt`). In the nested run the demuxer delays are 266 and 67 ms, four
and one 15 Hz frames. Meanwhile the render loop logs `blockedForSync=172 ms`, `124 ms`, then
49 to 67 ms every frame, because the scene is redrawn every frame while `editor.loading` runs the
"Opening recording…" spinner (`Main.qml`, a `RotationAnimator` with infinite loops). That spinner
only stops when the screen player reports `LoadedMedia` (`handle_media_status` →
`FINISH_LOADING_CALL`, or after 1.5 s), so it keeps the loop busy while playback starts.

The off-CPU profile of the GUI thread (`offcpu-qt-gui-thread-until1.3.txt`) shows the same: from
0.48 to 1.35 s it blocks a dozen times for 45 to 151 ms (mostly 60 to 68), in two exposes, two
more from configure events, frame update requests, and omareel's own queued calls
(`PlaybackDriver::frameShown`, the `Editor` queue).

### The variants (`launch.py`, 6 runs, warm median, fresh copy of the `launch` bundle each)

| | window | video | gap | play | seek |
|---|---|---|---|---|---|
| (a) default | 0.605 s | 1.219 s | 0.614 s | 175 ms | 40 ms |
| (b) `QT_FFMPEG_DECODING_HW_DEVICE_TYPES=none` | 0.555 s | 1.197 s | 0.642 s | 165 ms | 46 ms |
| (c) `QT_FFMPEG_DECODING_HW_DEVICE_TYPES=vulkan` | **2.600 s** | 3.254 s | 0.654 s | 165 ms | 43 ms |
| (d) `QSG_RENDER_LOOP=basic` | **0.456 s** | **0.690 s** | **0.234 s** | **117 ms** | 43 ms |

- (b): Qt warns `Unknown hw device type "none"` and lists no decoding frameworks, so it is an
  empty list, not the default one. It behaves like the default because the default already falls
  back to software (CUDA's device creation fails on this GPU).
- (c): decoding on Vulkan works (`Selected format vulkan`), but checking the Vulkan device takes
  0.92 s before the first engine (`qt-log-offscreen-decoding-vulkan.txt`) and the window comes 2 s
  later. Don't.
- (d): the GUI thread renders itself, so queued playback steps no longer wait for a render-thread
  swap. The gap drops from 0.61 to 0.23 s and the video shows 0.53 s sooner. The cost: click to
  the next frame is 19.6 ms instead of 4.3, since the GUI thread now waits on its own swap.
- Playback works in all four: every play click showed motion, and seeks landed, with no timeouts.
- With the spinner delayed 400 ms (7b04823), same runs: default 0.555 s window, 0.943 s video,
  0.388 s gap (from 0.614); basic 0.445 s, 0.652 s, 0.207 s (from 0.234).

**Fix ideas**, in order:

1. Don't animate while loading. A static "Opening recording…" (or one that starts animating after
   a few hundred ms) leaves the GUI thread free between frames, so the playback steps run at
   once. This also helps on fast displays, where the same chain costs a few frames.
2. Use the basic render loop on NVIDIA (`QSG_RENDER_LOOP=basic` beside the other NVIDIA settings
   in `configureVideoDecoding()`): 0.61 → 0.23 s here, and the first frame 0.57 → 0.45 s
   (bottleneck 2). Check that playback stays smooth at 60 Hz first; I couldn't measure smoothness
   on this capped window.
3. Leave `QT_FFMPEG_DECODING_HW_DEVICE_TYPES` as it is.

How much of this you'd see on a real 60 Hz monitor I couldn't measure (see the 15 Hz note), but
the chain is a count of frames: at 16.7 ms a frame, the same dozen blocks come to about 0.15 s,
which matches foundry2's 0.13 s window-to-video gap.

## Third round: the warm editor, the waiting bar, faster Stop, FFmpeg at runtime

This round's commits (c202119 to 46b513c, plus the export work at 7de76e5) against 291db4c, on
n1x. Nothing broke. The editor opens 0.44 s after Stop instead of 0.93 (0.51 instead of 1.13 with
audio), the bar shows in 0.33 s instead of 0.64, and with the warm editor the first frame is on
screen 0.19 s after the signal. Raw data in `results-n1x/round3/`. Same setup as before: nested
Hyprland windowed on DP-7, which only gets 15 frames a second (see "Regression checks"), so
anything measured in frames (play, seek) is in 66 ms steps.

### Correctness

- **Tests:** `cargo test --workspace --exclude omareel_qt` 357 passed; `ctest` 4 of 5, the same
  four `cameracapture` checks as before (ffmpeg's swscale crash on aarch64); `cargo test --release`
  in `spikes/rune` passes.
- **No FFmpeg linked:** `ldd omareel` lists 102 libraries, none of them `libav*` or `libsw*`
  (291db4c: 190, six of them FFmpeg's). Loading the five on first use costs 19 to 22 ms here
  (`dlopen` with `RTLD_NOW`).
- **What uses it works:** an export (1080p and 4K, the same pictures as the export results), the
  editor's probes and filmstrip (every startup run below), the Rune preview (plays in software
  through `media_av`, `rune-preview-1080p.md`), and real recordings (`ext-image-copy-capture`, 0
  drops). On n1x the recorder never takes its GPU path: every run logs `gpu=0` (NVIDIA's VA-API is
  excluded and this ffmpeg has no NVENC), so `gpu_capture.c`'s GPU encode isn't exercised here.
- **Recording parity** (`tools/parity/recording/recording-parity.py`, 291db4c against this round,
  `parity/diffs.txt`): every difference is one you listed or follows from one:
  - the recorder's `ffprobe` for the duration is gone, and so is the thumbnail's `ffmpeg` in the
    gpu-screen-recorder cases (made in process; the bundles have the same files);
  - `-stats_period 0.05` on the audio ffmpeg;
  - `edit --wait <pid>` in `editor_argv`, and `project.json` in `open-editor`'s bundle;
  - the NVENC encoder probe now runs at a different point among the calls;
  - `countdown_ready` is true at start in `gsr-fake-fullscreen-bar` (the bar was already waiting);
  - one missing input position in `fullscreen-bar` (the pointer's start, 800,450), which didn't
    come back in three reruns of that case: timing.

  Two things were needed to run it on n1x. The nested Hyprland segfaults drawing hardware cursors
  on NVIDIA (`CPointerManager::renderHWCursorBuffer`) as soon as the recorder turns them back on,
  which took every later case down with it on both sides; a wrapper that keeps
  `cursor:no_hardware_cursors` on fixes that. And the harness put its window on the first outer
  monitor; it now takes `OUTER_MONITOR`, so a run stays off the screen in use. As a side effect,
  the time to the file's first frame went from 250 to 330 ms down to 160 to 200 ms in most cases.

### The warm editor on NVIDIA

`warm/`, a 20 s fullscreen recording each way:

| | warm editor | `OMAREEL_EDITOR_WAIT=0` |
|---|---|---|
| Waiting editor's RSS | 234 MB | |
| Waiting editor's CPU while recording | 0.0 % | |
| Recorder's CPU while recording | 5.9 % | 6.0 % |
| Frames, drops (`capture.drops`) | 304, 0 | 304, 0 |
| Editor's window after Stop | 433 ms | 720 ms |

No crash and no core dump. With decoding and encoding both set to CUDA on NVIDIA, the throwaway
QMediaPlayer never touches NVIDIA's VA-API driver.

### Startup (`startup-profile.py`, median of runs 2 to 5 of 5)

| | 291db4c | this round | this round, `WAIT=4` (from the signal) |
|---|---|---|---|
| Editor: first frame | 0.515 s | 0.475 s | **0.188 s** |
| Filmstrip: first frame lands | 0.33 s | 0.70 s | 0.47 s |
| Filmstrip: filled (each tile its own frame or its keyframe's) | 1.70 s | **0.80 s** | **0.56 s** |
| Filmstrip: complete (every tile its own frame) | 1.70 s | 1.92 s | 1.68 s |
| Camera strip complete | 0.25 s | 0.76 s | 0.52 s |
| Launcher (`NO_BUNDLE=1`): first frame | 0.481 s | 0.463 s | |

The stand-ins fill the strip twice as early. Waiting for the first video frame (or 400 ms) moves
the filmstrip's and the camera strip's start back by about 0.4 s and finishes the exact frames a
little later; 126 frames are built instead of 78 (48 stand-ins, keyframes every second).

### Opening, playing, seeking (`launch.py`, 6 runs, warm median)

| | 291db4c | this round | this round, `WAIT=4` |
|---|---|---|---|
| Window | 0.545 s | 0.509 s | **0.179 s** |
| Video showing | 0.985 s | 0.899 s | **0.577 s** |
| Play to motion, median | 157 ms | 156 ms | 164 ms |
| Seek, median (rerun) | 16 ms (46 ms) | 45 ms (26 ms) | 52 ms |
| Idle RSS | 737 MB | 731 MB | 727 MB |

The seeks look like a change between the first two columns and aren't: rerun back to back, the
order flips (46 against 26 ms). On this window they are mostly a matter of which 66 ms frame they
land on.

### The recording loop (`loop-timing.py`, 6 runs, median ms)

| | 291db4c | this round | this round, both waits off | 291db4c, audio | this round, audio |
|---|---|---|---|---|---|
| Launcher window | 488 | **417** | 429 | 521 | **432** |
| Region picker | 244 | 177 | 196 | 269 | 158 |
| Bar, from the click | 644 | **327** | 595 | 631 | **332** |
| `record --stop` returns | 466 | **266** | 265 | 617 | **369** |
| Editor window, from Stop | 933 | **440** | 705 | 1126 | **513** |

The waiting bar is worth about 270 ms and the warm editor about 265 ms; the rest of the Stop path
(the in-process probes, the thumbnail moved after the editor, the 15 ms `record --stop`) another
230. Audio used to add about 190 ms to the editor's time and now adds about 70, most likely the
audio ffmpeg's `-stats_period` (I didn't isolate it).

Where Stop goes now, with audio (`loop/after-audio-stop-stages.txt`, median ms after
`stop_seen`, 6 runs; 291db4c has no such stages):

| stage | ms |
|---|---|
| `stop_capture_finished` | 24 |
| `stop_audio_muxed` | 140 |
| `stop_camera_stopped` | 140 |
| `stop_probed` | 263 |
| `stop_input_written` | 268 |
| `stop_editor_launch` | 274 |
| `stop_thumbnail` | 346 |

The biggest step left is `stop_camera_stopped` to `stop_probed`, 123 ms. Loading FFmpeg's libraries
the first time is about 20 ms of it; the rest is the in-process probing itself or something it
waits on, which I didn't profile. Worth a look.

### Qt's 4K playback CPU

One perf run of the Qt editor playing `b2160` (`OMAREEL_PREVIEW_STATS=1`, 8 s, frame pointers,
`qt-4k-playback-profile.txt`): 12.9 s of CPU in 8 s (161 %), of which 11.4 s is `libavcodec`,
sixteen `av:h264` frame threads at about 0.7 s each. The GUI thread is 0.46 s, the render thread
0.40 s, `memcpy` 0.10 s. So it isn't a CUDA download or the upload: it's software H.264 decoding of
every 4K frame at 60 frames a second, since CUDA decoding never starts on the N1X (`No HW decoder
found`). The 104 % I gave for Rune earlier was its app process alone, with its decoding in separate
ffmpeg processes (another 131 %); with in-process decoding (b81b10a) Rune's total is 182 %, so Qt
isn't behind. Only a working hardware decoder would bring this down here.

One more thing seen in passing: the Rune preview's RSS at 1080p was 1,243 MB in this run, against
788 MB at b81b10a. One run, not looked into.

## Top bottlenecks, ranked by milliseconds

### 1. Qt Multimedia probing hardware encoders: 1,830 ms (Qt only)

`QMediaPlayer::QMediaPlayer` → `QPlatformMediaIntegration::instance()`, called from
`PlaybackDriver::PlaybackDriver` in the `Editor` constructor, holds the main thread from 0.15 s to
2.15 s. The CPU profile only sees 190 ms of it (`vaInitialize` → `__vaDriverInit_1_0` 89 ms,
`vkCreateDevice` 59 ms, `cuStreamCreate`, `cuMemsetD8Async`) and the off-CPU profile finds almost
no sleeping in that span, so the rest is kernel time: `perf_event_paranoid` is 2 here, which
limits perf to user space, and kernel time shows up as "off the CPU" in `analyze-startup.py`.

Timing FFmpeg's own device creation on its own (`results-n1x/standalone/ffmpeg-hwdevice.txt`):

| `ffmpeg -init_hw_device` | wall | user | sys |
|---|---|---|---|
| none | 0.04 s | 0.02 | 0.02 |
| vaapi (`nvidia_drv_video.so`) | 0.62 s | 0.05 | 0.57 |
| vulkan | 1.09 s | 0.05 | 1.04 |
| cuda | fails with -12 (ENOMEM) in 0.03 s | | |

VA-API plus Vulkan is 1.7 s, which is the probe. `configureVideoDecoding()` already sets
`QT_FFMPEG_DECODING_HW_DEVICE_TYPES=cuda` on NVIDIA, but not the encoding one, so the encoder probe
still opens every device type. `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` removes it: 2.40 → 0.57 s
for the editor, and the launcher and bar as above.

**Fix:** set `QT_FFMPEG_ENCODING_HW_DEVICE_TYPES=cuda` beside the decoding variable in
`src/render/VideoDecoding.cpp`, under the same `vaapiDriverIsNvidia()` check. This is the same fix
`RESULTS-n1x.md` proposed (item 6); it isn't in 51743e9. **Landed in 0662299**: 0.581 s to the
first frame, see the follow-up below.

### 2. EGL setup on two threads: about 200 ms (Qt), 116 ms of it avoidable

With the probe off, the main thread spends 0.375 to 0.47 s in
`QSGRhiSupport::maybeCreateOffscreenSurface`, then waits 140 to 150 ms in
`QWaylandWindow::sendExposeEvent` for the render thread's first frame. The render thread is on
the CPU for only 32 ms of that (`QRhi::create` → `QOpenGLContext::create` 11 ms, NVIDIA's EGL
libraries 21 ms; `analyze/qt-after-encoff-dwarf-1-renderthread.txt`). The rest is kernel.

EGL on its own (`standalone/egl-setup.txt`, a device-platform display, so no window system):

| | wall | sys |
|---|---|---|
| `eglQueryDevicesEXT` | 23 ms | 12 |
| `eglInitialize` | 6 to 11 ms | 6 to 10 |
| `eglCreateContext` | 12 to 19 ms | 10 to 17 |
| first `eglMakeCurrent` | 66 to 74 ms | 64 to 72 |
| second context, same thread | 0.4 ms | 0 |
| first `eglMakeCurrent` on a new thread | 70 to 76 ms | 68 to 72 |

The 70 ms `eglMakeCurrent` is paid per thread, not per process. Qt's threaded render loop touches
EGL on the GUI thread (the offscreen surface) and again on the render thread, so it pays twice.
`QSG_RENDER_LOOP=basic` keeps it on one thread: **0.568 → 0.452 s**.

**Fix ideas:** use the basic render loop on NVIDIA (check playback first: it renders on the GUI
thread, and I haven't measured playback with it); or find what asks for the offscreen surface
before the window is exposed and avoid it. A warm-up context on a helper thread won't help, since
the cost is per thread.

### 3. Rune's Vulkan device: about 250 ms (Rune only)

`for_surface` (Rune's `Host::create` → wgpu instance, adapter and device) runs 0.12 to 0.44 s, on
the main thread, after the bundle has loaded. The CPU profile sees 52 ms of it
(`vkEnumerateInstanceExtensionProperties` 37 ms while the loader opens NVIDIA's ICD,
`vkCreateDevice` 9 ms); the rest is kernel. Only Vulkan is created; there is no GL device.

Vulkan on its own (`standalone/vulkan-setup.txt`):

| | wall | sys |
|---|---|---|
| `vkCreateInstance` | 43 to 60 ms | 33 to 50 |
| `vkCreateDevice` | 162 to 202 ms | 159 to 192 |

That is about 205 ms of kernel time no matter where it runs, but it doesn't have to run in turn
with everything else.

**Fix idea:** start the wgpu instance, adapter and device on a thread at the top of `main`,
overlapping the bundle load, the probes and the fonts (0.02 to 0.12 s, about 100 ms on the main
thread), and only create the surface on the main thread. The device doesn't depend on the window;
check surface support once both exist.

### 4. Second expose after Hyprland sets the scale: 110 to 123 ms (Qt)

Right after the first frame, `QWaylandWindow::setScale` → `sendExposeEvent` blocks the GUI thread
another 110 to 123 ms (`offcpu/qt-after-encoff-2.txt`). On foundry2 this was about 25 ms. It is
after the first frame, so it delays the first interactive frame, not the first visible one. My
guess (not profiled) is the window's buffers being reallocated at the new scale, which on NVIDIA
is more kernel time.

**Fix idea:** if that's it, getting the output's scale before the first expose would leave one
buffer allocation instead of two. Worth a look at whether Qt's Wayland plugin can be told the
preferred scale up front.

### 5. Dynamic linking: 70 to 86 ms (Qt), 25 to 33 ms (Rune)

The first 75 ms of Qt's main thread is `ld-linux` resolving symbols, and the Qt plugins it loads
later add more (`QFactoryLoader::instance`, 47 ms with everything it pulls in). Rune links less.

**Fix idea:** fewer plugin loads (see 6) is the part we control.

### 6. GTK platform theme: 30 to 38 ms (Qt)

Omarchy sets `QT_QPA_PLATFORMTHEME=gtk3`, so `QGuiApplication` loads `libqgtk3.so`, which runs
`gtk_init` (18 ms) and loads GTK's CSS. The app uses the Basic style and its own colours.
`main.cpp` already clears the variable for screenshots and headless runs.

**Fix idea:** clear it for the editor too, if nothing needs GTK's file dialog (the Insert and
background image pickers would fall back to Qt Quick's own).

### 7. QML creation: 58 to 66 ms (Qt)

Spread thin, as on foundry2. The QML profiler's top item before the first frame is `IconImage`
creation in `OmButton.qml` (lines 60 and 92), two per button, 130 buttons; its times are inflated
by the profiler but the count is real (`qml/qt-after-encoff-until0.5.txt`).

**Fix idea:** line 92 is the trailing icon, which every button creates even when
`trailingIconSource` is empty and the item is hidden; line 60 is the leading icon, created even for
buttons with no icon or with `leadingDot`. A `Loader { active: ... }` around each (or creating them
only when a source is set) skips most of the 260, since few buttons have a trailing icon.

### Smaller things

- **Wallpaper picker:** each finished wallpaper thumbnail sends `WallpapersChanged`, and
  `BackgroundPanel.qml:31` `wallpaperEntries` rebuilds the whole list each time: 100 calls,
  286 ms in the QML profile, after the first frame. It only happens while the picker's thumbnails
  are being made (first launch, or after a theme change), since they are cached in
  `~/.cache/omareel/wallpapers`. Coalescing the signal (one per batch) fixes it.
- **Rune waits:** about 100 ms of real sleeping before the first frame, the biggest being
  a `Command::output` on the main thread at 0.517 s (11 ms) and Wayland roundtrips.

## The filmstrip on aarch64

| | frames | first frame at | fill span | rate |
|---|---|---|---|---|
| Qt before | 78 | 2.54 s | 11.74 s | 6.6 frames/s |
| Qt after | 78 | 2.24 s | 1.87 s | 41.7 frames/s |
| Qt after, probe off | 78 | 0.31 s | 1.77 s | 44.7 frames/s |
| Rune before | 47 | 1.09 s | 8.21 s | 5.7 frames/s |
| Rune after | 47 | 0.54 s | 0.93 s | 50.8 frames/s |

`thumbnail_workers` is `20 × 3/4 / 2 = 7` here, so each frame costs a worker 140 to 155 ms: the
decode from the keyframe, up to a second of 1440p60. That is 7 to 9 times faster than before and
the biggest change on n1x after the probe. Two things would take it further:

- The keyframe-first stand-in from `STARTUP.md`: almost all of the 140 ms is decoding the frames
  between the keyframe and the one wanted.
- The clamp to 8 workers doesn't bind here (7), but half of n1x's cores are Cortex-A725s. At nice
  10, the workers likely land on the little cores while the big ones run the UI; I didn't record
  CPU placement (perf here records per thread, not per CPU), so this is a guess worth a
  `taskset` test.

## Regression checks

All on the after tree.

- `cargo test --offline --workspace --exclude omareel_qt`: 356 passed, 0 failed.
- `cargo test --release` in `spikes/rune`: 3 passed.
- `tools/parity/compare-binaries.sh` (before vs after Qt, with screenshots through a venv with
  Pillow): no differences.
- `ctest` (`QT_QPA_PLATFORM=offscreen`): 4 of 5 pass. `cameracapture` fails, **and fails the same
  way on 4fd1696**, so it isn't this change:
  `cameraFramesKeepTheirTimes`, `everyPackedFormatEncodes(NV12)`, `everyPackedFormatEncodes(NV21)`
  and `killedCameraEncoderKeepsCompletedFragments`. The cause is ffmpeg n9.0.2 on aarch64: the
  camera encoder's filter (`scale=in_color_matrix=bt601:in_range=tv:out_color_matrix=bt709:out_range=tv,format=yuv420p`)
  crashes on NV12 input with SIGSEGV or SIGABRT, and the core dump's stack is glibc aborting in
  `sws_freeContext` (heap corruption in libswscale). Plain `format=yuv420p`, the matrix change
  alone, or the range change alone don't crash; both together do. A webcam that delivers NV12
  would hit this when recording on n1x.
- Recording loop: no regression (table above).
- Playback (`measure.sh preview`, 3 runs, `RUNE_MODE=editor`): no regression. Before and after
  match within noise for both apps: 1080p CPU Rune 91.0 → 90.7 %, Qt 73.5 → 76.5 %; 4K Rune
  104.6 → 103.6 %, Qt 166.2 → 170.7 %. **Both ran at 15.1 frames/s**, though, against 59.5 in
  `RESULTS-n1x.md`: since that run DP-7 has become a 1280x360 strip, so `in-nested.sh` can't place
  the 1920x1080 nested window clear of the other monitors and it hangs mostly off screen. The
  comparison holds (same cap both sides); the absolute frame rates and drops don't. Moving the
  window to DP-6 or eDP-1 would cover a screen in use, so I left it.
- Export (`measure.sh export`, 3 runs): no regression. 1080p Rune 4.64 → 4.59 s, Qt 7.15 → 6.98 s;
  4K Rune 8.38 → 8.37 s, Qt 10.47 → 10.54 s.

## How it was run

- perf 7.3.1 from Arch Linux ARM's `perf` and `libpfm` packages (signatures checked), unpacked into
  `~/.cache/omareel-spikes/tools` with a wrapper. **On n1x every `--call-graph` mode fails with
  "Failure to open any events" unless `--user-callchains` is added**, so the runs passed it through
  `PERF_EVENT` (`PERF_EVENT="-F 2999 --user-callchains"`). No script change was needed for that.
- Qt profiles: `build/prof` (RelWithDebInfo, `-DQT_QML_DEBUG -fno-omit-frame-pointer`, Rust with
  frame pointers), 3 runs with `CALLGRAPH=fp` and 3 with `CALLGRAPH=dwarf,8192`, `SECONDS_RUN=4`
  (Qt's first frame is at 2.4 s here). Arch's Qt on aarch64 saves the frame record but doesn't set
  `x29`, so frame-pointer stacks break inside Qt; DWARF covered those, and the two agree.
  NVIDIA's libraries have neither, so stacks stop there either way.
- Rune profiles: the release build with `CALLGRAPH=dwarf,8192`, and a frame-pointer build
  (`RUSTFLAGS=-C force-frame-pointers=yes`, its own target dir) with `CALLGRAPH=fp`, 3 runs each;
  waits with `PERF_EVENT="-F 9999 --switch-events --user-callchains" CALLGRAPH=fp`.
- `qml-profile.py` needs `/usr/lib/qt6/bin` on `PATH` for `qmlprofiler`.
- Script changes: `analyze-startup.py` has a "Qt Multimedia, FFmpeg and CUDA" kind and counts
  `libnvidia-*` as GPU setup, so the NVIDIA time isn't "other". New `scripts/thread-stacks.py`
  sums one thread's stacks over a time window (used for the render thread).
- The standalone timings in `results-n1x/standalone/` come from small ctypes scripts kept there.
