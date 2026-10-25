#pragma once

// QString first, as in LauncherBase.h.
#include <QString>

#include <QFileSystemWatcher>
#include <QLockFile>
#include <QObject>
#include <QPointer>
#include <QTimer>
#include <memory>

namespace Omareel {

class CameraCapture;

// The Qt side of the recording bar object, which is Rust (crates/omareel_qt/src/recording_bar.rs):
// it owns the camera (CameraCapture, src/plumbing) and the preview it draws into, the watcher on
// the recorder's state directory, the bar's timers and singleShot calls, and the standby host's
// lock, and hands what they report to the pure virtuals below, which the Rust object overrides.
// The camera is destroyed before the host lock is released, as RecordingBar's members were. It
// is compiled into the omareel_qt archive.
class RecordingBarBase : public QObject
{
    Q_OBJECT
public:
    // The poll (50 ms), the self-view position write (34 ms, single shot) and the self-view
    // position save (180 ms, single shot).
    enum Timer { PollTimer, PositionWriteTimer, SaveSelfViewTimer };
    // What the bar runs from the event loop, as QTimer::singleShot(0, ...) did.
    enum Call { FinishedCall, PollCall };

    explicit RecordingBarBase(QObject *parent = nullptr);
    ~RecordingBarBase() override;

    // Shows the camera in the VideoOutput's sink and starts it; nothing without a camera.
    Q_INVOKABLE void attachCameraOutput(QObject *output);

    virtual void timerFired(int timer) = 0;
    virtual void calledLater(int call) = 0;
    // The recorder's state directory changed.
    virtual void stateDirectoryChanged() = 0;
    // CameraCapture's readyChanged, errorOccurred and stopped.
    virtual void handleCameraReadyChanged() = 0;
    virtual void handleCameraError(const QString &message) = 0;
    virtual void handleCameraStopped() = 0;

protected:
    void startBarTimer(int timer);
    void stopBarTimer(int timer);
    bool barTimerActive(int timer) const;
    void callLater(int call);
    void watchDirectory(const QString &path);
    // QLockFile(path).tryLock(0), held until the bar goes.
    bool tryLockHost(const QString &path);

    // The camera, with the cached encoder probe deciding its encoder. Creating one replaces
    // nothing: destroyCamera first.
    void createCamera(const QString &devicePath, int requestedHeight, const QString &videoPath,
                      const QString &timestampPath);
    void destroyCamera();
    // attachCameraOutput with the output last attached (null if it is gone).
    void reattachCameraOutput();
    bool hasCamera() const { return bool(m_cameraCapture); }
    bool cameraCaptureReady() const;
    int cameraCaptureWidth() const;
    int cameraCaptureHeight() const;
    double cameraFrameRate() const;
    void setCameraRecordingOutput(const QString &videoPath, const QString &timestampPath);
    void beginCameraRecording();
    void stopCamera();

private:
    QTimer &timer(int timer);

    QFileSystemWatcher m_watcher;
    QTimer m_timer;
    QTimer m_positionWriteTimer;
    QTimer m_saveSelfViewTimer;
    std::unique_ptr<QLockFile> m_hostLock;
    QPointer<QObject> m_cameraOutput;
    // Declared last, so it is destroyed first.
    std::unique_ptr<CameraCapture> m_cameraCapture;
};

// Qt's message output, for the bar's logs.
namespace RecordingBarShims {
// qInfo().noquote() << message.
void logInfo(const QString &message);
// How QDebug writes a qreal.
QString debugReal(double value);
} // namespace RecordingBarShims

} // namespace Omareel
