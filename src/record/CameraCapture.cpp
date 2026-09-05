#include "CameraCapture.h"
#include "ScreenCapture.h"

#include <QCamera>
#include <QCameraDevice>
#include <QCameraFormat>
#include <QDateTime>
#include <QFileInfo>
#include <QMediaCaptureSession>
#include <QMediaDevices>
#include <QSaveFile>
#include <QTimer>
#include <QVideoFrame>
#include <QVideoSink>
#include <QDebug>
#include <QTextStream>
#include <QProcess>
#include <QThread>
#include <QMutex>
#include <QMutexLocker>
#include <atomic>
#include <cstring>
#include <turbojpeg.h>
#include <algorithm>
#include <cmath>
#include <ctime>
#include <limits>

using namespace Omareel;

namespace {

qint64 monotonicUs()
{
    timespec value{};
    clock_gettime(CLOCK_MONOTONIC, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

QCameraDevice selectedDevice(const QString &path)
{
    const QByteArray pathBytes = QFileInfo(path).absoluteFilePath().toUtf8();
    const QString baseName = QFileInfo(path).fileName();
    const QList<QCameraDevice> devices = QMediaDevices::videoInputs();
    for (const QCameraDevice &device : devices)
        if (device.id() == pathBytes) return device;
    for (const QCameraDevice &device : devices) {
        const QString id = QString::fromUtf8(device.id());
        if (id.contains(path) || id.endsWith(baseName)
            || device.description().contains(path, Qt::CaseInsensitive)
            || device.description().contains(baseName, Qt::CaseInsensitive))
            return device;
    }
    return {};
}

QCameraFormat selectedFormat(const QCameraDevice &device, int requestedHeight)
{
    QCameraFormat best;
    double bestScore = std::numeric_limits<double>::max();
    const QSize requested(requestedHeight == 720 ? 1280 : 1920,
                          requestedHeight == 720 ? 720 : 1080);
    for (const QCameraFormat &format : device.videoFormats()) {
        if (format.maxFrameRate() + 0.01 < 30.0) continue;
        const QSize size = format.resolution();
        const double sizePenalty = std::abs(size.width() - requested.width())
            + std::abs(size.height() - requested.height());
        const double frameRatePenalty = std::abs(format.maxFrameRate() - 30.0f) * 100.0;
        const double score = sizePenalty + frameRatePenalty;
        if (score < bestScore) {
            best = format;
            bestScore = score;
        }
    }
    return best;
}

} // namespace

FirstFrameTimestamp::FirstFrameTimestamp(QString path): m_path(std::move(path))
{
}

void FirstFrameTimestamp::setPath(const QString &path)
{
    m_path = path;
    m_firstFrameUs = 0;
}

bool FirstFrameTimestamp::recordFrameArrival(qint64 monotonicValue, qint64 realtimeUs)
{
    if (m_firstFrameUs > 0) return true;
    if (m_path.isEmpty() || monotonicValue <= 0 || realtimeUs <= 0) return false;
    QSaveFile file(m_path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) return false;
    file.write("monotonic_microsec\trealtime_microsec\n");
    file.write(QByteArray::number(monotonicValue) + '\t' + QByteArray::number(realtimeUs) + '\n');
    if (!file.commit()) return false;
    m_firstFrameUs = monotonicValue;
    return true;
}

qint64 Omareel::cameraFrameDeadlineUs(qint64 firstFrameUs, qint64 frameNumber)
{
    return firstFrameUs + frameNumber * 1000000 / 30;
}

CameraRawFrame Omareel::packCameraFrame(QVideoFrame frame)
{
    CameraRawFrame result;
    if (!frame.map(QVideoFrame::ReadOnly)) return result;
    const int w = frame.width(), h = frame.height();
    const int cw = (w + 1) / 2, ch = (h + 1) / 2;
    if (frame.pixelFormat() == QVideoFrameFormat::Format_Jpeg) {
        // V4L2 MJPG arrives compressed. Decode directly to its native YUV planes,
        // avoiding both QImage's RGB decode and a subsequent RGB-to-YUV conversion.
        struct Decoder {
            tjhandle handle = tjInitDecompress();
            ~Decoder() { if (handle) tjDestroy(handle); }
        };
        thread_local Decoder decoder;
        int width = 0, height = 0, subsampling = 0, colorspace = 0;
        const auto *jpeg = frame.bits(0);
        const auto bytes = frame.mappedBytes(0);
        bool valid = decoder.handle && jpeg && bytes > 0
            && tjDecompressHeader3(decoder.handle, jpeg, bytes, &width, &height,
                                   &subsampling, &colorspace) == 0
            && width == w && height == h && w % 2 == 0 && h % 2 == 0;
        int chromaWidth = cw, chromaHeight = ch;
        if (subsampling == TJSAMP_420) result.pixelFormat = "yuvj420p";
        else if (subsampling == TJSAMP_422) { result.pixelFormat = "yuvj422p"; chromaHeight = h; }
        else if (subsampling == TJSAMP_444) { result.pixelFormat = "yuvj444p"; chromaWidth = w; chromaHeight = h; }
        else valid = false;
        if (valid) {
            result.pixels.resize(qsizetype(w)*h + 2*qsizetype(chromaWidth)*chromaHeight);
            auto *data = reinterpret_cast<unsigned char *>(result.pixels.data());
            unsigned char *planes[] = {data, data + w*h, data + w*h + chromaWidth*chromaHeight};
            int strides[] = {w, chromaWidth, chromaWidth};
            valid = tjDecompressToYUVPlanes(decoder.handle, jpeg, bytes, planes, w, strides, h,
                                           TJFLAG_FASTDCT) == 0;
        }
        frame.unmap();
        if (!valid) return {};
        result.size = QSize(w,h);
        return result;
    }
    struct Plane { int index; int bytes; int rows; };
    QList<Plane> planes;
    using F = QVideoFrameFormat;
    switch (frame.pixelFormat()) {
    case F::Format_NV12: result.pixelFormat = "nv12"; planes = {{0,w,h},{1,2*cw,ch}}; break;
    case F::Format_NV21: result.pixelFormat = "nv21"; planes = {{0,w,h},{1,2*cw,ch}}; break;
    case F::Format_YUV420P: result.pixelFormat = "yuv420p"; planes = {{0,w,h},{1,cw,ch},{2,cw,ch}}; break;
    case F::Format_YV12: result.pixelFormat = "yuv420p"; planes = {{0,w,h},{2,cw,ch},{1,cw,ch}}; break;
    case F::Format_YUV422P: result.pixelFormat = "yuv422p"; planes = {{0,w,h},{1,cw,h},{2,cw,h}}; break;
    case F::Format_UYVY: result.pixelFormat = "uyvy422"; planes = {{0,2*w,h}}; break;
    case F::Format_YUYV: result.pixelFormat = "yuyv422"; planes = {{0,2*w,h}}; break;
    case F::Format_BGRA8888: case F::Format_BGRX8888: result.pixelFormat = "bgra"; break;
    case F::Format_RGBA8888: case F::Format_RGBX8888: result.pixelFormat = "rgba"; break;
    case F::Format_ARGB8888: case F::Format_XRGB8888: result.pixelFormat = "argb"; break;
    case F::Format_ABGR8888: case F::Format_XBGR8888: result.pixelFormat = "abgr"; break;
    default: break;
    }
    if (result.pixelFormat.isEmpty()) {
        frame.unmap();
        // Exotic/GPU formats use Qt's conversion only on this worker.
        const QImage image = frame.toImage().convertToFormat(QImage::Format_RGBA8888);
        if (image.isNull()) return {};
        result = {QByteArray(reinterpret_cast<const char *>(image.constBits()), image.sizeInBytes()),
                  QStringLiteral("rgba"), image.size()};
        return result;
    }
    if (planes.isEmpty()) planes = {{0,4*w,h}};
    qsizetype total = 0;
    for (const auto &plane : planes) {
        if (plane.index >= frame.planeCount() || !frame.bits(plane.index)
            || frame.bytesPerLine(plane.index) < plane.bytes
            || frame.mappedBytes(plane.index) < qsizetype(plane.rows-1)*frame.bytesPerLine(plane.index)+plane.bytes) {
            frame.unmap();
            return {};
        }
        total += qsizetype(plane.bytes) * plane.rows;
    }
    result.pixels.resize(total);
    char *output = result.pixels.data();
    for (const auto &plane : planes) {
        for (int row = 0; row < plane.rows; ++row) {
            std::memcpy(output, frame.bits(plane.index) + row * frame.bytesPerLine(plane.index), plane.bytes);
            output += plane.bytes;
        }
    }
    frame.unmap();
    result.size = QSize(w,h);
    return result;
}

// A timer-driven worker owns QProcess and all mapping/copying. The mailbox has
// one frame; retain the first admission until consumed, then replace stale frames.
class Omareel::CameraEncoder : public QThread
{
public:
    explicit CameraEncoder(QString path): m_path(std::move(path)) {}
    QString error;
    void submit(const QVideoFrame &frame, qint64 arrival)
    {
        QMutexLocker lock(&m_mutex);
        if (m_firstUs == 0) m_firstUs = arrival;
        else if (!m_consumedFirst) return;
        m_pending = frame;
    }
    void requestStop()
    {
        qint64 expected = 0;
        m_stopDeadline.compare_exchange_strong(expected, monotonicUs() + 4500000);
    }
protected:
    void run() override
    {
        QProcess process;
        QTimer timer;
        timer.setTimerType(Qt::PreciseTimer);
        timer.setSingleShot(true);
        CameraRawFrame latest;
        qint64 firstUs = 0, count = 0;
        bool launched = false;
        const auto remainingMs = [this] {
            return int(std::max<qint64>(0, (m_stopDeadline.load() - monotonicUs()) / 1000));
        };
        const auto finish = [&] {
            timer.stop();
            if (process.state() != QProcess::NotRunning) {
                process.closeWriteChannel();
                if (!process.waitForFinished(remainingMs())) {
                    error = QStringLiteral("Camera encoder did not finish within five seconds");
                    process.kill();
                    process.waitForFinished(500);
                }
            }
            if (launched && (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
                && error.isEmpty())
                error = QStringLiteral("Camera encoder failed: ") + QString::fromUtf8(process.readAllStandardError()).right(2048);
            quit();
        };
        const auto fail = [&](const QString &message) {
            error = message;
            requestStop();
            process.kill();
            finish();
        };
        QObject::connect(&timer, &QTimer::timeout, &timer, [&] {
            if (m_stopDeadline.load()) { finish(); return; }
            QVideoFrame frame;
            {
                QMutexLocker lock(&m_mutex);
                frame = m_pending;
                m_pending = {};
                firstUs = m_firstUs;
                if (frame.isValid()) m_consumedFirst = true;
            }
            if (frame.isValid()) {
                auto packed = packCameraFrame(frame);
                if (packed.pixels.isEmpty()) { fail(QStringLiteral("Could not map camera frame")); return; }
                if (launched && (packed.size != latest.size || packed.pixelFormat != latest.pixelFormat)) {
                    fail(QStringLiteral("Camera changed its raw frame format during recording")); return;
                }
                latest = std::move(packed);
            }
            if (latest.pixels.isEmpty()) { timer.start(33); return; }
            if (!launched) {
                const bool nvenc = cachedPreferredEncoderAvailable();
                QStringList args{"-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo",
                    "-pixel_format", latest.pixelFormat, "-video_size",
                    QStringLiteral("%1x%2").arg(latest.size.width()).arg(latest.size.height()),
                    "-framerate", "30", "-i", "pipe:0", "-an", "-c:v", nvenc ? "h264_nvenc" : "libx264"};
                args += nvenc ? QStringList{"-preset", "p4", "-rc", "vbr", "-cq", "23"}
                              : QStringList{"-preset", "veryfast", "-crf", "20"};
                args += QStringList{"-bf", "0", "-g", "60", "-pix_fmt", "yuv420p", "-profile:v", "high",
                                    "-fps_mode", "cfr", "-video_track_timescale", "30000",
                                    "-movflags", "+faststart", m_path};
                QTextStream(stderr) << "Camera encoder: " << (nvenc ? "h264_nvenc" : "libx264")
                                    << " raw format: " << latest.pixelFormat << '\n';
                process.start(QStringLiteral("ffmpeg"), args);
                if (!process.waitForStarted(1000)) { fail(QStringLiteral("Could not start camera ffmpeg: ") + process.errorString()); return; }
                launched = true;
            }
            if (process.state() == QProcess::NotRunning) {
                fail(QStringLiteral("Camera encoder exited: ") + QString::fromUtf8(process.readAllStandardError()).right(2048)); return;
            }
            if (monotonicUs() - cameraFrameDeadlineUs(firstUs, count) > 2000000) {
                fail(QStringLiteral("Camera encoder fell more than two seconds behind")); return;
            }
            if (process.write(latest.pixels) != latest.pixels.size()) {
                fail(QStringLiteral("Could not write camera frame to ffmpeg")); return;
            }
            const qint64 writeDeadline = monotonicUs() + 2000000;
            // QProcess buffers at most one packed frame. Drain it before admitting
            // another write; the deadline covers the entire frame, not each chunk.
            for (int attempt = 0; attempt < 200 && process.bytesToWrite() > 0
                 && monotonicUs() < writeDeadline; ++attempt) {
                if (m_stopDeadline.load() && remainingMs() == 0) break;
                if (!process.waitForBytesWritten(10) && process.state() == QProcess::NotRunning) break;
            }
            if (process.bytesToWrite() > 0 || process.state() == QProcess::NotRunning) {
                fail(QStringLiteral("Camera encoder pipe failed or stalled for two seconds: ")
                     + QString::fromUtf8(process.readAllStandardError()).right(2048)); return;
            }
            ++count;
            const auto stderrText = process.readAllStandardError();
            if (!stderrText.isEmpty()) qWarning().noquote() << "Camera ffmpeg:" << QString::fromUtf8(stderrText).right(2048);
            const qint64 delay = cameraFrameDeadlineUs(firstUs, count) - monotonicUs();
            timer.start(int(std::max<qint64>(0, (delay + 999) / 1000)));
        });
        timer.start(0);
        exec();
    }
private:
    QString m_path;
    QMutex m_mutex;
    QVideoFrame m_pending;
    qint64 m_firstUs = 0;
    bool m_consumedFirst = false;
    std::atomic<qint64> m_stopDeadline{0};
};

CameraCapture::CameraCapture(const QString &devicePath, int requestedHeight,
                             const QString &videoPath, const QString &timestampPath,
                             QObject *parent)
    : QObject(parent), m_devicePath(devicePath),
      m_requestedHeight(requestedHeight == 720 ? 720 : 1080),
      m_videoPath(videoPath), m_timestamp(timestampPath),
      m_session(std::make_unique<QMediaCaptureSession>()),
      m_sink(std::make_unique<QVideoSink>()),
      m_startTimer(std::make_unique<QTimer>())
{
    m_session->setVideoSink(m_sink.get());
    connect(m_sink.get(), &QVideoSink::videoFrameChanged, this, &CameraCapture::handleFrame);
    m_startTimer->setSingleShot(true);
    m_startTimer->setInterval(2000);
    connect(m_startTimer.get(), &QTimer::timeout, this, [this] {
        if (!m_ready) fail(QStringLiteral("Camera did not produce a frame within two seconds"));
    });
}

CameraCapture::~CameraCapture()
{
    if (m_camera) m_camera->stop();
    if (m_encoder && m_encoder->isRunning()) {
        m_encoder->requestStop();
        if (!m_encoder->wait(5500)) qFatal("Camera worker did not stop within its deadline");
    }
    m_session->setCamera(nullptr);
    m_session->setVideoSink(nullptr);
}

void CameraCapture::attachVideoOutput(QObject *output)
{
    m_previewSink = output ? qvariant_cast<QVideoSink *>(output->property("videoSink")) : nullptr;
}

void CameraCapture::start()
{
    if (m_running || m_stopping) return;
    const QCameraDevice device = selectedDevice(m_devicePath);
    if (device.isNull()) { fail(QStringLiteral("Selected camera is unavailable")); return; }
    m_camera = std::make_unique<QCamera>(device);
    connect(m_camera.get(), &QCamera::errorOccurred, this,
            [this](QCamera::Error, const QString &message) { fail(message); });
    const QCameraFormat format = selectedFormat(device, m_requestedHeight);
    if (format.isNull()) { fail(QStringLiteral("Selected camera has no 30 fps capture format")); return; }
    m_camera->setCameraFormat(format);
    m_captureSize = format.resolution();
    m_frameRate = 30.0;
    emit captureSizeChanged();
    m_session->setCamera(m_camera.get());
    m_running = true;
    emit runningChanged();
    m_startTimer->start();
    m_camera->start();
}

void CameraCapture::stop()
{
    if (m_stopping) return;
    m_stopping = true;
    m_recordingFrames = false;
    m_startTimer->stop();
    if (m_camera) m_camera->stop();
    if (m_encoder && m_encoder->isRunning()) m_encoder->requestStop();
    else finishStop();
}

void CameraCapture::finishStop()
{
    if (m_encoder && !m_encoder->error.isEmpty() && m_errorString.isEmpty()) {
        m_errorString = m_encoder->error;
        qWarning().noquote() << m_errorString;
        emit errorOccurred(m_errorString);
    }
    m_stopping = true;
    m_recordingFrames = false;
    m_startTimer->stop();
    if (m_camera) m_camera->stop();
    m_running = false;
    emit runningChanged();
    emit stopped();
}

void CameraCapture::setRecordingOutput(const QString &videoPath, const QString &timestampPath)
{
    if (m_recordingFrames || m_stopping) return;
    m_videoPath = videoPath;
    m_timestamp.setPath(timestampPath);
}

void CameraCapture::beginRecording()
{
    if (m_stopping || m_recordingFrames) return;
    m_recordingFrames = true; // May be requested before start()/the first camera frame.
}

void CameraCapture::handleFrame(const QVideoFrame &frame)
{
    if (!frame.isValid() || m_stopping) return;
    const qint64 timestamp = monotonicUs();
    if (!m_formatLogged) {
        m_formatLogged = true;
        QString description;
        QDebug(&description) << frame.pixelFormat() << frame.size();
        QTextStream(stderr) << "Camera frame.pixelFormat(): " << description << '\n';
    }
    if (m_previewSink) m_previewSink->setVideoFrame(frame);
    if (!m_ready) {
        m_ready = true;
        m_startTimer->stop();
        emit readyChanged();
    }
    if (!m_recordingFrames) return;
    if (!m_timestamp.recordFrameArrival(timestamp, QDateTime::currentMSecsSinceEpoch() * 1000)) {
        fail(QStringLiteral("Could not write camera timestamp")); return;
    }
    if (!m_encoder) {
        m_encoder = std::make_unique<CameraEncoder>(m_videoPath);
        connect(m_encoder.get(), &QThread::finished, this, &CameraCapture::finishStop);
        m_encoder->submit(frame, timestamp);
        m_encoder->start();
    } else m_encoder->submit(frame, timestamp);
}

void CameraCapture::fail(const QString &message)
{
    if (!m_errorString.isEmpty()) return;
    m_errorString = message.isEmpty() ? QStringLiteral("Camera capture failed") : message;
    qWarning().noquote() << m_errorString;
    emit errorOccurred(m_errorString);
    stop();
}
