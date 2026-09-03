#include "RecordingBar.h"

#include "record/Recorder.h"
#include "record/CameraCapture.h"
#include "core/RecordingPreferences.h"

#include <algorithm>
#include <ctime>

using namespace OmaRecord;

static qint64 barMonotonicUs()
{
    timespec value{};
    clock_gettime(CLOCK_MONOTONIC, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

RecordingBar::RecordingBar(bool hidden, QObject *parent): QObject(parent),
    m_recordedMonitor(Recorder::recordedMonitor()), m_webcam(Recorder::recordingHasWebcam()),
    m_hidden(hidden)
{
    const QJsonObject state = Recorder::recordingState();
    m_cameraRotation = state.value(QStringLiteral("camera_rotation")).toInt(0);
    m_cameraFlipHorizontal = state.value(QStringLiteral("camera_flip_horizontal")).toBool(false);
    if (m_webcam) {
        const QString bundle = state.value(QStringLiteral("bundle")).toString();
        m_cameraCapture = std::make_unique<CameraCapture>(
            state.value(QStringLiteral("camera_device")).toString(),
            state.value(QStringLiteral("camera_height")).toInt(1080),
            bundle + QStringLiteral("/camera.mp4"),
            bundle + QStringLiteral("/camera.mp4.ts"), this);
        connect(m_cameraCapture.get(), &CameraCapture::readyChanged, this, [this] {
            if (!m_cameraCapture->ready()) return;
            Recorder::updateRecordingState(QJsonObject{
                {QStringLiteral("camera_status"), QStringLiteral("ready")},
                {QStringLiteral("camera_backend"), QStringLiteral("qt-multimedia")},
                {QStringLiteral("camera_width"), m_cameraCapture->captureSize().width()},
                {QStringLiteral("camera_height_actual"), m_cameraCapture->captureSize().height()},
                {QStringLiteral("camera_fps"), m_cameraCapture->frameRate()}
            });
        });
        connect(m_cameraCapture.get(), &CameraCapture::errorOccurred, this,
                [this](const QString &message) {
            m_cameraFailed = true;
            m_cameraError = message;
            emit cameraErrorChanged();
            Recorder::updateRecordingState(QJsonObject{
                {QStringLiteral("camera_status"), QStringLiteral("releasing")},
                {QStringLiteral("camera_error"), message}
            });
        });
        connect(m_cameraCapture.get(), &CameraCapture::stopped, this, [this] {
            Recorder::updateRecordingState(QJsonObject{
                {QStringLiteral("camera_status"), m_cameraFailed
                    ? QStringLiteral("failed") : QStringLiteral("stopped")}
            });
        });
    }
    connect(&m_timer, &QTimer::timeout, this, &RecordingBar::poll);
    m_timer.start(500);
    poll();
}

RecordingBar::~RecordingBar() = default;

void RecordingBar::poll()
{
    if (!Recorder::isRecording()) {
        m_timer.stop();
        emit finished();
        return;
    }
    const QJsonObject state = Recorder::recordingState();
    if (m_cameraCapture && state.value(QStringLiteral("camera_stop")).toBool(false)
        && !m_cameraStopRequested) {
        m_cameraStopRequested = true;
        m_cameraCapture->stop();
    }
    const qint64 seconds = std::max<qint64>(0, barMonotonicUs() - Recorder::recordingStartedUs()) / 1000000;
    const QString elapsed = QStringLiteral("%1:%2").arg(seconds / 60, 2, 10, QLatin1Char('0'))
                                                   .arg(seconds % 60, 2, 10, QLatin1Char('0'));
    if (elapsed != m_elapsed) {
        m_elapsed = elapsed;
        emit elapsedChanged();
    }
}

void RecordingBar::attachCameraOutput(QObject *output)
{
    if (!m_cameraCapture) return;
    m_cameraCapture->attachVideoOutput(output);
    m_cameraCapture->start();
}

void RecordingBar::rotateCamera()
{
    m_cameraRotation = (m_cameraRotation + 90) % 360;
    RecordingPreferences preferences = RecordingPreferences::load();
    preferences.webcamRotation = m_cameraRotation;
    preferences.webcam.insert(QStringLiteral("rotation"), m_cameraRotation);
    preferences.save();
    Recorder::updateRecordingState(QJsonObject{{QStringLiteral("camera_rotation"), m_cameraRotation}});
    emit cameraSettingsChanged();
}

void RecordingBar::flipCamera()
{
    m_cameraFlipHorizontal = !m_cameraFlipHorizontal;
    RecordingPreferences preferences = RecordingPreferences::load();
    preferences.webcamFlipHorizontal = m_cameraFlipHorizontal;
    preferences.webcam.insert(QStringLiteral("flipHorizontal"), m_cameraFlipHorizontal);
    preferences.save();
    Recorder::updateRecordingState(QJsonObject{
        {QStringLiteral("camera_flip_horizontal"), m_cameraFlipHorizontal}
    });
    emit cameraSettingsChanged();
}

void RecordingBar::stop()
{
    Recorder::signalExisting(false);
}

void RecordingBar::cancel()
{
    Recorder::signalExisting(true);
}
