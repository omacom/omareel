#include "RecordingBar.h"

#include "record/Recorder.h"

#include <algorithm>
#include <ctime>

using namespace OmaRecord;

static qint64 barMonotonicUs()
{
    timespec value{};
    clock_gettime(CLOCK_MONOTONIC, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

RecordingBar::RecordingBar(QObject *parent): QObject(parent),
    m_recordedMonitor(Recorder::recordedMonitor()), m_webcam(Recorder::recordingHasWebcam())
{
    connect(&m_timer, &QTimer::timeout, this, &RecordingBar::poll);
    m_timer.start(500);
    poll();
}

void RecordingBar::poll()
{
    if (!Recorder::isRecording()) {
        m_timer.stop();
        emit finished();
        return;
    }
    const qint64 seconds = std::max<qint64>(0, barMonotonicUs() - Recorder::recordingStartedUs()) / 1000000;
    const QString elapsed = QStringLiteral("%1:%2").arg(seconds / 60, 2, 10, QLatin1Char('0'))
                                                   .arg(seconds % 60, 2, 10, QLatin1Char('0'));
    if (elapsed != m_elapsed) {
        m_elapsed = elapsed;
        emit elapsedChanged();
    }
}

void RecordingBar::stop()
{
    Recorder::signalExisting(false);
}

void RecordingBar::cancel()
{
    Recorder::signalExisting(true);
}
