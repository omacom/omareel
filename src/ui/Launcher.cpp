#include "Launcher.h"

#include "core/OmarchyPaths.h"
#include "core/RecordingPreferences.h"
#include "record/Recorder.h"

#include <QCoreApplication>
#include <QCameraDevice>
#include <QDesktopServices>
#include <QDir>
#include <QFileInfo>
#include <QMediaDevices>
#include <QProcess>
#include <QSet>
#include <QUrl>
#include <ctime>

using namespace Omareel;

static qint64 monotonicUs()
{
    timespec value{};
    clock_gettime(CLOCK_MONOTONIC, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

Launcher::Launcher(QObject *parent): QObject(parent)
{
    const RecordingPreferences preferences = RecordingPreferences::load();
    m_systemAudio = preferences.systemAudio;
    m_microphone = preferences.microphone;
    m_microphoneDevice = preferences.microphoneDevice;
    m_webcam = preferences.webcamEnabled;
    m_webcamDevice = preferences.webcamDevice;
    m_webcamHeight = preferences.webcamHeight;
    m_webcamRotation = preferences.webcamRotation;
    m_webcamFlipHorizontal = preferences.webcamFlipHorizontal;
    m_selfViewEnabled = preferences.selfViewEnabled;
    m_selfViewSize = preferences.selfViewSize;
    refreshAudioDevices();
    refreshWebcamDevices();
    connect(&m_recordingTimer, &QTimer::timeout, this, &Launcher::refreshRecording);
    m_recordingTimer.start(1000);
    refreshRecording();
}

void Launcher::saveRecordingPreferences()
{
    RecordingPreferences preferences = RecordingPreferences::load();
    preferences.systemAudio = m_systemAudio;
    preferences.microphone = m_microphone;
    preferences.microphoneDevice = m_microphoneDevice;
    preferences.webcamEnabled = m_webcam;
    preferences.webcamDevice = m_webcamDevice;
    preferences.webcamHeight = m_webcamHeight;
    preferences.webcamRotation = m_webcamRotation;
    preferences.webcamFlipHorizontal = m_webcamFlipHorizontal;
    preferences.selfViewEnabled = m_selfViewEnabled;
    preferences.selfViewSize = m_selfViewSize;
    QString error;
    if (!preferences.save(&error)) emit errorOccurred(error);
}

void Launcher::setSystemAudio(bool value)
{
    if (m_systemAudio == value) return;
    m_systemAudio = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setMicrophone(bool value)
{
    if (m_microphone == value) return;
    m_microphone = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setMicrophoneDevice(const QString &value)
{
    if (value.isEmpty() || m_microphoneDevice == value) return;
    m_microphoneDevice = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setWebcam(bool value)
{
    if (m_webcam == value) return;
    m_webcam = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setWebcamDevice(const QString &value)
{
    if (value.isEmpty() || m_webcamDevice == value) return;
    m_webcamDevice = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setWebcamHeight(int value)
{
    value = value == 720 ? 720 : 1080;
    if (m_webcamHeight == value) return;
    m_webcamHeight = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setWebcamRotation(int value)
{
    value = ((value % 360) + 360) % 360;
    if (value != 0 && value != 90 && value != 180 && value != 270) value = 0;
    if (m_webcamRotation == value) return;
    m_webcamRotation = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setWebcamFlipHorizontal(bool value)
{
    if (m_webcamFlipHorizontal == value) return;
    m_webcamFlipHorizontal = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setSelfViewEnabled(bool value)
{
    if (m_selfViewEnabled == value) return;
    m_selfViewEnabled = value;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

void Launcher::setSelfViewSize(const QString &value)
{
    const QString normalized = value.toUpper();
    if (normalized != QLatin1String("S") && normalized != QLatin1String("M")
        && normalized != QLatin1String("L")) return;
    if (m_selfViewSize == normalized) return;
    m_selfViewSize = normalized;
    saveRecordingPreferences();
    emit recordingPreferencesChanged();
}

QVariant Launcher::webcamCameraDevice() const
{
    const QByteArray wanted = QFileInfo(m_webcamDevice).absoluteFilePath().toUtf8();
    for (const QCameraDevice &device : m_cameraDevices)
        if (device.id() == wanted) return QVariant::fromValue(device);
    for (const QCameraDevice &device : m_cameraDevices) {
        const QString id = QString::fromUtf8(device.id());
        if (id.contains(m_webcamDevice) || id.endsWith(QFileInfo(m_webcamDevice).fileName()))
            return QVariant::fromValue(device);
    }
    return {};
}

bool Launcher::webcamPreviewAvailable() const
{
    if (!qEnvironmentVariableIsEmpty("OMAREEL_SCREENSHOT")
        && qEnvironmentVariableIsEmpty("OMAREEL_SCREENSHOT_LIVE")) return false;
    return webcamCameraDevice().isValid();
}

void Launcher::refreshAudioDevices()
{
    QProcess process;
    process.start(QStringLiteral("gpu-screen-recorder"), {QStringLiteral("--list-audio-devices")});
    QVariantList devices;
    if (process.waitForFinished(5000) && process.exitCode() == 0) {
        const QStringList lines = QString::fromUtf8(process.readAllStandardOutput()).split('\n', Qt::SkipEmptyParts);
        for (const QString &line : lines) {
            const int separator = line.indexOf(QLatin1Char('|'));
            if (separator <= 0) continue;
            const QString id = line.left(separator).trimmed();
            if (id != QLatin1String("default_input") && !id.startsWith(QLatin1String("alsa_input."))) continue;
            devices << QVariantMap{{QStringLiteral("value"), id},
                                   {QStringLiteral("text"), line.mid(separator + 1).trimmed()}};
        }
    }
    if (devices.isEmpty())
        devices << QVariantMap{{QStringLiteral("value"), QStringLiteral("default_input")},
                               {QStringLiteral("text"), QStringLiteral("Default input")}};
    bool selectedFound = false;
    for (const QVariant &device : devices)
        selectedFound = selectedFound || device.toMap().value(QStringLiteral("value")) == m_microphoneDevice;
    if (!selectedFound) m_microphoneDevice = QStringLiteral("default_input");
    m_audioDevices = devices;
    emit audioDevicesChanged();
}

void Launcher::refreshWebcamDevices()
{
    QVariantList devices;
    QSet<QString> seen;
    m_cameraDevices = QMediaDevices::videoInputs();
    for (const QCameraDevice &camera : m_cameraDevices) {
        QString path = QString::fromUtf8(camera.id());
        if (!path.startsWith(QLatin1String("/dev/video"))) {
            const QString id = path;
            const qsizetype marker = id.indexOf(QLatin1String("/dev/video"));
            if (marker >= 0) path = id.mid(marker).section(QLatin1Char(' '), 0, 0);
        }
        if (!path.startsWith(QLatin1String("/dev/video")) || seen.contains(path)) continue;
        seen.insert(path);
        devices << QVariantMap{{QStringLiteral("value"), path},
            {QStringLiteral("text"), QStringLiteral("%1 (%2)").arg(camera.description(), path)}};
    }
    if (devices.isEmpty() && QFileInfo::exists(m_webcamDevice))
        devices << QVariantMap{{QStringLiteral("value"), m_webcamDevice},
                               {QStringLiteral("text"), m_webcamDevice}};
    bool selectedFound = false;
    for (const QVariant &device : devices)
        selectedFound = selectedFound || device.toMap().value(QStringLiteral("value")) == m_webcamDevice;
    if (!selectedFound && !devices.isEmpty()) m_webcamDevice = devices.first().toMap().value(QStringLiteral("value")).toString();
    m_webcamDevices = devices;
    emit webcamDevicesChanged();
}

void Launcher::refreshRecording()
{
    const bool active = Recorder::isRecording();
    if (active != m_recording) {
        m_recording = active;
        emit recordingChanged();
    }
    QString elapsed = QStringLiteral("00:00");
    if (active) {
        const qint64 startedUs = Recorder::recordingStartedUs();
        elapsed = formatDuration(std::max<qint64>(0, monotonicUs() - startedUs) / 1000000.0);
    }
    if (elapsed != m_recordingElapsed) {
        m_recordingElapsed = elapsed;
        emit recordingElapsedChanged();
    }
}

QString Launcher::formatDuration(double seconds)
{
    const int total = std::max(0, qRound(seconds));
    return QStringLiteral("%1:%2").arg(total / 60, 2, 10, QLatin1Char('0'))
                                      .arg(total % 60, 2, 10, QLatin1Char('0'));
}

void Launcher::openBundle(const QString &pathValue)
{
    const QUrl url(pathValue);
    const QString path = url.isLocalFile() ? url.toLocalFile() : pathValue;
    if (!QFileInfo(path).isDir()) {
        emit errorOccurred(QStringLiteral("Choose an .omareel or legacy .omarecord bundle directory"));
        return;
    }
    if (!QProcess::startDetached(QCoreApplication::applicationFilePath(), {QStringLiteral("edit"), path})) {
        emit errorOccurred(QStringLiteral("Could not launch editor"));
        return;
    }
    emit quitRequested();
}

void Launcher::showRecordingsFolder()
{
    const QString path = OmarchyPaths::recordingsDirectory();
    QDir().mkpath(path);
    if (!QDesktopServices::openUrl(QUrl::fromLocalFile(path)))
        emit errorOccurred(QStringLiteral("Could not open the recordings folder"));
}

void Launcher::record()
{
    if (m_recording || m_startingRecording) return;
    m_startingRecording = true;
    emit recordingStarting();
    QTimer::singleShot(150, this, [this] {
        QStringList arguments{QStringLiteral("record")};
        if (!m_systemAudio && !m_microphone) arguments << QStringLiteral("--no-audio");
        else {
            if (m_systemAudio) arguments << QStringLiteral("--with-desktop-audio");
            if (m_microphone) arguments << QStringLiteral("--with-microphone-audio")
                                        << QStringLiteral("--microphone-device") << m_microphoneDevice;
        }
        if (m_webcam) arguments << QStringLiteral("--with-webcam")
                                << QStringLiteral("--webcam-device") << m_webcamDevice
                                << QStringLiteral("--webcam-height") << QString::number(m_webcamHeight);
        if (!QProcess::startDetached(QCoreApplication::applicationFilePath(), arguments)) {
            m_startingRecording = false;
            emit errorOccurred(QStringLiteral("Could not start recording"));
            return;
        }
        emit quitRequested();
    });
}

void Launcher::stopRecording()
{
    QString error;
    if (!Recorder::signalExisting(false, &error)) emit errorOccurred(error);
}

void Launcher::cancelRecording()
{
    QString error;
    if (!Recorder::signalExisting(true, &error)) emit errorOccurred(error);
}
