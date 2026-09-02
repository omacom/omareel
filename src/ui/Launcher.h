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
public:
    explicit Launcher(QObject *parent = nullptr);
    QVariantList recentBundles() const { return m_recentBundles; }
    bool recording() const { return m_recording; }
    QString recordingElapsed() const { return m_recordingElapsed; }

    Q_INVOKABLE void refresh();
    Q_INVOKABLE void openBundle(const QString &path);
    Q_INVOKABLE void record(const QString &mode);
    Q_INVOKABLE void stopRecording();
    Q_INVOKABLE void cancelRecording();

signals:
    void recentBundlesChanged();
    void recordingChanged();
    void recordingElapsedChanged();
    void quitRequested();
    void errorOccurred(const QString &message);

private:
    static QString formatDuration(double seconds);
    static QString formatDate(const QDateTime &dateTime);
    void probeDurationAsync(const QString &bundlePath);
    void refreshRecording();
    QVariantList m_recentBundles;
    bool m_recording = false;
    QString m_recordingElapsed = QStringLiteral("00:00");
    QTimer m_recordingTimer;
};

} // namespace OmaRecord
