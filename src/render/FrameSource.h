#pragma once

#include <QImage>
#include <QMutex>
#include <QQuickItem>
#include <QVideoFrame>
#include <QtQml/qqmlregistration.h>

namespace OmaRecord {

class FrameSource : public QQuickItem
{
    Q_OBJECT
    QML_ELEMENT
public:
    explicit FrameSource(QQuickItem *parent = nullptr);

public slots:
    void setImage(const QImage &image);
    void setVideoFrame(const QVideoFrame &frame);

protected:
    QSGNode *updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *) override;

private:
    QMutex m_mutex;
    QImage m_image;
    quint64 m_revision = 0;
    quint64 m_renderedRevision = 0;
};

} // namespace OmaRecord
