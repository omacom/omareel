#pragma once

// QString first: a Qt header that only declares QChar, compiled with the cc crate's -Wall, makes
// GCC 16 warn when QChar is completed later (see QtShims.h in export.rs).
#include <QString>

#include <QByteArray>
#include <QCameraDevice>
#include <QList>
#include <QMediaDevices>
#include <QObject>
#include <QStringList>
#include <QTimer>
#include <QVariant>
#include <cstdint>
#include <memory>

class QAudioSource;
class QIODevice;

namespace Omareel {

// The Rust collector of the cameras Qt lists (crates/omareel_qt/src/launcher.rs).
struct CameraList;

// The Qt side of the launcher object, which is Rust (crates/omareel_qt/src/launcher.rs): it owns
// the QMediaDevices whose changes refresh the camera and microphone lists, the cameras Qt lists
// (so the webcamCameraDevice property can hold one), the QAudioSource the microphone meter listens
// to, and the launcher's timers, and hands what they report to the pure virtuals below, which the
// Rust object overrides. It is compiled into the omareel_qt archive.
class LauncherBase : public QObject
{
    Q_OBJECT
public:
    explicit LauncherBase(QObject *parent = nullptr);
    ~LauncherBase() override;

    // QMediaDevices::videoInputsChanged and audioInputsChanged.
    virtual void handleVideoInputsChanged() = 0;
    virtual void handleAudioInputsChanged() = 0;
    // The 50 ms recording poll, and record()'s 50 ms start timer.
    virtual void recordingTimerFired() = 0;
    virtual void startTimerFired() = 0;
    // What the microphone meter read every 50 ms, with its QAudioFormat::SampleFormat.
    virtual void microphoneRead(int sampleFormat, const QByteArray &samples) = 0;

protected:
    // QGuiApplication::screens().size().
    int screenCount() const;
    // QMediaDevices::videoInputs(), kept for cameraDeviceVariant and handed to the list as plain
    // data in the same order.
    void refreshCameraDevices(CameraList &cameras);
    // The camera of the last refresh at that index in a QVariant, or an invalid QVariant.
    QVariant cameraDeviceVariant(int index) const;
    // MicrophoneMeter::setDevice: a PulseAudio source name, as the recorder takes it, or
    // default_input; empty stops listening. Opens the device again when it has changed or the
    // previous one stopped. True when the meter stopped first, which drops the level to 0.
    bool listenToMicrophone(const QString &device);
    // Connects QMediaDevices' change signals, which also starts Qt Multimedia's device monitoring
    // (QMediaDevices::connectNotify); the C++ launcher connected after its first device refresh.
    void watchMediaDevices();
    void startRecordingTimer();
    void startStartTimer();
    void stopStartTimer();
    // `<program> __record-bar --standby --owner <pid>` started detached, without the screenshot
    // variables; pid is what QProcess::startDetached wrote (-1 when it failed).
    void startDetachedSelfViewHost(std::int64_t &pid);

private:
    void stopMicrophone();
    void readMicrophone();

    QMediaDevices m_mediaDevices;
    QList<QCameraDevice> m_cameraDevices;
    QTimer m_recordingTimer;
    QTimer m_startTimer;
    std::unique_ptr<QAudioSource> m_source;
    QIODevice *m_input = nullptr;
    QByteArray m_inputId;
    QTimer m_meterTimer;
};

// Qt calls the launcher makes, as free functions Rust can call.
namespace LauncherShims {
QString applicationDirPath();
QString applicationFilePath();
// QUrl(value).isLocalFile() ? toLocalFile() : value, as openBundle took a path or a URL.
QString bundlePath(const QString &value);
// QProcess::startDetached(program, arguments).
bool startDetached(const QString &program, const QStringList &arguments);
// QDesktopServices::openUrl(QUrl::fromLocalFile(path)).
bool openFolder(const QString &path);
} // namespace LauncherShims

} // namespace Omareel
