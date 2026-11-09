#pragma once

#include <QImage>
#include <QMutex>
#include <QQuickItem>
#include <QRectF>
#include <QVideoFrame>
#include <QtQml/qqmlregistration.h>
#include "YuvFrame.h"

namespace Omareel {

class FrameSource : public QQuickItem
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QRectF cropRect READ cropRect WRITE setCropRect NOTIFY cropRectChanged)
    Q_PROPERTY(qreal cornerRadiusRatio READ cornerRadiusRatio WRITE setCornerRadiusRatio NOTIFY cornerRadiusRatioChanged)
public:
    explicit FrameSource(QQuickItem *parent = nullptr);
    QRectF cropRect() const { return m_cropRect; }
    qreal cornerRadiusRatio() const { return m_cornerRadiusRatio; }

    void setCropRect(const QRectF &value);
    void setCornerRadiusRatio(qreal value);

    // The next picture as YUV, converted on the GPU (the export's recording): no RGB conversion
    // on the CPU, a third of the bytes to upload, and the same three textures every frame. Not
    // with a corner radius (software rendering), which paints the RGB image.
    // Called by name (ExportRenderer doesn't link this library).
    Q_INVOKABLE void setYuvFrame(const Omareel::YuvFrame &frame);

signals:
    void cropRectChanged();
    void cornerRadiusRatioChanged();

public slots:
    void setImage(const QImage &image);
    void setVideoFrame(const QVideoFrame &frame);

protected:
    QSGNode *updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *) override;

private:
    QSGNode *updateYuvNode(QSGNode *oldNode, const YuvFrame &frame, const QRectF &crop, bool changed);

    QMutex m_mutex;
    QImage m_image;
    YuvFrame m_yuv;
    QRectF m_cropRect{0.0, 0.0, 1.0, 1.0};
    qreal m_cornerRadiusRatio = 0.0;
    quint64 m_revision = 0;
    quint64 m_renderedRevision = 0;
};

} // namespace Omareel
