#include "RecordingBar.h"

#include "record/Recorder.h"
#include "record/CameraCapture.h"
#include "core/RecordingPreferences.h"

#include <algorithm>
#include <ctime>
#include <QCursor>
#include <QDebug>

using namespace Omareel;

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
    m_selfViewVisible = state.value(QStringLiteral("selfview")).toBool(false);
    m_selfViewSafe = state.value(QStringLiteral("selfview_safe")).toBool(false);
    m_selfViewMonitor = state.value(QStringLiteral("selfview_monitor")).toString(m_recordedMonitor);
    m_selfViewPixels = state.value(QStringLiteral("selfview_pixels")).toInt(160);
    m_selfViewX = state.value(QStringLiteral("selfview_x")).toInt(16);
    m_selfViewY = state.value(QStringLiteral("selfview_y")).toInt(16);
    m_captureStarted = state.value(QStringLiteral("capture_started")).toBool(false);
    m_commandSequence = state.value(QStringLiteral("recording_bar_command_sequence"))
                            .toVariant().toLongLong();
    m_saveSelfViewTimer.setSingleShot(true);
    m_saveSelfViewTimer.setInterval(180);
    connect(&m_saveSelfViewTimer, &QTimer::timeout, this, [this] {
        if (m_selfViewScreenSize.isEmpty()) return;
        RecordingPreferences preferences = RecordingPreferences::load();
        const int xRange = std::max(1, m_selfViewScreenSize.width() - m_selfViewPixels - 32);
        const int yRange = std::max(1, m_selfViewScreenSize.height() - m_selfViewPixels - 32);
        preferences.selfViewX = std::clamp((m_selfViewX - 16.0) / xRange, 0.0, 1.0);
        preferences.selfViewY = std::clamp((m_selfViewY - 16.0) / yRange, 0.0, 1.0);
        preferences.save();
    });
    m_selfViewPlacementTimer.setSingleShot(true);
    m_selfViewPlacementTimer.setInterval(0);
    connect(&m_selfViewPlacementTimer, &QTimer::timeout,
            this, &RecordingBar::selfViewPlacementChanged);
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
    m_timer.start(50);
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
    const bool captureStarted = state.value(QStringLiteral("capture_started")).toBool(false);
    if (captureStarted != m_captureStarted) {
        m_captureStarted = captureStarted;
        emit captureStartedChanged();
    }
    const bool requestedSelfView = state.value(QStringLiteral("selfview")).toBool(false);
    if (requestedSelfView != m_selfViewVisible) {
        m_selfViewVisible = requestedSelfView;
        emit selfViewVisibilityChanged();
    }
    const int requestedX = state.value(QStringLiteral("selfview_x")).toInt(m_selfViewX);
    const int requestedY = state.value(QStringLiteral("selfview_y")).toInt(m_selfViewY);
    const int nextX = m_selfViewScreenSize.isEmpty() ? requestedX
        : std::clamp(requestedX, 0, std::max(0, m_selfViewScreenSize.width() - m_selfViewPixels));
    const int nextY = m_selfViewScreenSize.isEmpty() ? requestedY
        : std::clamp(requestedY, 0, std::max(0, m_selfViewScreenSize.height() - m_selfViewPixels));
    if (nextX != m_selfViewX || nextY != m_selfViewY) {
        m_selfViewX = nextX;
        m_selfViewY = nextY;
        scheduleSelfViewPlacementUpdate();
    }
    const qint64 commandSequence = state.value(QStringLiteral("recording_bar_command_sequence"))
                                       .toVariant().toLongLong();
    if (commandSequence != m_commandSequence) {
        m_commandSequence = commandSequence;
        const QString command = state.value(QStringLiteral("recording_bar_command")).toString();
        if (command == QLatin1String("rotate")) {
            rotateCamera();
        } else if (command == QLatin1String("flip")) {
            flipCamera();
        } else if (command == QLatin1String("drag-sim")) {
            beginDrag(QPointF(state.value(QStringLiteral("drag_x0")).toDouble(),
                              state.value(QStringLiteral("drag_y0")).toDouble()));
            dragTo(QPointF(state.value(QStringLiteral("drag_x1")).toDouble(),
                           state.value(QStringLiteral("drag_y1")).toDouble()));
            endDrag();
        }
    }
    if (m_cameraCapture && state.value(QStringLiteral("camera_record")).toBool(false)
        && !m_cameraRecordRequested) {
        m_cameraRecordRequested = true;
        m_cameraCapture->beginRecording();
    }
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
    if (qEnvironmentVariableIsSet("OMAREEL_CAMERA_SETTINGS_LOG"))
        qInfo().nospace() << "camera rotation=" << m_cameraRotation
                          << " flip=" << m_cameraFlipHorizontal;
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
    if (qEnvironmentVariableIsSet("OMAREEL_CAMERA_SETTINGS_LOG"))
        qInfo().nospace() << "camera rotation=" << m_cameraRotation
                          << " flip=" << m_cameraFlipHorizontal;
    emit cameraSettingsChanged();
}

QPointF RecordingBar::globalCursorPos() const
{
    return QPointF(QCursor::pos());
}

void RecordingBar::beginDrag(const QPointF &globalPosition)
{
    m_dragPressPointer = globalPosition;
    m_dragPressBubble = QPoint(m_selfViewX, m_selfViewY);
    m_dragActive = true;
}

void RecordingBar::dragTo(const QPointF &globalPosition)
{
    if (!m_dragActive || m_selfViewScreenSize.isEmpty()) return;
    const QPoint position = clampedSelfViewDragPosition(
        m_dragPressPointer, m_dragPressBubble, globalPosition,
        m_selfViewScreenSize, m_selfViewPixels);
    if (qEnvironmentVariableIsSet("OMAREEL_SELFVIEW_DRAG_LOG")) {
        const QPointF delta = globalPosition - m_dragPressPointer;
        qInfo().nospace() << "pointer=(" << globalPosition.x() << ',' << globalPosition.y()
                          << ") bubble=(" << position.x() << ',' << position.y()
                          << ") delta=(" << delta.x() << ',' << delta.y() << ')';
    }
    setSelfViewPosition(position.x(), position.y());
}

void RecordingBar::endDrag()
{
    m_dragActive = false;
}

void RecordingBar::setSelfViewScreenSize(const QSize &size)
{
    m_selfViewScreenSize = size;
    m_selfViewX = std::clamp(m_selfViewX, 0, std::max(0, size.width() - m_selfViewPixels));
    m_selfViewY = std::clamp(m_selfViewY, 0, std::max(0, size.height() - m_selfViewPixels));
    scheduleSelfViewPlacementUpdate();
}

void RecordingBar::moveSelfView(int deltaX, int deltaY)
{
    setSelfViewPosition(m_selfViewX + deltaX, m_selfViewY + deltaY);
}

void RecordingBar::setSelfViewPosition(int x, int y)
{
    if (m_selfViewScreenSize.isEmpty()) return;
    const int nextX = std::clamp(x, 0,
                                 std::max(0, m_selfViewScreenSize.width() - m_selfViewPixels));
    const int nextY = std::clamp(y, 0,
                                 std::max(0, m_selfViewScreenSize.height() - m_selfViewPixels));
    if (nextX == m_selfViewX && nextY == m_selfViewY) return;
    m_selfViewX = nextX;
    m_selfViewY = nextY;
    Recorder::updateRecordingState(QJsonObject{{QStringLiteral("selfview_x"), m_selfViewX},
                                                {QStringLiteral("selfview_y"), m_selfViewY}});
    m_saveSelfViewTimer.start();
    scheduleSelfViewPlacementUpdate();
}

void RecordingBar::scheduleSelfViewPlacementUpdate()
{
    if (!m_selfViewPlacementTimer.isActive()) m_selfViewPlacementTimer.start();
}

void RecordingBar::setSelfViewVisible(bool visible)
{
    if (!m_webcam || m_selfViewVisible == visible) return;
    m_selfViewVisible = visible;
    Recorder::updateRecordingState(QJsonObject{{QStringLiteral("selfview"), visible}});
    emit selfViewVisibilityChanged();
}

void RecordingBar::stop()
{
    Recorder::signalExisting(false);
}

void RecordingBar::cancel()
{
    Recorder::signalExisting(true);
}
