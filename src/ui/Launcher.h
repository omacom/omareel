#pragma once

#include <QCameraDevice>
#include <QObject>
#include <QTimer>
#include <QVariantList>

namespace OmaRecord {

class Launcher : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool recording READ recording NOTIFY recordingChanged)
    Q_PROPERTY(QString recordingElapsed READ recordingElapsed NOTIFY recordingElapsedChanged)
    Q_PROPERTY(bool systemAudio READ systemAudio WRITE setSystemAudio NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(bool microphone READ microphone WRITE setMicrophone NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(QString microphoneDevice READ microphoneDevice WRITE setMicrophoneDevice NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(QVariantList audioDevices READ audioDevices NOTIFY audioDevicesChanged)
    Q_PROPERTY(bool webcam READ webcam WRITE setWebcam NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(QString webcamDevice READ webcamDevice WRITE setWebcamDevice NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(int webcamHeight READ webcamHeight WRITE setWebcamHeight NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(int webcamRotation READ webcamRotation WRITE setWebcamRotation NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(bool webcamFlipHorizontal READ webcamFlipHorizontal WRITE setWebcamFlipHorizontal NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(bool selfViewEnabled READ selfViewEnabled WRITE setSelfViewEnabled NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(QString selfViewSize READ selfViewSize WRITE setSelfViewSize NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(QVariant webcamCameraDevice READ webcamCameraDevice NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(bool webcamPreviewAvailable READ webcamPreviewAvailable NOTIFY webcamDevicesChanged)
    Q_PROPERTY(QVariantList webcamDevices READ webcamDevices NOTIFY webcamDevicesChanged)
public:
    explicit Launcher(QObject *parent = nullptr);
    bool recording() const { return m_recording; }
    QString recordingElapsed() const { return m_recordingElapsed; }
    bool systemAudio() const { return m_systemAudio; }
    bool microphone() const { return m_microphone; }
    QString microphoneDevice() const { return m_microphoneDevice; }
    QVariantList audioDevices() const { return m_audioDevices; }
    bool webcam() const { return m_webcam; }
    QString webcamDevice() const { return m_webcamDevice; }
    int webcamHeight() const { return m_webcamHeight; }
    int webcamRotation() const { return m_webcamRotation; }
    bool webcamFlipHorizontal() const { return m_webcamFlipHorizontal; }
    bool selfViewEnabled() const { return m_selfViewEnabled; }
    QString selfViewSize() const { return m_selfViewSize; }
    QVariant webcamCameraDevice() const;
    bool webcamPreviewAvailable() const;
    QVariantList webcamDevices() const { return m_webcamDevices; }

    void setSystemAudio(bool value);
    void setMicrophone(bool value);
    void setMicrophoneDevice(const QString &value);
    void setWebcam(bool value);
    void setWebcamDevice(const QString &value);
    void setWebcamHeight(int value);
    void setWebcamRotation(int value);
    void setWebcamFlipHorizontal(bool value);
    void setSelfViewEnabled(bool value);
    void setSelfViewSize(const QString &value);

    Q_INVOKABLE void openBundle(const QString &path);
    Q_INVOKABLE void showRecordingsFolder();
    Q_INVOKABLE void record();
    Q_INVOKABLE void stopRecording();
    Q_INVOKABLE void cancelRecording();

signals:
    void recordingChanged();
    void recordingElapsedChanged();
    void recordingPreferencesChanged();
    void audioDevicesChanged();
    void webcamDevicesChanged();
    void quitRequested();
    void errorOccurred(const QString &message);
    void recordingStarting();

private:
    static QString formatDuration(double seconds);
    void refreshRecording();
    void saveRecordingPreferences();
    void refreshAudioDevices();
    void refreshWebcamDevices();
    bool m_recording = false;
    QString m_recordingElapsed = QStringLiteral("00:00");
    QTimer m_recordingTimer;
    bool m_systemAudio = true;
    bool m_microphone = true;
    QString m_microphoneDevice = QStringLiteral("default_input");
    QVariantList m_audioDevices;
    bool m_webcam = false;
    QString m_webcamDevice = QStringLiteral("/dev/video2");
    int m_webcamHeight = 1080;
    int m_webcamRotation = 0;
    bool m_webcamFlipHorizontal = false;
    bool m_selfViewEnabled = true;
    QString m_selfViewSize = QStringLiteral("M");
    QVariantList m_webcamDevices;
    QList<QCameraDevice> m_cameraDevices;
    bool m_startingRecording = false;
};

} // namespace OmaRecord
