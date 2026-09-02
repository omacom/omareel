#include "FrameSource.h"

#include <QMutexLocker>
#include <QQuickWindow>
#include <QSGSimpleTextureNode>

using namespace OmaRecord;

FrameSource::FrameSource(QQuickItem *parent): QQuickItem(parent)
{
    setFlag(ItemHasContents, true);
}

void FrameSource::setImage(const QImage &image)
{
    {
        QMutexLocker lock(&m_mutex);
        m_image = image;
        ++m_revision;
    }
    update();
}

void FrameSource::setVideoFrame(const QVideoFrame &frame)
{
    // QVideoFrame::toImage() may perform a GPU readback. Preview can later gain a native
    // texture path; the copy keeps this first implementation correct and deterministic.
    setImage(frame.toImage());
}

QSGNode *FrameSource::updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *)
{
    auto *node = static_cast<QSGSimpleTextureNode *>(oldNode);
    if (!node) node = new QSGSimpleTextureNode;
    QImage image;
    quint64 revision;
    {
        QMutexLocker lock(&m_mutex);
        image = m_image;
        revision = m_revision;
    }
    if (!image.isNull() && revision != m_renderedRevision && window()) {
        QSGTexture *texture = window()->createTextureFromImage(image);
        QSGTexture *oldTexture = node->texture();
        node->setOwnsTexture(false);
        node->setTexture(texture);
        delete oldTexture;
        node->setOwnsTexture(true);
        node->setFiltering(QSGTexture::Linear);
        m_renderedRevision = revision;
    }
    node->setRect(boundingRect());
    return node;
}
