#include "RecordingBarBase.h"

#include "CameraCapture.h"
#include "omareel_qt/src/recording_bar.cxxqt.h"

#include <QDebug>
#include <QSize>

using namespace Omareel;

RecordingBarBase::RecordingBarBase(QObject *parent) : QObject(parent)
{
    m_saveSelfViewTimer.setSingleShot(true);
    m_saveSelfViewTimer.setInterval(180);
    connect(&m_saveSelfViewTimer, &QTimer::timeout, this, [this] { timerFired(SaveSelfViewTimer); });
    m_positionWriteTimer.setSingleShot(true);
    m_positionWriteTimer.setInterval(34);
    connect(&m_positionWriteTimer, &QTimer::timeout, this, [this] { timerFired(PositionWriteTimer); });
    m_timer.setInterval(50);
    connect(&m_timer, &QTimer::timeout, this, [this] { timerFired(PollTimer); });
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, [this] { stateDirectoryChanged(); });
}

RecordingBarBase::~RecordingBarBase()
{
    // The Rust object is gone by now: the camera reports nothing more as it is torn down.
    if (m_cameraCapture) disconnect(m_cameraCapture.get(), nullptr, this, nullptr);
    m_watcher.disconnect(this);
    m_timer.disconnect(this);
    m_positionWriteTimer.disconnect(this);
    m_saveSelfViewTimer.disconnect(this);
}

QTimer &RecordingBarBase::timer(int timer)
{
    switch (timer) {
    case PositionWriteTimer: return m_positionWriteTimer;
    case SaveSelfViewTimer: return m_saveSelfViewTimer;
    default: return m_timer;
    }
}

void RecordingBarBase::startBarTimer(int timer)
{
    this->timer(timer).start();
}

void RecordingBarBase::stopBarTimer(int timer)
{
    this->timer(timer).stop();
}

bool RecordingBarBase::barTimerActive(int timer) const
{
    return const_cast<RecordingBarBase *>(this)->timer(timer).isActive();
}

void RecordingBarBase::callLater(int call)
{
    QTimer::singleShot(0, this, [this, call] { calledLater(call); });
}

void RecordingBarBase::watchDirectory(const QString &path)
{
    m_watcher.addPath(path);
}

bool RecordingBarBase::tryLockHost(const QString &path)
{
    m_hostLock = std::make_unique<QLockFile>(path);
    return m_hostLock->tryLock(0);
}

void RecordingBarBase::createCamera(const QString &devicePath, int requestedHeight, const QString &videoPath,
                                    const QString &timestampPath)
{
    m_cameraCapture = std::make_unique<CameraCapture>(devicePath, requestedHeight, videoPath, timestampPath, this);
    m_cameraCapture->setPreferredEncoderQuery([] { return omareel::record::cachedPreferredEncoderAvailable(); });
    connect(m_cameraCapture.get(), &CameraCapture::readyChanged, this, [this] { handleCameraReadyChanged(); });
    connect(m_cameraCapture.get(), &CameraCapture::errorOccurred, this,
            [this](const QString &message) { handleCameraError(message); });
    connect(m_cameraCapture.get(), &CameraCapture::stopped, this, [this] { handleCameraStopped(); });
}

void RecordingBarBase::destroyCamera()
{
    if (m_cameraCapture) disconnect(m_cameraCapture.get(), nullptr, this, nullptr);
    m_cameraCapture.reset();
}

void RecordingBarBase::attachCameraOutput(QObject *output)
{
    if (!m_cameraCapture) return;
    m_cameraOutput = output;
    m_cameraCapture->attachVideoOutput(output);
    m_cameraCapture->start();
}

void RecordingBarBase::reattachCameraOutput()
{
    attachCameraOutput(m_cameraOutput);
}

bool RecordingBarBase::cameraCaptureReady() const
{
    return m_cameraCapture && m_cameraCapture->ready();
}

int RecordingBarBase::cameraCaptureWidth() const
{
    return m_cameraCapture ? m_cameraCapture->captureSize().width() : QSize().width();
}

int RecordingBarBase::cameraCaptureHeight() const
{
    return m_cameraCapture ? m_cameraCapture->captureSize().height() : QSize().height();
}

double RecordingBarBase::cameraFrameRate() const
{
    return m_cameraCapture ? m_cameraCapture->frameRate() : 30.0;
}

void RecordingBarBase::setCameraRecordingOutput(const QString &videoPath, const QString &timestampPath)
{
    if (m_cameraCapture) m_cameraCapture->setRecordingOutput(videoPath, timestampPath);
}

void RecordingBarBase::beginCameraRecording()
{
    if (m_cameraCapture) m_cameraCapture->beginRecording();
}

void RecordingBarBase::stopCamera()
{
    if (m_cameraCapture) m_cameraCapture->stop();
}

void RecordingBarShims::logInfo(const QString &message)
{
    qInfo().noquote() << message;
}

QString RecordingBarShims::debugReal(double value)
{
    QString text;
    QDebug(&text).nospace() << value;
    return text;
}
