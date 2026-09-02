#include "FrameSource.h"

#include <QMutexLocker>
#include <QQuickWindow>
#include <QSGSimpleTextureNode>
#include <QPainter>
#include <QPainterPath>
#include <algorithm>

using namespace OmaRecord;

FrameSource::FrameSource(QQuickItem *parent): QQuickItem(parent)
{
    setFlag(ItemHasContents, true);
    m_image = QImage(1, 1, QImage::Format_RGBA8888);
    m_image.fill(Qt::black);
    ++m_revision;
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

void FrameSource::setCropRect(const QRectF &value)
{
    const QRectF normalized(std::clamp(value.x(), 0.0, 1.0),
                            std::clamp(value.y(), 0.0, 1.0),
                            std::clamp(value.width(), 0.0001, 1.0),
                            std::clamp(value.height(), 0.0001, 1.0));
    if (m_cropRect == normalized) return;
    {
        QMutexLocker lock(&m_mutex);
        m_cropRect = normalized;
        ++m_revision;
    }
    emit cropRectChanged();
    update();
}

void FrameSource::setCornerRadiusRatio(qreal value)
{
    value = std::clamp(value, 0.0, 0.5);
    if (qFuzzyCompare(m_cornerRadiusRatio, value)) return;
    {
        QMutexLocker lock(&m_mutex);
        m_cornerRadiusRatio = value;
        ++m_revision;
    }
    emit cornerRadiusRatioChanged();
    update();
}

QSGNode *FrameSource::updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *)
{
    auto *node = static_cast<QSGSimpleTextureNode *>(oldNode);
    if (!node) node = new QSGSimpleTextureNode;
    QImage image;
    QRectF crop;
    qreal radiusRatio;
    quint64 revision;
    {
        QMutexLocker lock(&m_mutex);
        image = m_image;
        crop = m_cropRect;
        radiusRatio = m_cornerRadiusRatio;
        revision = m_revision;
    }
    if (!image.isNull() && revision != m_renderedRevision && window()) {
        QRect pixelCrop(qRound(crop.x() * image.width()), qRound(crop.y() * image.height()),
                        qRound(crop.width() * image.width()), qRound(crop.height() * image.height()));
        pixelCrop = pixelCrop.intersected(image.rect());
        if (pixelCrop.isValid() && pixelCrop != image.rect()) image = image.copy(pixelCrop);
        if (radiusRatio > 0.0 && !image.isNull()) {
            QImage rounded(image.size(), QImage::Format_RGBA8888_Premultiplied);
            rounded.fill(Qt::transparent);
            QPainter painter(&rounded);
            painter.setRenderHint(QPainter::Antialiasing, true);
            const qreal radius = radiusRatio * std::min(image.width(), image.height());
            QPainterPath path;
            path.addRoundedRect(QRectF(0, 0, image.width(), image.height()), radius, radius);
            painter.setClipPath(path);
            painter.drawImage(0, 0, image);
            painter.end();
            image = std::move(rounded);
        }
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
