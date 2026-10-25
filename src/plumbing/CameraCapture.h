#pragma once

#include <QObject>
#include <QByteArray>
#include <QPointer>
#include <QSize>
#include <QString>
#include <functional>
#include <memory>

#include <QList>

class QCamera;
class QCameraDevice;
class QMediaCaptureSession;
class QVideoSink;
class QTimer;
class QVideoFrame;

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

// Maps and repacks camera planes on the encoder worker, never on the UI thread.
struct CameraRawFrame {
    QByteArray pixels;
    QString pixelFormat;
    QSize size;
};
CameraRawFrame packCameraFrame(QVideoFrame frame);
qint64 cameraFrameDeadlineUs(qint64 firstFrameUs, qint64 frameNumber);
// The /dev/videoN node a saved camera path refers to now, or empty when it isn't plugged in.
QString cameraNodePath(const QString &path);
// The /dev/videoN node named in a Qt camera id, or empty.
QString cameraIdNode(const QByteArray &id);
QCameraDevice cameraDeviceForPath(const QList<QCameraDevice> &devices, const QString &path);
class CameraEncoder;

// The camera the recording bar shows and records: a QCamera into a video sink, the preview
// sink it is given, and an encoder thread that packs frames and pipes them to ffmpeg. Its
// owner (RecordingBarBase, crates/omareel_qt/cpp) tells it whether to encode with NVENC through
// setPreferredEncoderQuery, which the encoder thread asks when it starts ffmpeg; the answer is
// the screen recorder's cached encoder probe, which is Rust (omareel_record), and plumbing does
// not call Rust itself.
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

    // Whether the cached encoder probe found NVENC; asked on the encoder thread, so it must be
    // safe to call from any thread. Without one, libx264 is used.
    void setPreferredEncoderQuery(std::function<bool()> query) { m_preferredEncoderQuery = std::move(query); }
    Q_INVOKABLE void attachVideoOutput(QObject *output);
    Q_INVOKABLE void start();
    void setRecordingOutput(const QString &videoPath, const QString &timestampPath);
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
    void finishStop();
    void fail(const QString &message);

    QString m_devicePath;
    std::function<bool()> m_preferredEncoderQuery;
    int m_requestedHeight = 1080;
    QString m_videoPath;
    FirstFrameTimestamp m_timestamp;
    std::unique_ptr<QMediaCaptureSession> m_session;
    std::unique_ptr<QCamera> m_camera;
    std::unique_ptr<QVideoSink> m_sink;
    QPointer<QVideoSink> m_previewSink;
    std::unique_ptr<CameraEncoder> m_encoder;
    std::unique_ptr<QTimer> m_startTimer;
    QSize m_captureSize;
    qreal m_frameRate = 30.0;
    QString m_errorString;
    bool m_ready = false;
    bool m_running = false;
    bool m_stopping = false;
    bool m_recordingFrames = false;
    bool m_formatLogged = false;
};

} // namespace Omareel
