#include "LauncherBase.h"

#include "omareel_qt/src/launcher.cxxqt.h"

#include <QAudioDevice>
#include <QAudioFormat>
#include <QAudioSource>
#include <QCameraFormat>
#include <QCoreApplication>
#include <QDesktopServices>
#include <QGuiApplication>
#include <QIODevice>
#include <QProcess>
#include <QProcessEnvironment>
#include <QUrl>

using namespace Omareel;

LauncherBase::LauncherBase(QObject *parent) : QObject(parent)
{
    connect(&m_recordingTimer, &QTimer::timeout, this, [this] { recordingTimerFired(); });
    m_startTimer.setInterval(50);
    connect(&m_startTimer, &QTimer::timeout, this, [this] { startTimerFired(); });
    m_meterTimer.setInterval(50);
    connect(&m_meterTimer, &QTimer::timeout, this, &LauncherBase::readMicrophone);
}

LauncherBase::~LauncherBase()
{
    // The Rust object is gone by now: nothing reports to it while the rest is torn down.
    m_mediaDevices.disconnect(this);
    m_recordingTimer.disconnect(this);
    m_startTimer.disconnect(this);
    m_meterTimer.disconnect(this);
    stopMicrophone();
}

int LauncherBase::screenCount() const
{
    return int(QGuiApplication::screens().size());
}

void LauncherBase::refreshCameraDevices(CameraList &cameras)
{
    m_cameraDevices = QMediaDevices::videoInputs();
    for (const QCameraDevice &camera : std::as_const(m_cameraDevices)) {
        cameras.addCamera(camera.id(), camera.description());
        for (const QCameraFormat &format : camera.videoFormats())
            cameras.addFormat(format.resolution().width(), format.resolution().height(), format.minFrameRate(),
                              format.maxFrameRate());
    }
}

QVariant LauncherBase::cameraDeviceVariant(int index) const
{
    if (index < 0 || index >= m_cameraDevices.size()) return QVariant();
    return QVariant::fromValue(m_cameraDevices.at(index));
}

bool LauncherBase::listenToMicrophone(const QString &device)
{
    QAudioDevice input;
    if (!device.isEmpty()) {
        input = QMediaDevices::defaultAudioInput();
        const QList<QAudioDevice> inputs = QMediaDevices::audioInputs();
        for (const QAudioDevice &candidate : inputs)
            if (QString::fromUtf8(candidate.id()) == device) input = candidate;
    }
    if (m_source && m_source->state() != QAudio::StoppedState && input.id() == m_inputId) return false;
    stopMicrophone();
    m_inputId = input.id();
    if (input.isNull()) return true;
    m_source = std::make_unique<QAudioSource>(input, input.preferredFormat());
    m_input = m_source->start();
    if (m_input) m_meterTimer.start();
    return true;
}

void LauncherBase::stopMicrophone()
{
    m_meterTimer.stop();
    m_input = nullptr;
    if (m_source) m_source->stop();
    m_source.reset();
    m_inputId.clear();
}

void LauncherBase::readMicrophone()
{
    if (!m_input) return;
    microphoneRead(int(m_source->format().sampleFormat()), m_input->readAll());
}

void LauncherBase::watchMediaDevices()
{
    connect(&m_mediaDevices, &QMediaDevices::videoInputsChanged, this, [this] { handleVideoInputsChanged(); });
    connect(&m_mediaDevices, &QMediaDevices::audioInputsChanged, this, [this] { handleAudioInputsChanged(); });
}

void LauncherBase::startRecordingTimer()
{
    m_recordingTimer.start(50);
}

void LauncherBase::startStartTimer()
{
    m_startTimer.start();
}

void LauncherBase::stopStartTimer()
{
    m_startTimer.stop();
}

void LauncherBase::startDetachedSelfViewHost(std::int64_t &pid)
{
    QProcess process;
    process.setProgram(QCoreApplication::applicationFilePath());
    process.setArguments({QStringLiteral("__record-bar"), QStringLiteral("--standby"), QStringLiteral("--owner"),
                          QString::number(QCoreApplication::applicationPid())});
    auto environment = QProcessEnvironment::systemEnvironment();
    environment.remove(QStringLiteral("OMAREEL_SCREENSHOT"));
    environment.remove(QStringLiteral("OMAREEL_SCREENSHOT_LIVE"));
    process.setProcessEnvironment(environment);
    qint64 started = pid;
    process.startDetached(&started);
    pid = started;
}

QString LauncherShims::applicationDirPath()
{
    return QCoreApplication::applicationDirPath();
}

QString LauncherShims::applicationFilePath()
{
    return QCoreApplication::applicationFilePath();
}

QString LauncherShims::bundlePath(const QString &value)
{
    const QUrl url(value);
    return url.isLocalFile() ? url.toLocalFile() : value;
}

bool LauncherShims::startDetached(const QString &program, const QStringList &arguments)
{
    return QProcess::startDetached(program, arguments);
}

bool LauncherShims::openFolder(const QString &path)
{
    return QDesktopServices::openUrl(QUrl::fromLocalFile(path));
}
