#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/render/OpenGL.hpp>
#include <hyprland/src/render/Renderer.hpp>
// CMake sets OMAREEL_HYPRLAND_MINOR from the headers' version.h. 0.56 is built only while
// Omarchy's stable channel runs it; each #if below goes with it.
#if OMAREEL_HYPRLAND_MINOR >= 57
#include <hyprland/src/render/Context.hpp>
#endif
#include <hyprland/src/desktop/view/LayerSurface.hpp>
#include <hyprland/src/desktop/view/Popup.hpp>
#include <hyprland/src/desktop/view/WLSurface.hpp>
#include <hyprland/src/output/Monitor.hpp>
#include <hyprland/src/state/MonitorState.hpp>
#include <hyprland/src/protocols/LayerShell.hpp>
#include <hyprland/src/protocols/core/Compositor.hpp>
#include <hyprland/src/managers/SessionLockManager.hpp>
#include <algorithm>
#include <sstream>
#include <stdexcept>
#include <hyprutils/utils/ScopeGuard.hpp>
#if defined(__aarch64__)
#include "Arm64Hook.hpp"
#endif

namespace {
constexpr auto VERSION = "0.1.0";
HANDLE handle = nullptr;

// Hyprland's function hooks only patch x86-64 code, so aarch64 uses the plugin's own.
class RenderHook {
  public:
    bool install(void* source, void* destination) {
#if defined(__aarch64__)
        return m_hook.install(source, destination);
#else
        m_hook = HyprlandAPI::createFunctionHook(handle, source, destination);
        return m_hook && m_hook->hook();
#endif
    }
    void remove() {
#if defined(__aarch64__)
        m_hook.remove();
#else
        if (m_hook)
            m_hook->unhook();
        m_hook = nullptr;
#endif
    }
    void* original() const {
#if defined(__aarch64__)
        return m_hook.original();
#else
        return m_hook->m_original;
#endif
    }
    std::string error() const {
#if defined(__aarch64__)
        return m_hook.error();
#else
        return "Hyprland could not hook it";
#endif
    }

  private:
#if defined(__aarch64__)
    Arm64Hook m_hook;
#else
    CFunctionHook* m_hook = nullptr;
#endif
};
RenderHook layerHook;
RenderHook mirrorHook;
std::vector<std::string> prefixes{"omareel-"};
#if OMAREEL_HYPRLAND_MINOR >= 57
SP<IPC::Socket1::SCommand> command;
using LayerFn = void (*)(Render::IHyprRenderer*, Render::CRenderContext&, PHLLS, PHLMONITOR, const Time::steady_tp&, bool, bool, bool);
using MirrorFn = bool (*)(Render::GL::CHyprOpenGLImpl*, Render::CRenderContext&, const CBox&);
#else
SP<SHyprCtlCommand> command;
using LayerFn = void (*)(Render::IHyprRenderer*, PHLLS, PHLMONITOR, const Time::steady_tp&, bool, bool);
using MirrorFn = bool (*)(Render::GL::CHyprOpenGLImpl*, const CBox&);

// end() enables the final-output shader before the mirror hook. Surface textures can
// have no image description, and OpenGL.cpp:1523 dereferences it in that mode.
// A typed member pointer keeps this narrowly scoped access checked by the compiler;
// no hard-coded offsets or global redefinition of private are used.
struct FinalShaderMember {
    using Type = bool Render::GL::CHyprOpenGLImpl::*;
    friend Type finalShaderMember(FinalShaderMember);
};
template <typename Tag, typename Tag::Type Member> struct MemberAccess {
    friend typename Tag::Type finalShaderMember(Tag) { return Member; }
};
template struct MemberAccess<FinalShaderMember, &Render::GL::CHyprOpenGLImpl::m_applyFinalShader>;
#endif

bool matches(const PHLLS& layer) {
    return layer && std::ranges::any_of(prefixes, [&](const auto& prefix) { return layer->m_namespace.starts_with(prefix); });
}

#if OMAREEL_HYPRLAND_MINOR >= 57
// What renderLayer draws on a monitor: mapped, not fading out, not fully transparent.
bool shown(const PHLLS& layer) {
    return layer->mapped() && layer->acceptsInput() && layer->alphaNonZero();
}

void renderLayer(Render::IHyprRenderer* renderer, Render::CRenderContext& ctx, PHLLS layer, PHLMONITOR monitor,
                 const Time::steady_tp& when, bool popups, bool lockscreen, bool workspaceScene) {
    // Snapshots on a captured monitor omit these layers too, as do workspace captures.
    if (matches(layer) && (ctx.m_renderingCapture || (monitor && monitor->needsACopyFB())))
        return;
    reinterpret_cast<LayerFn>(layerHook.original())(renderer, ctx, layer, monitor, when, popups, lockscreen, workspaceScene);
}

void drawLayer(Render::CRenderContext& ctx, const PHLLS& layer, const PHLMONITOR& monitor, bool popups) {
    if (!matches(layer) || !shown(layer) || !layer->wlSurface() || !layer->wlSurface()->resource())
        return;
    // Never paint an ordinary layer over the session lock.
    if (g_pSessionLockManager->isSessionLocked() && !layer->m_ruleApplicator->aboveLock().valueOrDefault())
        return;
    const auto size = layer->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
    CSurfacePassElement::SRenderData data{monitor, Time::steadyNow(), layer->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT)};
    data.fadeAlpha = layer->alpha()[Desktop::View::LS_ALPHA_FADE]->value();
    data.surface = layer->wlSurface()->resource();
    data.w = size.x;
    data.h = size.y;
    data.pLS = layer;
    data.decorate = false;
    data.blur = false;
    data.clipBox = CBox{0, 0, monitor->m_size.x, monitor->m_size.y}.scale(monitor->m_scale).round();
    const auto draw = [&](SP<CWLSurfaceResource> surface, const Vector2D& offset, bool main) {
        if (!surface || !surface->m_current.texture || surface->m_current.size.x < 1 || surface->m_current.size.y < 1)
            return;
        data.localPos = offset;
        data.texture = surface->m_current.texture;
        data.surface = surface;
        data.mainSurface = main;
        g_pHyprRenderer->draw(ctx, data, ctx.m_data.damage);
        ++data.surfaceCounter;
    };
    if (!popups) {
        layer->wlSurface()->resource()->breadthfirst([&](SP<CWLSurfaceResource> surface, const Vector2D& offset, void*) {
            draw(surface, offset, surface == layer->wlSurface()->resource());
        }, nullptr);
    } else if (layer->popupHead()) {
        data.squishOversized = false;
        data.dontRound = true;
        data.popup = true;
        data.discardMode &= ~DISCARD_ALPHA;
        layer->popupHead()->breadthfirst([&](WP<Desktop::View::CPopup> popup, void*) {
            if (popup && popup->mapped() && popup->resource() && popup->alphaNonZero() && popup->acceptsInput())
                draw(popup->wlSurface()->resource(), popup->coordsRelativeToParent(), false);
        }, nullptr);
    }
}

bool saveMirror(Render::GL::CHyprOpenGLImpl* gl, Render::CRenderContext& ctx, const CBox& box) {
    const bool result = reinterpret_cast<MirrorFn>(mirrorHook.original())(gl, ctx, box);
    const auto monitor = ctx.m_data.pMonitor.lock();
    if (!monitor)
        return result;
    // bind() only binds the framebuffer and viewport; draw-buffer state belongs to the FB.
    GLint count = 0;
    glGetIntegerv(GL_MAX_DRAW_BUFFERS, &count);
    std::vector<GLenum> buffers(count);
    for (int index = 0; index < count; ++index) {
        GLint buffer = GL_NONE;
        glGetIntegerv(GL_DRAW_BUFFER0 + index, &buffer);
        buffers[index] = buffer;
    }
    const GLenum displayOnly = GL_COLOR_ATTACHMENT0;
    glDrawBuffers(1, &displayOnly);
    // end() enables the final-output shader only after the mirror copy, but never let a surface
    // texture without an image description reach it.
    const bool savedFinalShader = ctx.m_gl.applyFinalShader;
    // Surface drawing changes the clip box, UVs and filtering; the scope puts them back.
    const auto drawState = ctx.saveDrawState();
    ctx.m_gl.applyFinalShader = false;
    Hyprutils::Utils::CScopeGuard restore([&] {
        ctx.m_gl.applyFinalShader = savedFinalShader;
        glDrawBuffers(count, buffers.data());
    });
    for (const auto plane : {ZWLR_LAYER_SHELL_V1_LAYER_TOP, ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY})
        for (const auto& weak : monitor->m_layerSurfaceLayers[plane])
            drawLayer(ctx, weak.lock(), monitor, false);
    for (const auto& plane : monitor->m_layerSurfaceLayers)
        for (const auto& weak : plane)
            drawLayer(ctx, weak.lock(), monitor, true);
    return result;
}
#else
// What renderLayer draws on a monitor.
bool shown(const PHLLS& layer) {
    return layer->visible();
}

void renderLayer(Render::IHyprRenderer* renderer, PHLLS layer, PHLMONITOR monitor,
                 const Time::steady_tp& when, bool popups, bool lockscreen) {
    // m_fakeFrame is private in 0.56.2; snapshots on a captured monitor omit these layers too.
    if (matches(layer) && monitor && monitor->needsACopyFB())
        return;
    reinterpret_cast<LayerFn>(layerHook.original())(renderer, layer, monitor, when, popups, lockscreen);
}

void drawLayer(const PHLLS& layer, const PHLMONITOR& monitor, bool popups) {
    if (!matches(layer) || !layer->visible() || !layer->wlSurface() || !layer->wlSurface()->resource())
        return;
    // Never paint an ordinary layer over the session lock.
    if (g_pSessionLockManager->isSessionLocked() && !layer->m_ruleApplicator->aboveLock().valueOrDefault())
        return;
    const auto size = layer->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
    CSurfacePassElement::SRenderData data{monitor, Time::steadyNow(), layer->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT)};
    data.fadeAlpha = layer->alpha()[Desktop::View::LS_ALPHA_FADE]->value();
    data.surface = layer->wlSurface()->resource();
    data.w = size.x;
    data.h = size.y;
    data.pLS = layer;
    data.decorate = false;
    data.blur = false;
    data.clipBox = CBox{0, 0, monitor->m_size.x, monitor->m_size.y}.scale(monitor->m_scale).round();
    const auto draw = [&](SP<CWLSurfaceResource> surface, const Vector2D& offset, bool main) {
        if (!layer->visible() || !surface || !surface->m_current.texture || surface->m_current.size.x < 1 || surface->m_current.size.y < 1)
            return;
        data.localPos = offset;
        data.texture = surface->m_current.texture;
        data.surface = surface;
        data.mainSurface = main;
        g_pHyprRenderer->draw(data, g_pHyprRenderer->m_renderData.damage);
        ++data.surfaceCounter;
    };
    if (!popups) {
        layer->wlSurface()->resource()->breadthfirst([&](SP<CWLSurfaceResource> surface, const Vector2D& offset, void*) {
            draw(surface, offset, surface == layer->wlSurface()->resource());
        }, nullptr);
    } else if (layer->m_popupHead) {
        data.squishOversized = false;
        data.dontRound = true;
        data.popup = true;
        data.discardMode &= ~DISCARD_ALPHA;
        layer->m_popupHead->breadthfirst([&](WP<Desktop::View::CPopup> popup, void*) {
            if (popup && popup->aliveAndVisible())
                draw(popup->wlSurface()->resource(), popup->coordsRelativeToParent(), false);
        }, nullptr);
    }
}

bool saveMirror(Render::GL::CHyprOpenGLImpl* gl, const CBox& box) {
    const bool result = reinterpret_cast<MirrorFn>(mirrorHook.original())(gl, box);
    const auto monitor = g_pHyprRenderer->m_renderData.pMonitor.lock();
    if (!monitor)
        return result;
    // bind() only binds the framebuffer and viewport; draw-buffer state belongs to the FB.
    GLint count = 0;
    glGetIntegerv(GL_MAX_DRAW_BUFFERS, &count);
    std::vector<GLenum> buffers(count);
    for (int index = 0; index < count; ++index) {
        GLint buffer = GL_NONE;
        glGetIntegerv(GL_DRAW_BUFFER0 + index, &buffer);
        buffers[index] = buffer;
    }
    const GLenum displayOnly = GL_COLOR_ATTACHMENT0;
    glDrawBuffers(1, &displayOnly);
    auto& finalShader = gl->*finalShaderMember(FinalShaderMember{});
    const bool savedFinalShader = finalShader;
    auto& renderData = g_pHyprRenderer->m_renderData;
    const auto savedClip = renderData.clipBox;
    const auto savedNearest = renderData.useNearestNeighbor;
    const auto savedUVTop = renderData.primarySurfaceUVTopLeft;
    const auto savedUVBottom = renderData.primarySurfaceUVBottomRight;
    finalShader = false;
    // end() has enabled monitor transforms for its final blit; surface drawing uses the scene state.
    g_pHyprRenderer->pushMonitorTransformEnabled(false);
    Hyprutils::Utils::CScopeGuard restore([&] {
        g_pHyprRenderer->popMonitorTransformEnabled();
        finalShader = savedFinalShader;
        renderData.clipBox = savedClip;
        renderData.useNearestNeighbor = savedNearest;
        renderData.primarySurfaceUVTopLeft = savedUVTop;
        renderData.primarySurfaceUVBottomRight = savedUVBottom;
        glDrawBuffers(count, buffers.data());
    });
    for (const auto plane : {ZWLR_LAYER_SHELL_V1_LAYER_TOP, ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY})
        for (const auto& weak : monitor->m_layerSurfaceLayers[plane])
            drawLayer(weak.lock(), monitor, false);
    for (const auto& plane : monitor->m_layerSurfaceLayers)
        for (const auto& weak : plane)
            drawLayer(weak.lock(), monitor, true);
    return result;
}
#endif

std::string quote(const std::string& text) {
    std::string result = "\"";
    constexpr char hex[] = "0123456789abcdef";
    for (unsigned char c : text) {
        if (c == '\\' || c == '"') { result += '\\'; result += c; }
        else if (c < 32) { result += "\\u00"; result += hex[c >> 4]; result += hex[c & 15]; }
        else result += c;
    }
    return result + "\"";
}

std::string status() {
    std::string result = "{\"version\":" + quote(VERSION) + ",\"built_hash\":" + quote(GIT_COMMIT_HASH) + ",\"prefixes\":[";
    for (size_t i = 0; i < prefixes.size(); ++i) result += (i ? "," : "") + quote(prefixes[i]);
    result += "],\"monitors\":[";
    bool first = true;
    for (const auto& monitor : State::monitorState()->monitors()) {
        int excluded = 0;
        for (const auto& plane : monitor->m_layerSurfaceLayers)
            for (const auto& weak : plane) {
                const auto layer = weak.lock();
                if (matches(layer) && shown(layer)) ++excluded;
            }
        if (!first) result += ",";
        first = false;
        result += "{\"name\":" + quote(monitor->m_name) + ",\"capturing\":" + (monitor->needsACopyFB() ? "true" : "false")
            + ",\"excluded_layers\":" + std::to_string(excluded) + "}";
    }
    return result + "]}";
}
}

APICALL EXPORT std::string PLUGIN_API_VERSION() { return HYPRLAND_API_VERSION; }

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE owner) {
    handle = owner;
    if (HyprlandAPI::getHyprlandVersion(handle).hash != GIT_COMMIT_HASH)
        throw std::runtime_error("omareel-capture-exclude: Hyprland commit mismatch; rebuild the plugin");
    const auto layers = HyprlandAPI::findFunctionsByName(handle, "renderLayer");
    const auto mirrors = HyprlandAPI::findFunctionsByName(handle, "saveBufferForMirror");
    if (layers.size() != 1 || mirrors.size() != 1)
        throw std::runtime_error("omareel-capture-exclude: expected exactly one renderLayer and saveBufferForMirror symbol");
    if (!layerHook.install(layers[0].address, reinterpret_cast<void*>(renderLayer)))
        throw std::runtime_error("omareel-capture-exclude: could not hook renderLayer: " + layerHook.error());
    if (!mirrorHook.install(mirrors[0].address, reinterpret_cast<void*>(saveMirror))) {
        layerHook.remove();
        throw std::runtime_error("omareel-capture-exclude: could not hook saveBufferForMirror: " + mirrorHook.error());
    }
#if OMAREEL_HYPRLAND_MINOR >= 57
    command = HyprlandAPI::registerHyprCtlCommand(handle, {"omareel-exclude", IPC::Socket1::COMMAND_MATCH_PREFIX,
        [](const IPC::Socket1::SRequest& request) -> IPC::Socket1::SResponse {
            std::istringstream input(request.command);
#else
    command = HyprlandAPI::registerHyprCtlCommand(handle, {"omareel-exclude", false,
        [](eHyprCtlOutputFormat, std::string request) {
            std::istringstream input(request);
#endif
            std::string name, action, prefix, extra;
            input >> name >> action >> prefix >> extra;
            if (action == "status" && prefix.empty()) return status();
            if ((action == "add" || action == "remove") && !prefix.empty() && extra.empty()) {
                if (action == "add" && std::ranges::find(prefixes, prefix) == prefixes.end()) prefixes.push_back(prefix);
                if (action == "remove") std::erase(prefixes, prefix);
                return status();
            }
            return std::string{"{\"error\":\"usage: omareel-exclude status | add <prefix> | remove <prefix>\"}"};
        }});
    if (!command) {
        layerHook.remove();
        mirrorHook.remove();
        throw std::runtime_error("omareel-capture-exclude: could not register status command");
    }
    return {"omareel-capture-exclude", "Display-only recording overlays", "Omareel", VERSION};
}

APICALL EXPORT void PLUGIN_EXIT() {
    layerHook.remove();
    mirrorHook.remove();
    if (command) HyprlandAPI::unregisterHyprCtlCommand(handle, command);
    command.reset();
}
