#pragma once

#include <QObject>
#include <QPointer>
#include <QVideoFrame>

class QVideoSink;

namespace Omareel {

class FrameSource;

class PreviewSink : public QObject
{
    Q_OBJECT
public:
    explicit PreviewSink(QVideoSink *sink, QObject *parent = nullptr);
    void setFrameSource(FrameSource *source);

private:
    QPointer<FrameSource> m_source;
    QVideoFrame m_lastFrame;
};

} // namespace Omareel
