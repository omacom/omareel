#pragma once

#include <QObject>
#include <QByteArray>
#include <QMutex>
#include <QPointer>
#include <QSize>
#include <QString>
#include <QStringList>
#include <QThread>
#include <QVideoFrame>
#include <QWaitCondition>
#include <atomic>
#include <deque>
#include <functional>
#include <memory>

#include <QList>

class QCamera;
class QCameraDevice;
class QMediaCaptureSession;
class QVideoSink;
class QTimer;

namespace Omareel {

// Writes the first recorded frame's CLOCK_MONOTONIC time, once, to the .ts file beside
// camera.mp4. The recorder offsets the camera track from the screen's first frame by it, and
// the frame at that time is camera.mp4's time zero.
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

// When each camera frame was captured, in CLOCK_MONOTONIC microseconds. Qt's V4L2 camera puts
// the driver's buffer timestamp in QVideoFrame::startTime(), but relative to the stream's first
// frame; uvcvideo takes it from CLOCK_MONOTONIC as the frame starts to arrive. Arrival in the
// sink adds the transfer and however long the UI thread was busy, so the clock keeps the
// driver's spacing and anchors it at the least delay seen (arrival minus start time). Without
// a usable start time (-1, or one that doesn't advance) a frame keeps its arrival time. Times
// never exceed arrival and always increase.
class CameraFrameClock
{
public:
    qint64 frameTimeUs(qint64 startTimeUs, qint64 arrivalUs);

private:
    qint64 m_offsetUs = 0;
    bool m_hasOffset = false;
    qint64 m_lastStartUs = -1;
    qint64 m_lastFrameUs = 0;
    // The least delay over the current window of driver time; when a whole window stayed far
    // above the anchor, the driver's clock is not the monotonic one (or jumped) and it moves.
    qint64 m_windowStartUs = 0;
    qint64 m_windowOffsetUs = 0;
};

// Maps and repacks camera planes on the encoder worker, never on the UI thread. colorMatrix
// and fullRange describe YUV pixels (ffmpeg's in_color_matrix and in_range names).
struct CameraRawFrame {
    QByteArray pixels;
    QString pixelFormat;
    QSize size;
    QString colorMatrix = QStringLiteral("bt601");
    bool fullRange = false;
};
CameraRawFrame packCameraFrame(QVideoFrame frame);

// The camera's raw frames reach ffmpeg as uncompressed Matroska, as the screen's do
// (screen_capture's capture_stream_header), so each keeps its capture time. The header is
// empty for a pixel format Matroska has no FourCC for.
QByteArray cameraStreamHeader(const CameraRawFrame &format);
QByteArray cameraFrameHeader(qint64 timestampUs, qint64 frameBytes);
// ffmpeg's arguments for the camera recording: frames at their own times (VFR), converted to
// BT.709 limited-range 4:2:0 and tagged so, in fragmented MP4 that stays playable up to the last
// fragment if the recorder dies.
QStringList cameraEncoderArguments(const CameraRawFrame &format, bool nvenc, const QString &path);

// Packs queued frames on its own thread and pipes them, with their times, to ffmpeg. Frames
// are only dropped when more than a second's worth waits for the encoder.
class CameraEncoder : public QThread
{
public:
    CameraEncoder(QString path, std::function<bool()> preferredEncoderQuery);
    ~CameraEncoder() override;

    // Queues a frame captured at timeUs (CLOCK_MONOTONIC); the first frame queued is time zero.
    // False when the frame was dropped.
    bool submit(const QVideoFrame &frame, qint64 timeUs);
    // Encodes what is queued and closes the file, within four and a half seconds.
    void requestStop();
    QString error() const;
    qint64 encodedFrames() const { return m_encoded.load(); }
    qint64 droppedFrames() const { return m_dropped.load(); }

protected:
    void run() override;

private:
    struct Queued {
        QVideoFrame frame;
        qint64 timestampUs = 0;
    };
    QString m_path;
    std::function<bool()> m_preferredEncoderQuery;
    mutable QMutex m_mutex;
    QWaitCondition m_queued;
    std::deque<Queued> m_queue;
    QString m_error;
    qint64 m_firstUs = -1;
    std::atomic<qint64> m_stopDeadline{0};
    std::atomic<qint64> m_encoded{0};
    std::atomic<qint64> m_dropped{0};
};
// The /dev/videoN node a saved camera path refers to now, or empty when it isn't plugged in.
QString cameraNodePath(const QString &path);
// The /dev/videoN node named in a Qt camera id, or empty.
QString cameraIdNode(const QByteArray &id);
QCameraDevice cameraDeviceForPath(const QList<QCameraDevice> &devices, const QString &path);

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
    CameraFrameClock m_clock;
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
