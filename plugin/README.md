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

The application remains C++17; this Hyprland 0.56.2 plugin requires C++26 and a
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

This implementation is tied to Hyprland commit
`efb50993780079460b0cbed1363e2166a2de1d9f`. References below are relative to that
source tree (the development checkout is `/tmp/hyprland-src`).

* `src/render/Renderer.cpp:935`: hook `IHyprRenderer::renderLayer` to omit matching
  layers, including popups, when that monitor's `needsACopyFB()` is true.
* `src/render/OpenGL.cpp:2533`: hook `saveBufferForMirror`, call the original,
  then draw visible TOP and OVERLAY surfaces immediately into the restored main
  framebuffer. Layer ordering and popup traversal follow
  `src/render/Renderer.cpp:1233` and `src/render/Renderer.cpp:1007`. Geometry,
  subsurfaces, clipping, and fade alpha follow `src/render/Renderer.cpp:957`.
  Blur is disabled for these display-only surfaces.
* `src/render/OpenGL.cpp:799` and `src/render/OpenGL.cpp:808` place this draw
  between capture-copy and display-blit. `src/render/gl/GLFramebuffer.cpp:91`
  does not restore draw buffers. The hook saves their complete GL state,
  selects only `GL_COLOR_ATTACHMENT0`, and restores it afterwards. Thus the
  MRT attachment created at `src/render/OpenGL.cpp:760` stays clean too.

Two necessary implementation details differ from the initial design:

* `m_fakeFrame` is private (`src/render/OpenGL.hpp:303`, `:319`). The layer hook
  uses `needsACopyFB()` alone. Consequently compositor snapshots taken while
  that predicate is true also omit matching layers. Omareel applies `no_anim`
  before mapping its bar/self-view; the countdown shares the bar namespace.
  This avoids the unmap snapshot created at `src/desktop/view/LayerSurface.cpp:247`.
  The app disables its retired named rules before installing new animation rules:
  Lua updates merge effects on existing names (`src/config/lua/bindings/LuaBindingsConfigRules.cpp:1301`).
* `end()` enables the private `m_applyFinalShader` flag before copying. Immediate
  surface drawing with that flag set crashes on textures without an image
  description (`src/render/OpenGL.cpp:1523`). A narrowly scoped, typed member
  accessor temporarily disables this flag for the overlay draw, then restores
  it for the display blit. This uses C++ explicit template instantiation,
  without offsets or redefining `private`. Draw buffers, monitor transforms,
  clipping, UV state, and nearest-neighbour state are restored by a scope guard.
  Damage, final damage, and the render pass are not modified.

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
harness's own outer window on DP-3 and makes that window opaque. Hidden
`__layer-test` and `__window-test` clients exit within 60 seconds. Reports,
command logs, recordings, and all extracted frames remain in the supplied
artifact directory.
