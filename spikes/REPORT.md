# GPU UI spikes: GPUI, iced and Slint against Qt Quick

Each spike built what `SPEC.md` asks over `omareel_core`:

- **The preview window:** the real composition (wallpaper, rounded screen frame with shadow, zoom, cursor, webcam bubble with blurred backdrop, keystrokes) playing a recorded bundle at 60 Hz.
- **An offscreen export** of the same scene piped to ffmpeg.
- **A layer-shell recording bar.**

Each part was measured against the Qt build. The three toolkit reports have the details, the workarounds and the n1x instructions:

- `gpui/REPORT.md`
- `iced/REPORT.md`
- `slint/REPORT.md`

**How it was measured:**
- All runs were on foundry2 (Intel Iris Xe). Previews ran in the nested Hyprland on a 60 Hz headless output.
- Each spike measured its own Qt baseline in the same session. The three Qt export baselines agree within 4%.
- GPUI's runs were taken under more machine load than the others: export at load 6–9.
- Nothing was measured on n1x (aarch64, NVIDIA).

## Results

| | Qt Quick | GPUI | iced | Slint |
|---|---|---|---|---|
| Toolkit version | Qt 6.11 | zed git `a5c4ca8` (crates.io too old) | 0.14.0 | 1.18.1 |
| Fork or patch needed | – | no | no | no |
| Unstable / pinned | – | git pin | `Cargo.lock` pin for iced_layershell | `unstable-wgpu-30` for video and export |
| Preview 1080p fps, p95 frame ms | 58–60, 16.9–17.0 | 59.5, 19.1 | 59.6, 17.1 | 59.8, 19.7 |
| Preview 4K fps, p95 frame ms | 57–60, 17.0–17.5 | **33.3**, 43.3 | 59.6, 17.1 | 59.8, 21.5 |
| Preview 4K CPU, app + decoders (% of a core) | 20 (decode on the GPU) | 126 + 234 | 47 + 202 | 126 + 236 |
| Preview 4K GPU render engine % | 9–10 | 29 | 23 | 41 |
| Preview 4K RSS, app (MB) | 637–668 | 454 | 75 | 240 |
| Export fps, 1080p bundle | 86.6–90.3 | 45.5 | **119.8** | 110.2 |
| Export fps, 4K bundle | 49.0–51.6 | 24.9 | 55.8 | 55.4 |
| Composition code (QML: 681 lines) | – | 538 | 1,645 | 484 |
| Offscreen rendering | QQuickRenderControl | test-only `HeadlessAppContext` | headless wgpu | custom platform (unstable API) |
| Layer-shell bar | LayerShellQt | built in | iced_layershell | hand-written platform (300 lines) |
| Blur | MultiEffect | CPU, 24×24 | WGSL | CPU |
| Stripped binary | 9–14 MB + Qt | 31 MB | 19.8 MB | 25.7 MB |
| Clean / incremental release build | 88 s / – | 2–2.5 min / 4 s | 81 s / 7 s | 82 s / 7 s |

## What all three share

**No hardware video decoding.**
- None of the toolkits can take a VAAPI or NVDEC frame into wgpu without unsafe Vulkan interop, so every spike decodes with ffmpeg on the CPU.
- That costs about half a core at 1080p and two to three and a half cores at 4K.
- The Qt editor decodes on the GPU's video engine instead.
- On a laptop this is the largest real cost of moving off Qt Quick, and it is the same problem whichever toolkit is chosen.

**Getting frames onto the GPU is the hard part.**
- **GPUI** has no external-texture path on Linux. Every frame becomes a new image, converted and uploaded on the UI thread. That is why its 4K preview runs at 33 fps and its export at half Qt's speed.
- **iced and Slint** both reached 60 fps at 4K the same way: the decode threads fill mapped wgpu staging buffers, and the UI thread only records a copy.

**Qt Quick is cheaper per frame.**
- Qt's render engine is 9–12% busy, against 19–42% for the spikes.
- Slint's femtovg redraws the whole scene every frame, about 7.5 ms of CPU.
- iced is the cheapest of the three because its scene is one hand-written shader.

**The exports beat Qt where the frame path is right.**
- iced exports 1.33× faster than Qt from a 1080p bundle and Slint 1.22×. At 4K, decoding limits all of them.
- Neither is pixel-identical to Qt's export: PSNR is 39–41 dB and SSIM 0.995.

## Writing it with an agent

All three compiled with very few errors: one to three each, and rustc pointed at the fix. What got past the compiler was the same kind of thing in each:

- **GPUI:**
  - Frames that weren't freed by hand (5.7 GiB of GPU memory in a 10 s export, with no error).
  - A fixed 2× scale factor that quietly exported quarter-size frames.
- **iced:**
  - A WGSL uniform 16 bytes longer than its Rust struct, which panicked at frame 240.
  - A transitive prerelease bump that broke the build.
- **Slint:**
  - A monospace font that silently fell back to a proportional one.
  - Group opacity that differs from QML's per-child opacity.
  - Image caching that would show stale video frames.

## Verdict

**Keep Qt Quick for now.**
- It previews and exports at the same frame rates for a half to a quarter of the GPU time.
- It decodes video in hardware.
- It is already written.

With PRs #12–#15 the logic is in Rust and the QML is a thin layer over CXX-Qt objects, so this decision costs nothing to defer.

**If omareel drops Qt later, move to Slint.**
- Its markup is the closest to QML: the composition ported almost binding for binding.
- Its compile-time checks cover the markup too, which is what matters for agent-written code.
- The work in front of that move, in order:
  1. Hardware-decoded frames into wgpu, which gates every toolkit.
  2. A plain-wgpu composition renderer shared by the preview and the export, the design the iced spike ended up with.
  3. The layer-shell and export platforms, kept small and owned by omareel.

**Not GPUI.** Drawing was the easiest part, but video, effects and offscreen rendering are exactly where it is weakest. Fixing them means a fork of a framework that is run for Zed.

**Not iced as the main UI.** Its strength is the custom-shader path, and that is better used as a renderer under whichever UI omareel has. As a toolkit it leaves the effects, text and the rest of the editor to be written by hand: 2.4× the QML for the composition alone.

**The one piece worth taking now, whatever the UI:** the iced spike's wgpu renderer, run headless for export. It would replace the QQuickRenderControl/GL export path with a faster pure-Rust one. Its output differs from today's by a few dB of PSNR, so that is a visible change and a decision of its own.
