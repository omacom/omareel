#include "CameraCapture.h"

#include <QCamera>
#include <QCameraDevice>
#include <QCameraFormat>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
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

QString Omareel::cameraNodePath(const QString &path)
{
    return QFileInfo(path).canonicalFilePath();
}

QString Omareel::cameraIdNode(const QByteArray &id)
{
    const QString text = QString::fromUtf8(id);
    const qsizetype marker = text.indexOf(QLatin1String("/dev/video"));
    return marker < 0 ? QString() : text.mid(marker).section(QLatin1Char(' '), 0, 0);
}

QCameraDevice Omareel::cameraDeviceForPath(const QList<QCameraDevice> &devices, const QString &path)
{
    const QString node = cameraNodePath(path);
    if (node.isEmpty()) return {};
    for (const QCameraDevice &device : devices)
        if (cameraIdNode(device.id()) == node) return device;
    return {};
}

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

qint64 CameraFrameClock::frameTimeUs(qint64 startTimeUs, qint64 arrivalUs)
{
    // A window of two seconds of driver time; half a second above the anchor for all of it is
    // no UI stall.
    constexpr qint64 windowUs = 2000000, lagUs = 500000;
    qint64 time = arrivalUs;
    if (startTimeUs >= 0 && startTimeUs > m_lastStartUs) {
        const qint64 offset = arrivalUs - startTimeUs;
        if (!m_hasOffset) {
            m_offsetUs = m_windowOffsetUs = offset;
            m_windowStartUs = startTimeUs;
        } else if (startTimeUs - m_windowStartUs >= windowUs) {
            if (m_windowOffsetUs - m_offsetUs > lagUs) m_offsetUs = m_windowOffsetUs;
            m_windowStartUs = startTimeUs;
            m_windowOffsetUs = offset;
        } else {
            m_windowOffsetUs = std::min(m_windowOffsetUs, offset);
        }
        m_offsetUs = std::min(m_offsetUs, offset);
        m_hasOffset = true;
        time = m_offsetUs + startTimeUs;
    } else {
        // No driver time, or the camera restarted its stream: arrival until it advances again.
        m_hasOffset = false;
    }
    m_lastStartUs = startTimeUs;
    // A millisecond apart at least, so no two frames share a tick of the encoder's time base.
    if (m_lastFrameUs > 0) time = std::max(time, m_lastFrameUs + 1000);
    m_lastFrameUs = time;
    return time;
}

CameraRawFrame Omareel::packCameraFrame(QVideoFrame frame)
{
    CameraRawFrame result;
    if (!frame.map(QVideoFrame::ReadOnly)) return result;
    // UVC cameras send BT.601 YCbCr unless their descriptor says otherwise, which Qt reports as
    // the colour space; V4L2's JPEG colour space (Qt's AdobeRgb) is full-range YCbCr.
    const QVideoFrameFormat surface = frame.surfaceFormat();
    if (surface.colorSpace() == QVideoFrameFormat::ColorSpace_BT709) result.colorMatrix = QStringLiteral("bt709");
    else if (surface.colorSpace() == QVideoFrameFormat::ColorSpace_BT2020) result.colorMatrix = QStringLiteral("bt2020");
    result.fullRange = surface.colorRange() == QVideoFrameFormat::ColorRange_Full
        || (surface.colorRange() != QVideoFrameFormat::ColorRange_Video
            && surface.colorSpace() == QVideoFrameFormat::ColorSpace_AdobeRgb);
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
        // JFIF: BT.601, full range, whatever the camera reports.
        result.colorMatrix = QStringLiteral("bt601");
        result.fullRange = true;
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

namespace {

// The EBML writer of screen_capture (crates/omareel_record), which plumbing doesn't call: IDs
// keep their marker bits, sizes are always eight bytes.
void appendEbmlId(QByteArray &out, quint32 id)
{
    for (int shift = 24; shift >= 0; shift -= 8)
        if (id >> shift) out.append(char((id >> shift) & 0xff));
}

void appendEbmlSize(QByteArray &out, quint64 size)
{
    out.append(char(0x01));
    for (int shift = 48; shift >= 0; shift -= 8) out.append(char((size >> shift) & 0xff));
}

QByteArray ebmlElement(quint32 id, const QByteArray &payload)
{
    QByteArray out;
    appendEbmlId(out, id);
    appendEbmlSize(out, quint64(payload.size()));
    return out + payload;
}

QByteArray ebmlUnsigned(quint32 id, quint64 value)
{
    QByteArray payload(8, Qt::Uninitialized);
    for (int index = 0; index < 8; ++index) payload[index] = char((value >> (56 - 8 * index)) & 0xff);
    return ebmlElement(id, payload);
}

bool rgbFormat(const QString &pixelFormat)
{
    return pixelFormat == QLatin1String("bgra") || pixelFormat == QLatin1String("rgba")
        || pixelFormat == QLatin1String("argb") || pixelFormat == QLatin1String("abgr");
}

// The FourCC ffmpeg's Matroska demuxer reads each packed format back from. The JPEG decoder's
// full-range planes are the plain planar formats; the range goes to ffmpeg separately.
QByteArray rawFourcc(const QString &pixelFormat)
{
    static const QHash<QString, QByteArray> fourccs{
        {QStringLiteral("nv12"), "NV12"}, {QStringLiteral("nv21"), "NV21"},
        {QStringLiteral("yuv420p"), "I420"}, {QStringLiteral("yuvj420p"), "I420"},
        {QStringLiteral("yuv422p"), "Y42B"}, {QStringLiteral("yuvj422p"), "Y42B"},
        {QStringLiteral("yuvj444p"), "444P"},
        {QStringLiteral("yuyv422"), "YUY2"}, {QStringLiteral("uyvy422"), "UYVY"},
        {QStringLiteral("bgra"), "BGRA"}, {QStringLiteral("rgba"), "RGBA"},
        {QStringLiteral("argb"), "ARGB"}, {QStringLiteral("abgr"), "ABGR"},
    };
    return fourccs.value(pixelFormat);
}

} // namespace

QByteArray Omareel::cameraStreamHeader(const CameraRawFrame &format)
{
    const QByteArray fourcc = rawFourcc(format.pixelFormat);
    if (fourcc.isEmpty() || format.size.isEmpty()) return {};
    const QByteArray ebml = ebmlElement(0x1A45DFA3,
        ebmlUnsigned(0x4286, 1) + ebmlUnsigned(0x42F7, 1) + ebmlUnsigned(0x42F2, 4)
        + ebmlUnsigned(0x42F3, 8) + ebmlElement(0x4282, "matroska") + ebmlUnsigned(0x4287, 4)
        + ebmlUnsigned(0x4285, 2));
    // Timestamps count microseconds. The default duration, one frame at the 30 fps the format
    // was chosen for, is only the last frame's.
    const QByteArray info = ebmlElement(0x1549A966,
        ebmlUnsigned(0x2AD7B1, 1000) + ebmlElement(0x4D80, "omareel") + ebmlElement(0x5741, "omareel"));
    const QByteArray video = ebmlElement(0xE0,
        ebmlUnsigned(0xB0, quint64(format.size.width())) + ebmlUnsigned(0xBA, quint64(format.size.height()))
        + ebmlElement(0x2EB524, fourcc));
    const QByteArray track = ebmlElement(0xAE,
        ebmlUnsigned(0xD7, 1) + ebmlUnsigned(0x73C5, 1) + ebmlUnsigned(0x83, 1)
        + ebmlElement(0x86, "V_UNCOMPRESSED") + ebmlUnsigned(0x9C, 0)
        + ebmlUnsigned(0x23E383, 33333333) + video);
    QByteArray segment;
    appendEbmlId(segment, 0x18538067);
    // Unknown size: the stream ends with the pipe.
    segment.append("\x01\xff\xff\xff\xff\xff\xff\xff", 8);
    return ebml + segment + info + ebmlElement(0x1654AE6B, track);
}

QByteArray Omareel::cameraFrameHeader(qint64 timestampUs, qint64 frameBytes)
{
    // A cluster at the frame's time holding one SimpleBlock of track 1, a keyframe at no offset;
    // the frame's bytes follow.
    const char blockHeader[] = {char(0x81), 0x00, 0x00, char(0x80)};
    QByteArray block;
    appendEbmlId(block, 0xA3);
    appendEbmlSize(block, quint64(qint64(sizeof blockHeader) + frameBytes));
    block.append(blockHeader, sizeof blockHeader);
    const QByteArray timestamp = ebmlUnsigned(0xE7, quint64(std::max<qint64>(0, timestampUs)));
    QByteArray cluster;
    appendEbmlId(cluster, 0x1F43B675);
    appendEbmlSize(cluster, quint64(timestamp.size() + block.size() + frameBytes));
    return cluster + timestamp + block;
}

QStringList Omareel::cameraEncoderArguments(const CameraRawFrame &format, bool nvenc, const QString &path)
{
    // swscale reads untagged YCbCr as BT.601 while players take HD video for BT.709, so convert
    // from what the camera sent to BT.709 limited range here, once, and say so in the file.
    QString scale = QStringLiteral("scale=");
    if (!rgbFormat(format.pixelFormat))
        scale += QStringLiteral("in_color_matrix=%1:in_range=%2:")
                     .arg(format.colorMatrix, format.fullRange ? QStringLiteral("pc") : QStringLiteral("tv"));
    // The encoder takes the tags from the frames; cameras use BT.709 primaries and transfer.
    scale += QStringLiteral("out_color_matrix=bt709:out_range=tv,format=yuv420p,"
                            "setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv");
    QStringList args{"-hide_banner", "-loglevel", "error", "-y", "-f", "matroska", "-i", "pipe:0", "-an",
                     "-vf", scale, "-c:v", nvenc ? "h264_nvenc" : "libx264"};
    args += nvenc ? QStringList{"-preset", "p4", "-zerolatency", "1", "-rc", "vbr", "-cq", "23"}
                  : QStringList{"-preset", "veryfast", "-tune", "zerolatency", "-crf", "20"};
    // A keyframe, and so a fragment, about every second; zerolatency stops that GOP staying
    // inside the encoder when the recording stops.
    args += QStringList{"-bf", "0", "-g", "30", "-pix_fmt", "yuv420p", "-profile:v", "high",
                        "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
                        "-color_range", "tv",
                        // Frames keep the times they were captured at; gaps stay gaps.
                        "-fps_mode", "vfr", "-enc_time_base", "1/90000", "-video_track_timescale", "90000",
                        // Fragmented, so a recorder that dies leaves every finished fragment playable.
                        "-movflags", "+frag_keyframe+empty_moov+default_base_moof", path};
    return args;
}

CameraEncoder::CameraEncoder(QString path, std::function<bool()> preferredEncoderQuery)
    : m_path(std::move(path)), m_preferredEncoderQuery(std::move(preferredEncoderQuery))
{
}

CameraEncoder::~CameraEncoder()
{
    if (isRunning()) {
        requestStop();
        wait();
    }
}

bool CameraEncoder::submit(const QVideoFrame &frame, qint64 timeUs)
{
    // About a second of frames; more waiting means the encoder can't keep up.
    constexpr size_t queueLimit = 30;
    QMutexLocker lock(&m_mutex);
    if (m_stopDeadline.load()) return false;
    if (m_firstUs < 0) m_firstUs = timeUs;
    if (m_queue.size() >= queueLimit) {
        const qint64 dropped = ++m_dropped;
        if (dropped == 1 || dropped % 30 == 0)
            qWarning().noquote() << "Camera encoder is behind; dropped" << dropped << "frames";
        return false;
    }
    m_queue.push_back({frame, timeUs - m_firstUs});
    m_queued.wakeOne();
    return true;
}

void CameraEncoder::requestStop()
{
    qint64 expected = 0;
    m_stopDeadline.compare_exchange_strong(expected, monotonicUs() + 4500000);
    QMutexLocker lock(&m_mutex);
    m_queued.wakeAll();
}

QString CameraEncoder::error() const
{
    QMutexLocker lock(&m_mutex);
    return m_error;
}

void CameraEncoder::run()
{
    QProcess process;
    QString pixelFormat;
    QSize size;
    bool launched = false;
    QString failure;
    const auto remainingMs = [this] {
        return int(std::max<qint64>(0, (m_stopDeadline.load() - monotonicUs()) / 1000));
    };
    const auto errorText = [&process] { return QString::fromUtf8(process.readAllStandardError()).right(2048); };
    // QProcess buffers what it is given; the pipe has to take all of it within two seconds.
    const auto write = [&process](const QByteArray &header, const QByteArray &pixels) {
        if (process.write(header) != header.size() || process.write(pixels) != pixels.size()) return false;
        const qint64 deadline = monotonicUs() + 2000000;
        while (process.bytesToWrite() > 0 && process.state() != QProcess::NotRunning && monotonicUs() < deadline)
            process.waitForBytesWritten(10);
        return process.bytesToWrite() == 0 && process.state() != QProcess::NotRunning;
    };
    for (;;) {
        Queued item;
        {
            QMutexLocker lock(&m_mutex);
            while (m_queue.empty() && !m_stopDeadline.load()) m_queued.wait(&m_mutex);
            if (m_queue.empty()) break;
            // Frames still waiting when the stop deadline nears are dropped; ffmpeg keeps a
            // second and a half to finish the file.
            if (m_stopDeadline.load() && remainingMs() < 1500) {
                m_dropped += qint64(m_queue.size());
                m_queue.clear();
                break;
            }
            item = std::move(m_queue.front());
            m_queue.pop_front();
        }
        CameraRawFrame packed = packCameraFrame(item.frame);
        item.frame = {};
        if (packed.pixels.isEmpty()) { failure = QStringLiteral("Could not map camera frame"); break; }
        if (!launched) {
            const QByteArray header = cameraStreamHeader(packed);
            if (header.isEmpty()) {
                failure = QStringLiteral("Camera frames are in a format the encoder doesn't take: ") + packed.pixelFormat;
                break;
            }
            const bool nvenc = m_preferredEncoderQuery && m_preferredEncoderQuery();
            const QStringList args = cameraEncoderArguments(packed, nvenc, m_path);
            QTextStream(stderr) << "Camera encoder: " << (nvenc ? "h264_nvenc" : "libx264")
                                << " raw format: " << packed.pixelFormat << ' ' << args[args.indexOf("-vf") + 1] << '\n';
            process.start(QStringLiteral("ffmpeg"), args);
            if (!process.waitForStarted(1000)) {
                failure = QStringLiteral("Could not start camera ffmpeg: ") + process.errorString();
                break;
            }
            launched = true;
            pixelFormat = packed.pixelFormat;
            size = packed.size;
            if (!write(header, {})) {
                failure = QStringLiteral("Camera encoder exited: ") + errorText();
                break;
            }
        } else if (packed.size != size || packed.pixelFormat != pixelFormat) {
            failure = QStringLiteral("Camera changed its raw frame format during recording");
            break;
        }
        if (!write(cameraFrameHeader(item.timestampUs, packed.pixels.size()), packed.pixels)) {
            failure = QStringLiteral("Camera encoder pipe failed or stalled for two seconds: ") + errorText();
            break;
        }
        ++m_encoded;
        const QString text = errorText();
        if (!text.isEmpty()) qWarning().noquote() << "Camera ffmpeg:" << text;
    }
    if (!failure.isEmpty()) {
        requestStop();
        process.kill();
    }
    {
        QMutexLocker lock(&m_mutex);
        m_queue.clear();
    }
    if (process.state() != QProcess::NotRunning) {
        process.closeWriteChannel();
        if (!process.waitForFinished(remainingMs())) {
            if (failure.isEmpty()) failure = QStringLiteral("Camera encoder did not finish within five seconds");
            process.kill();
            process.waitForFinished(500);
        }
    }
    if (launched && failure.isEmpty() && (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0))
        failure = QStringLiteral("Camera encoder failed: ") + errorText();
    QTextStream(stderr) << "Camera encoder: " << m_encoded.load() << " frames encoded, "
                        << m_dropped.load() << " dropped\n";
    QMutexLocker lock(&m_mutex);
    m_error = failure;
}

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
    // Some webcams take seconds to stream after they open; the Insta360 Link 2 calibrates first.
    // Only a camera that stays dark well past that is treated as broken.
    m_startTimer->setInterval(10000);
    connect(m_startTimer.get(), &QTimer::timeout, this, [this] {
        if (!m_ready) fail(QStringLiteral("Camera did not produce a frame within ten seconds"));
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
    // The preview keeps the previous camera's final frame otherwise, which looks like a frozen
    // feed while this one starts or after it fails.
    if (m_previewSink) m_previewSink->setVideoFrame({});
    const QCameraDevice device = cameraDeviceForPath(QMediaDevices::videoInputs(), m_devicePath);
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
    if (m_encoder && !m_encoder->error().isEmpty() && m_errorString.isEmpty()) {
        m_errorString = m_encoder->error();
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
    // Preview frames keep the clock's anchor current before recording starts.
    const qint64 arrival = monotonicUs();
    const qint64 timestamp = m_clock.frameTimeUs(frame.startTime(), arrival);
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
    const qint64 realtime = QDateTime::currentMSecsSinceEpoch() * 1000 - (arrival - timestamp);
    if (!m_timestamp.recordFrameArrival(timestamp, realtime)) {
        fail(QStringLiteral("Could not write camera timestamp")); return;
    }
    if (!m_encoder) {
        m_encoder = std::make_unique<CameraEncoder>(m_videoPath, m_preferredEncoderQuery);
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
