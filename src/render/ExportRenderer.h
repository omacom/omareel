#pragma once

#include "Exporter.h"

#include <QByteArray>
#include <QImage>
#include <QString>
#include <array>
#include <memory>

class QOffscreenSurface;
class QOpenGLContext;
class QOpenGLExtraFunctions;
class QOpenGLFramebufferObject;
class QQmlComponent;
class QQmlEngine;
class QQuickItem;
class QQuickRenderControl;
class QQuickWindow;

namespace Omareel {

// Renders Composition.qml offscreen for the exporter, one frame at a time. With OpenGL it
// renders through QQuickRenderControl into a framebuffer and reads each frame back
// asynchronously, two frames late; on the software scene graph, which QQuickRenderControl
// does not support, it grabs a hidden window instead. Frames come back as tightly packed RGBA
// rows, bottom-up from OpenGL and top-down from the software path.
class ExportRenderer
{
public:
    // Time spent per stage across all frames, for --timing.
    struct Timing {
        qint64 uploadNs = 0;
        qint64 renderNs = 0;
        qint64 readbackNs = 0;
    };

    ExportRenderer();
    ~ExportRenderer();

    // What the composition shows. Fill it before init(), which loads Composition.qml against
    // it, and again before each frame.
    CompositionState &state() { return m_state; }
    bool init(int width, int height, bool softwareRendering, QString *error);
    // Hands the composition the frame's pictures (a null image leaves that layer's as it was),
    // renders it, and returns in *pixels the frame that is ready: this one in software, the one
    // two frames back with OpenGL, or none yet.
    bool renderFrame(const QImage &screen, const QImage &camera, const QImage &insert,
                     QByteArray *pixels, QString *error);
    // After the last frame: the frames still being read back, oldest first, one per call; false
    // once none is left. A frame that could not be read comes back empty with *error set.
    bool takePendingFrame(QByteArray *pixels, QString *error);
    // Frees the readback buffers once no frame is left to take.
    void releaseReadbacks();
    // Releases the scene graph's resources before a successful export tears the renderer down.
    void invalidate();
    // Runs the event loop once, then makes the export's OpenGL context current again.
    void processEvents();
    const Timing &timing() const { return m_timing; }

private:
    struct ReadbackSlot { unsigned int buffer = 0; int frame = -1; };
    QByteArray mapReadback(ReadbackSlot &slot, QString *error);

    int m_width = 0;
    int m_height = 0;
    bool m_softwareRendering = false;
    // Torn down in reverse: the composition, the QML engine, the state it reads, the window,
    // the render control, the framebuffer, the surface, and last the context.
    std::unique_ptr<QOpenGLContext> m_context;
    std::unique_ptr<QOffscreenSurface> m_surface;
    std::unique_ptr<QOpenGLFramebufferObject> m_fbo;
    std::unique_ptr<QQuickRenderControl> m_renderControl;
    std::unique_ptr<QQuickWindow> m_window;
    CompositionState m_state;
    std::unique_ptr<QQmlEngine> m_engine;
    std::unique_ptr<QQmlComponent> m_component;
    std::unique_ptr<QObject> m_rootObject;
    QQuickItem *m_frameSource = nullptr;
    QQuickItem *m_cameraFrameSource = nullptr;
    QQuickItem *m_mediaFrameSource = nullptr;
    std::array<ReadbackSlot, 2> m_readbacks;
    qsizetype m_frameBytes = 0;
    QOpenGLExtraFunctions *m_glExtra = nullptr;
    int m_frame = 0;
    int m_pendingFrame = -1;
    Timing m_timing;
};

} // namespace Omareel
