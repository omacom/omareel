#pragma once

#include <QObject>
#include <QTimer>
#include <memory>

namespace OmaRecord {

class CameraCapture;

class RecordingBar : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString elapsed READ elapsed NOTIFY elapsedChanged)
    Q_PROPERTY(QString recordedMonitor READ recordedMonitor CONSTANT)
    Q_PROPERTY(bool webcam READ webcam CONSTANT)
    Q_PROPERTY(bool hidden READ hidden CONSTANT)
    Q_PROPERTY(int cameraRotation READ cameraRotation NOTIFY cameraSettingsChanged)
    Q_PROPERTY(bool cameraFlipHorizontal READ cameraFlipHorizontal NOTIFY cameraSettingsChanged)
    Q_PROPERTY(QString cameraError READ cameraError NOTIFY cameraErrorChanged)
public:
    explicit RecordingBar(bool hidden = false, QObject *parent = nullptr);
    ~RecordingBar() override;

    QString elapsed() const { return m_elapsed; }
    QString recordedMonitor() const { return m_recordedMonitor; }
    bool webcam() const { return m_webcam; }
    bool hidden() const { return m_hidden; }
    int cameraRotation() const { return m_cameraRotation; }
    bool cameraFlipHorizontal() const { return m_cameraFlipHorizontal; }
    QString cameraError() const { return m_cameraError; }

    Q_INVOKABLE void attachCameraOutput(QObject *output);
    Q_INVOKABLE void rotateCamera();
    Q_INVOKABLE void flipCamera();
    Q_INVOKABLE void stop();
    Q_INVOKABLE void cancel();

signals:
    void elapsedChanged();
    void cameraSettingsChanged();
    void cameraErrorChanged();
    void finished();

private:
    void poll();
    QString m_elapsed = QStringLiteral("00:00");
    QString m_recordedMonitor;
    bool m_webcam = false;
    bool m_hidden = false;
    int m_cameraRotation = 0;
    bool m_cameraFlipHorizontal = false;
    QString m_cameraError;
    std::unique_ptr<CameraCapture> m_cameraCapture;
    bool m_cameraStopRequested = false;
    bool m_cameraFailed = false;
    QTimer m_timer;
};

} // namespace OmaRecord
