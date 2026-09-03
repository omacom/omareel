#pragma once

#include <QObject>
#include <QTimer>

namespace OmaRecord {

class RecordingBar : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString elapsed READ elapsed NOTIFY elapsedChanged)
    Q_PROPERTY(QString recordedMonitor READ recordedMonitor CONSTANT)
    Q_PROPERTY(bool webcam READ webcam CONSTANT)
public:
    explicit RecordingBar(QObject *parent = nullptr);

    QString elapsed() const { return m_elapsed; }
    QString recordedMonitor() const { return m_recordedMonitor; }
    bool webcam() const { return m_webcam; }

    Q_INVOKABLE void stop();
    Q_INVOKABLE void cancel();

signals:
    void elapsedChanged();
    void finished();

private:
    void poll();
    QString m_elapsed = QStringLiteral("00:00");
    QString m_recordedMonitor;
    bool m_webcam = false;
    QTimer m_timer;
};

} // namespace OmaRecord
