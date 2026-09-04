#include "PreviewSink.h"

#include "FrameSource.h"

#include <QVideoSink>

using namespace Omareel;

PreviewSink::PreviewSink(QVideoSink *sink, QObject *parent): QObject(parent)
{
    connect(sink, &QVideoSink::videoFrameChanged, this, [this](const QVideoFrame &frame) {
        m_lastFrame = frame;
        if (m_source) m_source->setVideoFrame(frame);
    });
}

void PreviewSink::setFrameSource(FrameSource *source)
{
    m_source = source;
    if (m_source && m_lastFrame.isValid()) m_source->setVideoFrame(m_lastFrame);
}
