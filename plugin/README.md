# Omareel capture exclusion

`omareel-capture-exclude` renders `omareel-` layer-shell namespaces on the display
after Hyprland has copied the scene for capture. No pixel reconstruction or
redaction is involved. No plugin configuration is required.

## Build and install

Run `bin/build`, or configure the application with
`-DOMAREEL_BUILD_HYPRLAND_PLUGIN=ON` (the default). When `pkg-config hyprland` is
available, CMake builds `build/plugin/omareel-capture-exclude.so` and its adjacent
`.so.hash` file. The plugin can also be built separately:

```sh
cmake -S plugin -B /tmp/omareel-nested/plugin-build -G Ninja
cmake --build /tmp/omareel-nested/plugin-build
```

The application remains C++17; this Hyprland 0.57 plugin requires C++26 and a
compiler compatible with the compositor. Headers default to `/usr/include/hyprland`.
A private include-directory symlink takes precedence over stale copies under
`/usr/local/include`. `OMAREEL_HYPRLAND_HEADERS` selects another matching header
tree when building the standalone plugin. Rebuild after updating Hyprland.

CMake installation and `pkg/PKGBUILD` install both files under `/usr/lib/omareel/`.
The application checks the sidecar commit against `hyprctl -j version` before
loading. It searches an explicit `OMAREEL_PLUGIN_PATH` first (an explicit missing
path forces fallback), otherwise the executable's `plugin/` directory and then
the installed path. It does not unload the plugin after recording. With
`ecosystem:enforce_permissions` enabled, Hyprland shows a one-time permission
dialog; refusal leaves the application in fallback mode.

## Hooks and state

Each build loads only on the Hyprland commit it was compiled against. The code
follows Hyprland 0.57.1 (`acc1aa0bfd4bd575dcb0f76c35ccf12b660a1ab9`; 0.57.0 has the
same interfaces). References below are relative to that source tree.

* `src/render/Renderer.cpp:1016`: hook `IHyprRenderer::renderLayer` to omit matching
  layers, including popups, when that monitor's `needsACopyFB()` is true or the
  render context is a workspace capture (`m_renderingCapture`).
* `src/render/OpenGL.cpp:2391`: hook `saveBufferForMirror`, call the original,
  then draw visible TOP and OVERLAY surfaces immediately into the restored main
  framebuffer. Layer ordering and popup traversal follow
  `src/render/Renderer.cpp:1337` and `src/render/Renderer.cpp:1091`. Geometry,
  subsurfaces, clipping, fade alpha and visibility (mapped, accepting input,
  alpha above zero) follow `renderLayer`. Blur is disabled for these display-only
  surfaces.
* `src/render/OpenGL.cpp:834` and `:887` place this draw between capture-copy and
  the final-shader display blit. `src/render/gl/GLFramebuffer.cpp:48` gives a
  framebuffer with a mirror texture two draw buffers, and binding it again does
  not reset them. The hook saves their complete GL state, selects only
  `GL_COLOR_ATTACHMENT0`, and restores it afterwards, so the mirror attachment
  stays clean too.

Hyprland's own `no_screen_share` rule still draws a black box over the layer in
monitor captures (`src/managers/screenshare/ScreenshareFrame.cpp:272`); only
workspace captures omit it (`src/render/Context.cpp:37`).

Two necessary implementation details:

* The render context's `fakeFrame` is not passed to `renderLayer`, so the layer
  hook uses `needsACopyFB()` alone. Consequently compositor snapshots taken while
  that predicate is true also omit matching layers. Omareel applies `no_anim`
  before mapping its bar/self-view; the countdown shares the bar namespace.
  This avoids the unmap snapshot created at `src/desktop/view/LayerSurface.cpp:270`.
  The app disables its retired named rules before installing new animation rules:
  Lua updates merge effects on existing names.
* Immediate surface drawing with the final screen shader enabled dereferences
  the texture's image description, which surface textures can lack. `end()` only
  enables `ctx.m_gl.applyFinalShader` after the mirror copy, but the hook clears
  it for its draw anyway and restores it. Draw buffers are restored by a scope
  guard, and the render data (clipping, UVs, filtering) by the context's
  `saveDrawState()`. Damage, final damage, and the render pass are not modified.

Hyprland's `CFunctionHook` only patches x86-64 code; on any other architecture
`hook()` returns false (`src/plugins/HookSystem.cpp:147`). On aarch64 the plugin
hooks the same two functions itself with `Arm64Hook.hpp`: the first instruction
becomes a branch to a page within branch range, which jumps on to the hook and
holds a trampoline that runs the moved instruction and continues in the
original. PC-relative instructions are refused rather than relocated. A `bti c`
or `paciasp` landing pad stays where it is and the instruction after it moves;
for `paciasp` the return address is authenticated before the hook and signed
again for the original, and a BTI-guarded page stays guarded. In Omarchy's
aarch64 0.56.2 and 0.57.1 builds both functions start with `stp x29, x30, [sp, #-N]!`.
`test_arm64hook` runs under ctest on aarch64. From x86-64 it can be built with
clang against an Arch Linux ARM sysroot and run with `qemu-aarch64-static -cpu max`,
which enforces BTI and PAC.

Initialization rejects a different running commit or anything other than one
symbol match for each hook. Partial installation rolls back; exit unhooks both
functions. Monitor/layer lists are traversed through weak pointers and visibility
is checked at draw time. There is no retained per-monitor state.

```sh
hyprctl omareel-exclude status
hyprctl omareel-exclude add my-display-overlay-
hyprctl omareel-exclude remove my-display-overlay-
```

The JSON status contains version, `built_hash`, prefixes, and each monitor's
`capturing` predicate and visible matching layer count. `capturing` is an
instantaneous `needsACopyFB()` value; it can be false between individual capture
requests. Add/remove arguments are literal prefixes, without glob syntax. Keep
`omareel-` enabled for application integration. Custom excluded layers should use
TOP/OVERLAY and `no_anim` too.

## Limits and verification

Software cursors (including zoom) are drawn underneath these layers. Mirror
outputs of the recorded monitor omit the layers. The gsr backend ignores this
plugin, so Omareel uses fallback placement with that backend. Display-only
layers are drawn above the already-composited scene and do not blur it.

Never experiment with loading or unloading this plugin in a working desktop
session. Use the nested harness, which verifies both the private IPC signature
and private display socket, bounds its test body, and tears down its compositor
and detached clients on success or failure. For example:

```sh
tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/pixels --timeout 150 -- \
  python3 tests/tools/capture-exclusion-proof.py pixels
```

Other proof modes are `hygiene` (forces MRT), `overlays`, `fallback`,
`fallback-two`, `host-plugin` (the real UI with retired rules present),
`stability` (200 separate screencopy sessions), and `recorder-cycles` (200 full
record/stop operations; use `--timeout 300`). Pixel checks
need grim, ffmpeg, Python, Pillow, and NumPy. The display proof positions only the
harness's own outer window on DP-3 (or the first monitor when there is no DP-3)
and makes that window opaque. Hidden
`__layer-test` and `__window-test` clients exit within 60 seconds. Reports,
command logs, recordings, and all extracted frames remain in the supplied
artifact directory.
