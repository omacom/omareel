#include "RecordingBar.h"

#include "record/Recorder.h"
#include "record/CameraCapture.h"
#include "core/RecordingPreferences.h"

#include <algorithm>
#include <ctime>
#include "record/CursorSampler.h"
#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QProcess>
#include <csignal>
#include <unistd.h>
#include <QDebug>
#include <QFile>

using namespace Omareel;

static qint64 barMonotonicUs()
{
    timespec value{};
    clock_gettime(CLOCK_MONOTONIC, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

static void debugBarStage(const QString &stage, const QString &detail = {})
{
    if (qEnvironmentVariable("OMAREEL_DEBUG") != QLatin1String("1")) return;
    QFile file(QStringLiteral("/tmp/omareel.log"));
    if (!file.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text)) return;
    QString line = QStringLiteral("OMAREEL_START stage=%1 monotonic_us=%2")
        .arg(stage).arg(barMonotonicUs());
    const qint64 daemonStartUs = Recorder::recordingState()
                                     .value(QStringLiteral("daemon_started_us"))
                                     .toVariant().toLongLong();
    if (daemonStartUs > 0)
        line += QStringLiteral(" elapsed_ms=%1")
                    .arg((barMonotonicUs() - daemonStartUs) / 1000.0, 0, 'f', 3);
    if (!detail.isEmpty()) line += QLatin1Char(' ') + detail;
    file.write((line + QLatin1Char('\n')).toUtf8());
}

RecordingBar::RecordingBar(bool hidden, bool standby, qint64 owner, QObject *parent): QObject(parent),
    m_standby(standby), m_owner(owner),
    m_recordedMonitor(Recorder::recordingState().value("overlay_monitor").toString(Recorder::recordedMonitor())), m_webcam(Recorder::recordingHasWebcam()),
    m_hidden(hidden)
{
    QJsonObject state = Recorder::recordingState();
    if (m_standby) {
        QDir().mkpath(QFileInfo(Recorder::selfViewHostPath()).absolutePath());
        m_hostLock = std::make_unique<QLockFile>(Recorder::selfViewHostPath() + ".host-lock");
        if (!m_hostLock->tryLock(0)) {
            QTimer::singleShot(0, this, &RecordingBar::finished);
            return;
        }
        const auto prefs = RecordingPreferences::load();
        QProcess monitors;
        monitors.start("hyprctl", {"-j", "monitors"});
        QJsonObject monitor;
        if (monitors.waitForFinished(1000)) {
            for (const auto &value : QJsonDocument::fromJson(monitors.readAllStandardOutput()).array()) {
                if (monitor.isEmpty() || value.toObject().value("focused").toBool()) monitor = value.toObject();
                if (value.toObject().value("focused").toBool()) break;
            }
        }
        const double scale = qMax(.01, monitor.value("scale").toDouble(1));
        const int pixels = prefs.selfViewSize == "S" ? 120 : prefs.selfViewSize == "L" ? 220 : 160;
        m_recordedMonitor = monitor.value("name").toString();
        m_webcam = true;
        state = {{"camera_rotation", prefs.webcamRotation}, {"camera_flip_horizontal", prefs.webcamFlipHorizontal},
                 {"selfview", prefs.selfViewEnabled}, {"selfview_safe", true}, {"selfview_monitor", m_recordedMonitor},
                 {"selfview_pixels", pixels}, {"camera_device", prefs.webcamDevice}, {"camera_height", prefs.webcamHeight},
                 {"selfview_x", 16 + qRound(prefs.selfViewX * qMax(0, qRound(monitor.value("width").toDouble()/scale) - pixels - 32))},
                 {"selfview_y", 16 + qRound(prefs.selfViewY * qMax(0, qRound(monitor.value("height").toDouble()/scale) - pixels - 32))}};
        Recorder::updateStateFile(Recorder::selfViewHostPath(), {{"command", ""}, {"command_sequence", 0}});
    }
    m_cameraRotation = state.value(QStringLiteral("camera_rotation")).toInt(0);
    m_cameraFlipHorizontal = state.value(QStringLiteral("camera_flip_horizontal")).toBool(false);
    m_selfViewVisible = state.value(QStringLiteral("selfview")).toBool(false);
    m_selfViewSafe = state.value(QStringLiteral("selfview_safe")).toBool(false);
    m_selfViewMonitor = state.value(QStringLiteral("selfview_monitor")).toString(m_recordedMonitor);
    m_selfViewPixels = state.value(QStringLiteral("selfview_pixels")).toInt(160);
    m_selfViewX = state.value(QStringLiteral("selfview_x")).toInt(16);
    m_selfViewY = state.value(QStringLiteral("selfview_y")).toInt(16);
    m_captureStarted = state.value(QStringLiteral("capture_started")).toBool(false);
    m_countdownActive = state.value(QStringLiteral("countdown_active")).toBool(false);
    m_commandSequence = state.value(QStringLiteral("recording_bar_command_sequence"))
                            .toVariant().toLongLong();
    m_saveSelfViewTimer.setSingleShot(true);
    m_saveSelfViewTimer.setInterval(180);
    connect(&m_saveSelfViewTimer, &QTimer::timeout, this, &RecordingBar::saveSelfViewPosition);
    if (m_webcam) createCamera(state);
    m_positionWriteTimer.setSingleShot(true);
    m_positionWriteTimer.setInterval(34);
    connect(&m_positionWriteTimer, &QTimer::timeout, this, &RecordingBar::publishGeometry);
    m_watcher.addPath(QFileInfo(Recorder::stateFilePath()).absolutePath());
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, &RecordingBar::poll);
    if (m_standby) publishHost();
    connect(&m_timer, &QTimer::timeout, this, &RecordingBar::poll);
    m_timer.start(50);
    poll();
}

RecordingBar::~RecordingBar()
{
    if (m_standby && m_hostLock && m_hostLock->isLocked()) QFile::remove(Recorder::selfViewHostPath());
}

void RecordingBar::poll()
{
    const bool active = Recorder::isRecording();
    QJsonObject state = Recorder::recordingState();
    if (m_standby) {
        const auto host = Recorder::readStateFile(Recorder::selfViewHostPath());
        const auto sequence = host.value("command_sequence").toVariant().toLongLong();
        if (sequence != m_hostSequence) {
            m_hostSequence = sequence;
            hostCommand(host.value("command").toString());
        }
        const bool ours = active && state.value("host_pid").toVariant().toLongLong() == QCoreApplication::applicationPid();
        if (ours && !m_adopted) {
            m_adopted = true;
            m_standbyMonitor = m_selfViewMonitor;
            m_standbyPosition = QPoint(m_selfViewX, m_selfViewY);
            m_recordedMonitor = state.value("overlay_monitor").toString();
            if (state.value("overlay_exclusion").toString() == "fallback") {
                m_selfViewMonitor = state.value("selfview_monitor").toString();
                m_selfViewSafe = state.value("selfview_safe").toBool();
                emit selfViewPlacementChanged();
                emit selfViewVisibilityChanged();
            }
            m_hidden = !state.value("bar_visible").toBool(true);
            const QString bundle = state.value("bundle").toString();
            m_cameraCapture->setRecordingOutput(bundle + "/camera.mp4", bundle + "/camera.mp4.ts");
            publishGeometry();
            Recorder::updateRecordingState({{"host_acknowledged", true}, {"countdown_ready", true},
                {"camera_rotation", m_cameraRotation}, {"camera_flip_horizontal", m_cameraFlipHorizontal},
                {"camera_status", m_cameraFailed ? "failed" : m_cameraCapture->ready() ? "ready" : "starting"},
                {"camera_width", m_cameraCapture->captureSize().width()},
                {"camera_height_actual", m_cameraCapture->captureSize().height()},
                {"camera_fps", m_cameraCapture->frameRate()}});
            state = Recorder::recordingState();
            emit captureStartedChanged();
        }
        const bool suppressed = active && !ours;
        if (suppressed != m_suppressed) {
            m_suppressed = suppressed;
            emit selfViewVisibilityChanged();
        }
        if (!active && m_adopted) {
            m_adopted = false;
            m_completedRecording = true;
            m_captureStarted = false;
            m_hidden = true;
            m_selfViewSafe = true;
            m_selfViewMonitor = m_standbyMonitor;
            m_selfViewX = m_standbyPosition.x();
            m_selfViewY = m_standbyPosition.y();
            m_cameraRecordRequested = m_cameraStopRequested = false;
            emit captureStartedChanged();
            emit selfViewPlacementChanged();
            emit selfViewVisibilityChanged();
            hostCommand("configure");
            publishHost();
        }
        if (!active && (m_quit || (!m_completedRecording && (m_owner <= 0 || ::kill(pid_t(m_owner), 0) != 0)))) {
            m_timer.stop();
            emit finished();
            return;
        }
        if (!ours) return;
    } else if (!active) {
        m_timer.stop();
        emit finished();
        return;
    }
    const QString barMonitor = state.value("overlay_monitor").toString(m_recordedMonitor);
    const bool hidden = !state.value("bar_visible").toBool(!m_hidden);
    if (m_recordedMonitor != barMonitor || m_hidden != hidden) {
        m_recordedMonitor = barMonitor;
        m_hidden = hidden;
        emit captureStartedChanged();
    }
    const bool safe = state.value("selfview_safe").toBool(m_selfViewSafe);
    const QString bubbleMonitor = state.value("selfview_monitor").toString(m_selfViewMonitor);
    if (safe != m_selfViewSafe || bubbleMonitor != m_selfViewMonitor) {
        m_selfViewSafe = safe;
        m_selfViewMonitor = bubbleMonitor;
        emit selfViewPlacementChanged();
        emit selfViewVisibilityChanged();
    }
    const bool captureStarted = state.value(QStringLiteral("capture_started")).toBool(false);
    if (captureStarted != m_captureStarted) {
        m_captureStarted = captureStarted;
        emit selfViewVisibilityChanged();
        emit captureStartedChanged();
    }
    if (captureStarted && qEnvironmentVariable("OMAREEL_DEBUG") == QLatin1String("1")
        && !property("captureStageLogged").toBool()) {
        setProperty("captureStageLogged", true);
        debugBarStage(QStringLiteral("capture_seen_by_bar"));
    }
    const bool countdownActive = state.value(QStringLiteral("countdown_active")).toBool(false);
    const qint64 countdownEndUs = state.value(QStringLiteral("countdown_end_us"))
                                      .toVariant().toLongLong();
    const int countdownValue = std::clamp(
        int((std::max<qint64>(0, countdownEndUs - barMonotonicUs()) + 999999) / 1000000), 1, 3);
    if (countdownActive != m_countdownActive || countdownValue != m_countdownValue) {
        m_countdownActive = countdownActive;
        m_countdownValue = countdownValue;
        emit countdownChanged();
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
    if (!m_dragActive && !m_positionWriteTimer.isActive()
        && (nextX != m_selfViewX || nextY != m_selfViewY)) {
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
        debugBarStage(QStringLiteral("camera_record_begin"));
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
    m_cameraOutput = output;
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
    if (!m_standby || m_adopted) Recorder::updateRecordingState({{"camera_rotation", m_cameraRotation}});
    publishHost();
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
    if (!m_standby || m_adopted) Recorder::updateRecordingState({{"camera_flip_horizontal", m_cameraFlipHorizontal}});
    publishHost();
    if (qEnvironmentVariableIsSet("OMAREEL_CAMERA_SETTINGS_LOG"))
        qInfo().nospace() << "camera rotation=" << m_cameraRotation
                          << " flip=" << m_cameraFlipHorizontal;
    emit cameraSettingsChanged();
}

QPointF RecordingBar::globalCursorPos() const
{
    QPointF point = m_dragPressPointer;
    CursorSampler::cursorPosition(&point);
    return point;
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
    m_positionWriteTimer.stop();
    publishGeometry();
    m_saveSelfViewTimer.stop();
    saveSelfViewPosition();
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
    if (!m_positionWriteTimer.isActive()) m_positionWriteTimer.start();
    m_saveSelfViewTimer.start();
    scheduleSelfViewPlacementUpdate();
}

void RecordingBar::scheduleSelfViewPlacementUpdate()
{
    emit selfViewPlacementChanged();
}

void RecordingBar::setSelfViewVisible(bool visible)
{
    if (!m_webcam || m_selfViewVisible == visible) return;
    m_selfViewVisible = visible;
    auto prefs = RecordingPreferences::load();
    prefs.selfViewEnabled = visible;
    prefs.save();
    publishGeometry();
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

void RecordingBar::publishGeometry()
{
    if (!m_standby || m_adopted)
        Recorder::updateRecordingState({{"selfview_monitor", m_selfViewMonitor},
            {"selfview_x", m_selfViewX}, {"selfview_y", m_selfViewY},
            {"selfview_pixels", m_selfViewPixels}, {"selfview", m_selfViewVisible}});
    publishHost();
}

void RecordingBar::publishHost()
{
    if (!m_standby) return;
    Recorder::updateStateFile(Recorder::selfViewHostPath(), {
        {"pid", QCoreApplication::applicationPid()}, {"owner_pid", m_owner},
        {"monitor", m_selfViewMonitor}, {"x", m_selfViewX}, {"y", m_selfViewY},
        {"pixels", m_selfViewPixels}, {"visible", m_selfViewVisible},
        {"rotation", m_cameraRotation}, {"flip_horizontal", m_cameraFlipHorizontal},
        {"camera_status", m_cameraFailed ? "failed" : m_cameraCapture && m_cameraCapture->ready() ? "ready" : "starting"},
        {"camera_error", m_cameraError}});
}

void RecordingBar::cameraState(const QJsonObject &values)
{
    if (!m_standby || m_adopted) Recorder::updateRecordingState(values);
    publishHost();
}

void RecordingBar::hostCommand(const QString &command)
{
    if (command == "quit") m_quit = true;
    else if (command == "configure" && !m_adopted) {
        const auto prefs = RecordingPreferences::load();
        if (m_cameraCapture) disconnect(m_cameraCapture.get(), nullptr, this, nullptr);
        m_cameraCapture.reset();
        m_cameraFailed = false;
        m_cameraError.clear();
        emit cameraErrorChanged();
        createCamera({{"camera_device", prefs.webcamDevice}, {"camera_height", prefs.webcamHeight}});
        attachCameraOutput(m_cameraOutput);
        publishHost();
    }
    else if (command == "rotate") rotateCamera();
    else if (command == "flip") flipCamera();
    else if (command == "show" || command == "hide") setSelfViewVisible(command == "show");
    else if (command == "S" || command == "M" || command == "L") {
        m_selfViewPixels = command == "S" ? 120 : command == "L" ? 220 : 160;
        auto prefs = RecordingPreferences::load();
        prefs.selfViewSize = command;
        prefs.save();
        setSelfViewScreenSize(m_selfViewScreenSize);
        publishGeometry();
    }
}

void RecordingBar::saveSelfViewPosition()
{
    if (m_adopted && Recorder::recordingState().value("overlay_exclusion").toString() == "fallback") return;
    if (m_selfViewScreenSize.isEmpty()) return;
    RecordingPreferences preferences = RecordingPreferences::load();
    const int xRange = std::max(1, m_selfViewScreenSize.width() - m_selfViewPixels - 32);
    const int yRange = std::max(1, m_selfViewScreenSize.height() - m_selfViewPixels - 32);
    preferences.selfViewX = std::clamp((m_selfViewX - 16.0) / xRange, 0.0, 1.0);
    preferences.selfViewY = std::clamp((m_selfViewY - 16.0) / yRange, 0.0, 1.0);
    preferences.save();
}

void RecordingBar::createCamera(const QJsonObject &state)
{
    const QString bundle = state.value(QStringLiteral("bundle")).toString();
    m_cameraCapture = std::make_unique<CameraCapture>(
        state.value(QStringLiteral("camera_device")).toString(),
        state.value(QStringLiteral("camera_height")).toInt(1080),
        bundle.isEmpty() ? QString() : bundle + QStringLiteral("/camera.mp4"),
        bundle.isEmpty() ? QString() : bundle + QStringLiteral("/camera.mp4.ts"), this);
    connect(m_cameraCapture.get(), &CameraCapture::readyChanged, this, [this] {
        if (!m_cameraCapture->ready()) return;
        cameraState(QJsonObject{
            {QStringLiteral("camera_status"), QStringLiteral("ready")},
            {QStringLiteral("camera_backend"), QStringLiteral("qt-multimedia")},
            {QStringLiteral("camera_width"), m_cameraCapture->captureSize().width()},
            {QStringLiteral("camera_height_actual"), m_cameraCapture->captureSize().height()},
            {QStringLiteral("camera_fps"), m_cameraCapture->frameRate()}
        });
        debugBarStage(QStringLiteral("camera_ready"));
    });
    connect(m_cameraCapture.get(), &CameraCapture::errorOccurred, this,
            [this](const QString &message) {
        m_cameraFailed = true;
        m_cameraError = message;
        emit cameraErrorChanged();
        cameraState(QJsonObject{
            {QStringLiteral("camera_status"), QStringLiteral("releasing")},
            {QStringLiteral("camera_error"), message}
        });
    });
    connect(m_cameraCapture.get(), &CameraCapture::stopped, this, [this] {
        cameraState(QJsonObject{
            {QStringLiteral("camera_status"), m_cameraFailed
                ? QStringLiteral("failed") : QStringLiteral("stopped")}
        });
    });
}
