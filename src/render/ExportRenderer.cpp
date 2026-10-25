#include "ExportRenderer.h"

#include <QCoreApplication>
#include <QElapsedTimer>
#include <QOffscreenSurface>
#include <QOpenGLContext>
#include <QOpenGLExtraFunctions>
#include <QOpenGLFramebufferObject>
#include <QOpenGLFunctions>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickGraphicsDevice>
#include <QQuickItem>
#include <QQuickRenderControl>
#include <QQuickRenderTarget>
#include <QQuickWindow>
#include <algorithm>
#include <cstring>

using namespace Omareel;

ExportRenderer::ExportRenderer() = default;
ExportRenderer::~ExportRenderer() = default;

bool ExportRenderer::init(int width, int height, bool softwareRendering, QString *error)
{
    m_width = width;
    m_height = height;
    m_softwareRendering = softwareRendering;
    if (!softwareRendering) {
        m_context = std::make_unique<QOpenGLContext>();
        if (!m_context->create()) { if (error) *error = QStringLiteral("Could not create OpenGL context"); return false; }
        m_surface = std::make_unique<QOffscreenSurface>();
        m_surface->setFormat(m_context->format());
        m_surface->create();
        if (!m_surface->isValid() || !m_context->makeCurrent(m_surface.get())) {
            if (error) *error = QStringLiteral("Could not create an offscreen OpenGL surface");
            return false;
        }
        QOpenGLFramebufferObjectFormat fboFormat;
        fboFormat.setAttachment(QOpenGLFramebufferObject::CombinedDepthStencil);
        m_fbo = std::make_unique<QOpenGLFramebufferObject>(width, height, fboFormat);
        if (!m_fbo->isValid()) { if (error) *error = QStringLiteral("Could not create render framebuffer"); return false; }
    }
    if (softwareRendering) {
        // Qt's software scene-graph adaptation rejects QQuickRenderControl. A hidden
        // offscreen QQuickWindow keeps headless export functional on hosts with no EGL/GL.
        m_window = std::make_unique<QQuickWindow>();
    } else {
        m_renderControl = std::make_unique<QQuickRenderControl>();
        m_window = std::make_unique<QQuickWindow>(m_renderControl.get());
        m_window->setGraphicsDevice(QQuickGraphicsDevice::fromOpenGLContext(m_context.get()));
        QQuickRenderTarget target = QQuickRenderTarget::fromOpenGLTexture(m_fbo->texture(), {width, height});
        m_window->setRenderTarget(target);
    }
    m_window->setGeometry(0, 0, width, height);
    m_window->setColor(Qt::transparent);
    if (!softwareRendering && !m_renderControl->initialize()) {
        if (error) *error = QStringLiteral("Could not initialize Qt Quick render control");
        return false;
    }

    m_engine = std::make_unique<QQmlEngine>();
    m_engine->rootContext()->setContextProperty(QStringLiteral("comp"), &m_state);
    m_component = std::make_unique<QQmlComponent>(
        m_engine.get(), QUrl(QStringLiteral("qrc:/qt/qml/Omareel/Composition.qml")));
    if (m_component->status() != QQmlComponent::Ready) {
        if (error) *error = m_component->errorString();
        return false;
    }
    m_rootObject.reset(m_component->create());
    auto *rootItem = qobject_cast<QQuickItem *>(m_rootObject.get());
    if (!rootItem) { if (error) *error = QStringLiteral("Composition.qml did not create a QQuickItem"); return false; }
    rootItem->setParentItem(m_window->contentItem());
    rootItem->setSize(QSizeF(width, height));
    // Each picture goes to a FrameSource, found by the name Composition.qml gives it.
    m_frameSource = rootItem->findChild<QQuickItem *>(QStringLiteral("videoFrameSource"));
    if (!m_frameSource) { if (error) *error = QStringLiteral("Composition has no FrameSource"); return false; }
    m_cameraFrameSource = rootItem->findChild<QQuickItem *>(QStringLiteral("cameraFrameSource"));
    if (!m_cameraFrameSource) { if (error) *error = QStringLiteral("Composition has no camera FrameSource"); return false; }
    m_mediaFrameSource = rootItem->findChild<QQuickItem *>(QStringLiteral("mediaFrameSource"));
    if (!m_mediaFrameSource) { if (error) *error = QStringLiteral("Composition has no inserted-clip FrameSource"); return false; }

    m_frameBytes = qsizetype(width) * height * 4;
    if (!softwareRendering) {
        m_glExtra = m_context->extraFunctions();
        m_glExtra->initializeOpenGLFunctions();
        for (auto &slot : m_readbacks) {
            m_glExtra->glGenBuffers(1, &slot.buffer);
            m_glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, slot.buffer);
            m_glExtra->glBufferData(GL_PIXEL_PACK_BUFFER, m_frameBytes, nullptr, GL_STREAM_READ);
        }
        m_glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
    }
    return true;
}

void ExportRenderer::processEvents()
{
    QCoreApplication::processEvents();
    // Painting the editor while a queue is full can switch GL contexts.
    // Restore ours before touching its readbacks or framebuffer again.
    if (m_context) m_context->makeCurrent(m_surface.get());
}

QByteArray ExportRenderer::mapReadback(ReadbackSlot &slot, QString *error)
{
    QByteArray bytes(m_frameBytes, Qt::Uninitialized);
    m_glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, slot.buffer);
    const void *mapped = m_glExtra->glMapBufferRange(GL_PIXEL_PACK_BUFFER, 0, m_frameBytes,
                                                     GL_MAP_READ_BIT);
    if (!mapped) {
        if (error) *error = QStringLiteral("Could not map asynchronous frame readback");
        return QByteArray();
    }
    std::memcpy(bytes.data(), mapped, m_frameBytes);
    m_glExtra->glUnmapBuffer(GL_PIXEL_PACK_BUFFER);
    slot.frame = -1;
    return bytes;
}

bool ExportRenderer::renderFrame(const QImage &screen, const QImage &camera, const QImage &insert,
                                 QByteArray *pixels, QString *error)
{
    pixels->clear();
    const int frame = m_frame++;
    m_state.notifyChanged();
    QElapsedTimer stageTimer;
    stageTimer.start();
    // FrameSource::setImage, called through the meta-object.
    const auto upload = [](QQuickItem *source, const QImage &image) {
        QMetaObject::invokeMethod(source, "setImage", Qt::DirectConnection, Q_ARG(QImage, image));
    };
    if (!insert.isNull()) upload(m_mediaFrameSource, insert);
    if (!screen.isNull()) upload(m_frameSource, screen);
    if (!camera.isNull()) upload(m_cameraFrameSource, camera);
    processEvents();
    m_timing.uploadNs += stageTimer.nsecsElapsed();
    if (m_softwareRendering) {
        m_window->show();
        m_window->requestUpdate();
        QCoreApplication::processEvents();
        QImage rendered = m_window->grabWindow();
        rendered = rendered.convertToFormat(QImage::Format_RGBA8888);
        if (rendered.size() != QSize(m_width, m_height)) {
            if (error) *error = QStringLiteral("Could not grab the rendered frame");
            return false;
        }
        *pixels = QByteArray(reinterpret_cast<const char *>(rendered.constBits()),
                             rendered.sizeInBytes());
        return true;
    }
    m_fbo->bind();
    m_context->functions()->glViewport(0, 0, m_width, m_height);
    m_context->functions()->glClearColor(0, 0, 0, 0);
    m_context->functions()->glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT);
    stageTimer.restart();
    m_renderControl->beginFrame();
    m_renderControl->polishItems();
    m_timing.renderNs += stageTimer.nsecsElapsed();
    stageTimer.restart();
    m_renderControl->sync();
    m_timing.uploadNs += stageTimer.nsecsElapsed();
    stageTimer.restart();
    m_renderControl->render();
    m_renderControl->endFrame();
    m_timing.renderNs += stageTimer.nsecsElapsed();
    stageTimer.restart();
    // Two buffers: this frame's readback starts as the one two frames back is collected.
    auto &slot = m_readbacks[frame % int(m_readbacks.size())];
    if (slot.frame >= 0) {
        QString mapError;
        *pixels = mapReadback(slot, &mapError);
        if (!mapError.isEmpty()) {
            if (error) *error = mapError;
            return false;
        }
    }
    m_glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, slot.buffer);
    m_glExtra->glReadPixels(0, 0, m_width, m_height, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
    slot.frame = frame;
    m_glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
    m_timing.readbackNs += stageTimer.nsecsElapsed();
    return true;
}

bool ExportRenderer::takePendingFrame(QByteArray *pixels, QString *error)
{
    if (m_softwareRendering) return false;
    if (m_pendingFrame < 0) m_pendingFrame = std::max(0, m_frame - int(m_readbacks.size()));
    while (m_pendingFrame < m_frame) {
        const int frame = m_pendingFrame++;
        auto &slot = m_readbacks[frame % int(m_readbacks.size())];
        if (slot.frame != frame) continue;
        *pixels = mapReadback(slot, error);
        return true;
    }
    return false;
}

void ExportRenderer::releaseReadbacks()
{
    if (m_softwareRendering) return;
    m_glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
    for (auto &slot : m_readbacks)
        if (slot.buffer) m_glExtra->glDeleteBuffers(1, &slot.buffer);
}

void ExportRenderer::invalidate()
{
    if (m_renderControl) m_renderControl->invalidate();
}
