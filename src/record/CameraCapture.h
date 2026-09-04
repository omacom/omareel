#pragma once

#include <QObject>
#include <QSize>
#include <QString>
#include <memory>

class QCamera;
class QMediaCaptureSession;
class QMediaRecorder;
class QTimer;
class QVideoFrame;
class QVideoFrameInput;

namespace Omareel {

class FirstFrameTimestamp
{
public:
    explicit FirstFrameTimestamp(QString path = {});

    void setPath(const QString &path);
    bool recordFrameArrival(qint64 monotonicUs, qint64 realtimeUs);
    qint64 firstFrameUs() const { return m_firstFrameUs; }

private:
    QString m_path;
    qint64 m_firstFrameUs = 0;
};

class CameraCapture : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool ready READ ready NOTIFY readyChanged)
    Q_PROPERTY(bool running READ running NOTIFY runningChanged)
    Q_PROPERTY(QString errorString READ errorString NOTIFY errorOccurred)
    Q_PROPERTY(QSize captureSize READ captureSize NOTIFY captureSizeChanged)
    Q_PROPERTY(qreal frameRate READ frameRate NOTIFY captureSizeChanged)
public:
    CameraCapture(const QString &devicePath, int requestedHeight,
                  const QString &videoPath, const QString &timestampPath,
                  QObject *parent = nullptr);
    ~CameraCapture() override;

    bool ready() const { return m_ready; }
    bool running() const { return m_running; }
    QString errorString() const { return m_errorString; }
    QSize captureSize() const { return m_captureSize; }
    qreal frameRate() const { return m_frameRate; }

    Q_INVOKABLE void attachVideoOutput(QObject *output);
    Q_INVOKABLE void start();
    Q_INVOKABLE void beginRecording();
    Q_INVOKABLE void stop();

signals:
    void readyChanged();
    void runningChanged();
    void captureSizeChanged();
    void errorOccurred(const QString &message);
    void stopped();

private:
    void handleFrame(const QVideoFrame &frame);
    void sendLatestFrame();
    void fail(const QString &message);

    QString m_devicePath;
    int m_requestedHeight = 1080;
    QString m_videoPath;
    FirstFrameTimestamp m_timestamp;
    std::unique_ptr<QMediaCaptureSession> m_session;
    std::unique_ptr<QMediaCaptureSession> m_recordSession;
    std::unique_ptr<QCamera> m_camera;
    std::unique_ptr<QMediaRecorder> m_recorder;
    std::unique_ptr<QVideoFrameInput> m_frameInput;
    std::unique_ptr<QVideoFrame> m_latestFrame;
    std::unique_ptr<QTimer> m_startTimer;
    std::unique_ptr<QTimer> m_recordTimer;
    QSize m_captureSize;
    qreal m_frameRate = 30.0;
    QString m_errorString;
    bool m_ready = false;
    bool m_running = false;
    bool m_stopping = false;
    bool m_inputReady = false;
    bool m_recordingFrames = false;
    qint64 m_recordedFrameCount = 0;
};

} // namespace Omareel
