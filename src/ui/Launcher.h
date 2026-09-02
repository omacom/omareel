#pragma once

#include <QDateTime>
#include <QObject>
#include <QTimer>
#include <QVariantList>

namespace OmaRecord {

class Launcher : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantList recentBundles READ recentBundles NOTIFY recentBundlesChanged)
    Q_PROPERTY(bool recording READ recording NOTIFY recordingChanged)
    Q_PROPERTY(QString recordingElapsed READ recordingElapsed NOTIFY recordingElapsedChanged)
    Q_PROPERTY(bool systemAudio READ systemAudio WRITE setSystemAudio NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(bool microphone READ microphone WRITE setMicrophone NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(QString microphoneDevice READ microphoneDevice WRITE setMicrophoneDevice NOTIFY recordingPreferencesChanged)
    Q_PROPERTY(QVariantList audioDevices READ audioDevices NOTIFY audioDevicesChanged)
public:
    explicit Launcher(QObject *parent = nullptr);
    QVariantList recentBundles() const { return m_recentBundles; }
    bool recording() const { return m_recording; }
    QString recordingElapsed() const { return m_recordingElapsed; }
    bool systemAudio() const { return m_systemAudio; }
    bool microphone() const { return m_microphone; }
    QString microphoneDevice() const { return m_microphoneDevice; }
    QVariantList audioDevices() const { return m_audioDevices; }

    void setSystemAudio(bool value);
    void setMicrophone(bool value);
    void setMicrophoneDevice(const QString &value);

    Q_INVOKABLE void refresh();
    Q_INVOKABLE void openBundle(const QString &path);
    Q_INVOKABLE void record(const QString &mode);
    Q_INVOKABLE void stopRecording();
    Q_INVOKABLE void cancelRecording();

signals:
    void recentBundlesChanged();
    void recordingChanged();
    void recordingElapsedChanged();
    void recordingPreferencesChanged();
    void audioDevicesChanged();
    void quitRequested();
    void errorOccurred(const QString &message);

private:
    static QString formatDuration(double seconds);
    static QString formatDate(const QDateTime &dateTime);
    void probeDurationAsync(const QString &bundlePath);
    void refreshRecording();
    void saveRecordingPreferences();
    void refreshAudioDevices();
    QVariantList m_recentBundles;
    bool m_recording = false;
    QString m_recordingElapsed = QStringLiteral("00:00");
    QTimer m_recordingTimer;
    bool m_systemAudio = true;
    bool m_microphone = true;
    QString m_microphoneDevice = QStringLiteral("default_input");
    QVariantList m_audioDevices;
};

} // namespace OmaRecord
