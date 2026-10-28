#include "CameraCapture.h"

#include <QtTest>
#include <QTemporaryDir>
#include <QVideoFrame>
#include <QAbstractVideoBuffer>
#include <QBuffer>
#include <QElapsedTimer>
#include <QProcess>
#include <QStandardPaths>
#include <QThread>
#include <array>
#include <cmath>
#include <cstring>

using namespace Omareel;

class JpegTestBuffer : public QAbstractVideoBuffer
{
public:
    explicit JpegTestBuffer(QByteArray bytes, QSize size = QSize(16, 8)): data(std::move(bytes)), size(size) {}
    MapData map(QVideoFrame::MapMode) override
    {
        MapData mapped;
        mapped.planeCount = 1;
        mapped.data[0] = reinterpret_cast<uchar *>(data.data());
        mapped.dataSize[0] = data.size();
        return mapped;
    }
    QVideoFrameFormat format() const override
    { return QVideoFrameFormat(size, QVideoFrameFormat::Format_Jpeg); }
    QByteArray data;
    QSize size;
};

namespace {

// Runs a tool to completion; its standard output, and whether it exited cleanly.
QByteArray run(const QString &program, const QStringList &arguments, bool *ok)
{
    QProcess process;
    process.start(program, arguments);
    const bool finished = process.waitForFinished(60000);
    *ok = finished && process.exitStatus() == QProcess::NormalExit && process.exitCode() == 0;
    if (!*ok) qWarning().noquote() << program << arguments.join(' ') << process.readAllStandardError();
    return process.readAllStandardOutput();
}

bool haveFfmpeg()
{
    return !QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
        && !QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty();
}

// BT.601 or BT.709 limited-range YCbCr of an RGB colour.
std::array<uchar, 3> yuv(QColor colour, bool bt709)
{
    const double r = colour.red(), g = colour.green(), b = colour.blue();
    const double y = bt709 ? 46.559 * r + 156.629 * g + 15.812 * b : 65.481 * r + 128.553 * g + 24.966 * b;
    const double cb = bt709 ? -25.664 * r - 86.336 * g + 112.0 * b : -37.797 * r - 74.203 * g + 112.0 * b;
    const double cr = bt709 ? 112.0 * r - 101.730 * g - 10.270 * b : 112.0 * r - 93.786 * g - 18.214 * b;
    return {uchar(std::lround(16 + y / 255)), uchar(std::lround(128 + cb / 255)), uchar(std::lround(128 + cr / 255))};
}

QVideoFrame nv12Frame(QSize size, uchar luma, uchar cb = 128, uchar cr = 128,
                      QVideoFrameFormat::ColorSpace space = QVideoFrameFormat::ColorSpace_Undefined)
{
    QVideoFrameFormat format(size, QVideoFrameFormat::Format_NV12);
    format.setColorSpace(space);
    QVideoFrame frame(format);
    if (!frame.map(QVideoFrame::WriteOnly)) return {};
    for (int row = 0; row < size.height(); ++row)
        std::memset(frame.bits(0) + row * frame.bytesPerLine(0), luma, size.width());
    for (int row = 0; row < size.height() / 2; ++row)
        for (int column = 0; column < size.width() / 2; ++column) {
            frame.bits(1)[row * frame.bytesPerLine(1) + 2 * column] = cb;
            frame.bits(1)[row * frame.bytesPerLine(1) + 2 * column + 1] = cr;
        }
    frame.unmap();
    return frame;
}

QVideoFrame yuyvFrame(QSize size, std::array<uchar, 3> value)
{
    QVideoFrame frame(QVideoFrameFormat(size, QVideoFrameFormat::Format_YUYV));
    if (!frame.map(QVideoFrame::WriteOnly)) return {};
    for (int row = 0; row < size.height(); ++row)
        for (int pair = 0; pair < size.width() / 2; ++pair) {
            uchar *pixel = frame.bits(0) + row * frame.bytesPerLine(0) + 4 * pair;
            pixel[0] = value[0]; pixel[1] = value[1]; pixel[2] = value[0]; pixel[3] = value[2];
        }
    frame.unmap();
    return frame;
}

QVideoFrame bgraFrame(QSize size, QColor colour)
{
    QVideoFrame frame(QVideoFrameFormat(size, QVideoFrameFormat::Format_BGRA8888));
    if (!frame.map(QVideoFrame::WriteOnly)) return {};
    for (int row = 0; row < size.height(); ++row)
        for (int column = 0; column < size.width(); ++column) {
            uchar *pixel = frame.bits(0) + row * frame.bytesPerLine(0) + 4 * column;
            pixel[0] = colour.blue(); pixel[1] = colour.green(); pixel[2] = colour.red(); pixel[3] = 255;
        }
    frame.unmap();
    return frame;
}

QVideoFrame jpegFrame(QSize size, QColor colour)
{
    QImage source(size, QImage::Format_RGB32);
    source.fill(colour);
    QByteArray jpeg;
    QBuffer buffer(&jpeg);
    if (!buffer.open(QIODevice::WriteOnly) || !source.save(&buffer, "JPEG", 95)) return {};
    return QVideoFrame(std::make_unique<JpegTestBuffer>(jpeg, size));
}

struct EncodeResult {
    QString error;
    qint64 encoded = 0;
    qint64 dropped = 0;
};

// Records frames at the given CLOCK_MONOTONIC times through the camera encoder (libx264). The
// test is about times, not about dropping, so it lets the encoder catch up every ten frames.
EncodeResult encode(const QString &path, const QList<QPair<QVideoFrame, qint64>> &frames)
{
    CameraEncoder encoder(path, [] { return false; });
    encoder.start();
    for (qsizetype index = 0; index < frames.size(); ++index) {
        encoder.submit(frames[index].first, frames[index].second);
        if (index % 10 == 9) {
            QElapsedTimer timer;
            timer.start();
            while (encoder.encodedFrames() < index + 1 && encoder.isRunning() && timer.elapsed() < 20000)
                QThread::msleep(5);
        }
    }
    encoder.requestStop();
    encoder.wait(20000);
    return {encoder.error(), encoder.encodedFrames(), encoder.droppedFrames()};
}

// The top-level MP4 boxes: type and offset.
QList<QPair<QByteArray, qint64>> topLevelBoxes(const QByteArray &file)
{
    QList<QPair<QByteArray, qint64>> boxes;
    qint64 offset = 0;
    while (offset + 8 <= file.size()) {
        const auto *bytes = reinterpret_cast<const uchar *>(file.constData() + offset);
        const qint64 size = qint64(bytes[0]) << 24 | qint64(bytes[1]) << 16 | qint64(bytes[2]) << 8 | bytes[3];
        boxes.append({file.mid(offset + 4, 4), offset});
        if (size < 8) break;
        offset += size;
    }
    return boxes;
}

} // namespace

class CameraCaptureTest : public QObject
{
    Q_OBJECT
private slots:
    void firstCameraFrameWritesTimestampOnce()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path = directory.filePath(QStringLiteral("camera.mp4.ts"));
        FirstFrameTimestamp timestamp(path);
        QVERIFY(timestamp.recordFrameArrival(1234567, 9876543));
        QVERIFY(timestamp.recordFrameArrival(2345678, 8765432));
        QFile file(path);
        QVERIFY(file.open(QIODevice::ReadOnly | QIODevice::Text));
        QCOMPARE(file.readAll(), QByteArray("monotonic_microsec\trealtime_microsec\n"
                                            "1234567\t9876543\n"));
        QCOMPARE(timestamp.firstFrameUs(), qint64(1234567));
    }

    void frameClockKeepsDriverSpacing()
    {
        // The driver stamps frames 33.333 ms apart; they reach the sink 20-40 ms later, one
        // after a 400 ms UI stall. Times keep the driver's spacing from the least delay seen.
        const qint64 zero = 5000000000;
        const qint64 delays[] = {25000, 22000, 40000, 20000, 400000, 21000, 30000};
        CameraFrameClock clock;
        qint64 least = delays[0];
        for (int index = 0; index < 7; ++index) {
            const qint64 start = index * 33333;
            least = std::min(least, delays[index]);
            QCOMPARE(clock.frameTimeUs(start, zero + start + delays[index]), zero + least + start);
        }
    }

    void frameClockFallsBackToArrival()
    {
        CameraFrameClock clock;
        // No driver time at all.
        QCOMPARE(clock.frameTimeUs(-1, 1000000), qint64(1000000));
        QCOMPARE(clock.frameTimeUs(-1, 1040000), qint64(1040000));
        // A driver that stamps nothing: the first zero is taken, the rest don't advance.
        CameraFrameClock zeros;
        QCOMPARE(zeros.frameTimeUs(0, 2000000), qint64(2000000));
        QCOMPARE(zeros.frameTimeUs(0, 2035000), qint64(2035000));
        QCOMPARE(zeros.frameTimeUs(0, 2061000), qint64(2061000));
        // The stream restarts (Qt counts from its new first frame): arrival, then a new anchor.
        CameraFrameClock restarted;
        QCOMPARE(restarted.frameTimeUs(0, 3000000), qint64(3000000));
        QCOMPARE(restarted.frameTimeUs(33333, 3040000), qint64(3033333));
        QCOMPARE(restarted.frameTimeUs(0, 3500000), qint64(3500000));
        QCOMPARE(restarted.frameTimeUs(33333, 3560000), qint64(3560000));
        QCOMPARE(restarted.frameTimeUs(66666, 3570000), qint64(3570000));
        // Times always increase, by a millisecond at least.
        CameraFrameClock same;
        QCOMPARE(same.frameTimeUs(-1, 4000000), qint64(4000000));
        QCOMPARE(same.frameTimeUs(-1, 4000000), qint64(4001000));
    }

    void frameClockFollowsADriftingDriverClock()
    {
        // A driver clock that falls behind the monotonic clock by 2 ms a frame (not uvcvideo's
        // CLOCK_MONOTONIC): after a two-second window spent more than half a second behind, the
        // anchor moves to that window's least delay. Times never pass arrival.
        CameraFrameClock clock;
        qint64 previous = 0;
        bool moved = false;
        for (int index = 0; index < 600; ++index) {
            const qint64 start = index * 33333;
            const qint64 arrival = 7000000 + start + 2000 * index + 20000;
            const qint64 time = clock.frameTimeUs(start, arrival);
            QVERIFY(time <= arrival);
            QVERIFY(time > previous);
            QVERIFY(arrival - time < 1100000);
            moved = moved || (index > 300 && arrival - time < 200000);
            previous = time;
        }
        QVERIFY(moved);
    }

    void cameraStreamHeaderNamesTheRawFormat()
    {
        CameraRawFrame format;
        format.size = QSize(64, 48);
        format.pixelFormat = QStringLiteral("yuyv422");
        QVERIFY(cameraStreamHeader(format).contains(QByteArray("\x2e\xb5\x24", 3) + QByteArray::fromHex("0100000000000004") + "YUY2"));
        format.pixelFormat = QStringLiteral("yuvj420p");
        QVERIFY(cameraStreamHeader(format).contains("I420"));
        format.pixelFormat = QStringLiteral("p010");
        QVERIFY(cameraStreamHeader(format).isEmpty());
        // A cluster at 1.5 s holding a 10-byte keyframe.
        QCOMPARE(cameraFrameHeader(1500000, 10).toHex(),
                 QByteArray("1f43b6750100000000000028" "e7010000000000000800000000" "0016e360"
                            "a3010000000000000e" "81000080"));
    }

    void cameraEncoderArgumentsConvertAndTag()
    {
        CameraRawFrame format;
        format.size = QSize(1920, 1080);
        format.pixelFormat = QStringLiteral("yuyv422");
        const QStringList tags{"-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
                               "-color_range", "tv"};
        for (const bool nvenc : {false, true}) {
            const QStringList args = cameraEncoderArguments(format, nvenc, QStringLiteral("/b/camera.mp4"));
            QCOMPARE(args.mid(args.indexOf("-f"), 4), QStringList({"-f", "matroska", "-i", "pipe:0"}));
            QCOMPARE(args.at(args.indexOf("-vf") + 1),
                     QStringLiteral("scale=in_color_matrix=bt601:in_range=tv:out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv"));
            QCOMPARE(args.at(args.indexOf("-c:v") + 1), nvenc ? QStringLiteral("h264_nvenc") : QStringLiteral("libx264"));
            QCOMPARE(args.mid(args.indexOf("-colorspace"), tags.size()), tags);
            QCOMPARE(args.at(args.indexOf("-fps_mode") + 1), QStringLiteral("vfr"));
            QCOMPARE(args.at(args.indexOf("-enc_time_base") + 1), QStringLiteral("1/90000"));
            QCOMPARE(args.at(args.indexOf("-movflags") + 1), QStringLiteral("+frag_keyframe+empty_moov+default_base_moof"));
            QVERIFY(!args.join(' ').contains("faststart"));
            QVERIFY(!args.contains("-framerate"));
            QCOMPARE(args.last(), QStringLiteral("/b/camera.mp4"));
        }
        format.pixelFormat = QStringLiteral("yuvj422p");
        format.fullRange = true;
        QVERIFY(cameraEncoderArguments(format, false, "x.mp4").contains(
            "scale=in_color_matrix=bt601:in_range=pc:out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv"));
        format.pixelFormat = QStringLiteral("bgra");
        QVERIFY(cameraEncoderArguments(format, false, "x.mp4").contains(
            "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv"));
    }

    void packedFramesCarryTheirColourSpace()
    {
        CameraRawFrame packed = packCameraFrame(nv12Frame(QSize(16, 8), 100));
        QCOMPARE(packed.colorMatrix, QStringLiteral("bt601"));
        QVERIFY(!packed.fullRange);
        packed = packCameraFrame(nv12Frame(QSize(16, 8), 100, 128, 128, QVideoFrameFormat::ColorSpace_BT709));
        QCOMPARE(packed.colorMatrix, QStringLiteral("bt709"));
        QVERIFY(!packed.fullRange);
        packed = packCameraFrame(nv12Frame(QSize(16, 8), 100, 128, 128, QVideoFrameFormat::ColorSpace_AdobeRgb));
        QCOMPARE(packed.colorMatrix, QStringLiteral("bt601"));
        QVERIFY(packed.fullRange);
        packed = packCameraFrame(jpegFrame(QSize(16, 8), Qt::red));
        QCOMPARE(packed.colorMatrix, QStringLiteral("bt601"));
        QVERIFY(packed.fullRange);
    }

    void cameraFramesKeepTheirTimes()
    {
        if (!haveFfmpeg()) QSKIP("ffmpeg is not installed");
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path = directory.filePath(QStringLiteral("camera.mp4"));
        // 45 frames at a jittery 30 fps, half a second of nothing, 30 frames at 15 fps. Each
        // frame's luma says which it is.
        const qint64 zero = 9000000000;
        QList<qint64> times;
        for (int index = 0; index < 45; ++index) times.append(zero + index * 33333 + (index ? (index * 7919) % 4001 - 2000 : 0));
        for (int index = 0; index < 30; ++index) times.append(times[44] + 500000 + index * 66667);
        QList<QPair<QVideoFrame, qint64>> frames;
        for (int index = 0; index < times.size(); ++index)
            frames.append({nv12Frame(QSize(64, 48), uchar(16 + 2 * index)), times[index]});
        const EncodeResult result = encode(path, frames);
        QCOMPARE(result.error, QString());
        QCOMPARE(result.encoded, qint64(times.size()));
        QCOMPARE(result.dropped, qint64(0));

        bool ok = false;
        const QList<QByteArray> pts = run("ffprobe", {"-v", "error", "-select_streams", "v:0", "-show_entries",
                                                      "packet=pts", "-of", "csv=p=0", path}, &ok).trimmed().split('\n');
        QVERIFY(ok);
        QCOMPARE(pts.size(), times.size());
        for (int index = 0; index < times.size(); ++index) {
            const qint64 expected = std::llround((times[index] - zero) * 0.09);
            if (std::abs(pts[index].toLongLong() - expected) > 1)
                QFAIL(qPrintable(QStringLiteral("frame %1 at %2, expected %3").arg(index).arg(QString::fromLatin1(pts[index])).arg(expected)));
        }

        const QByteArray stream = run("ffprobe", {"-v", "error", "-select_streams", "v:0", "-show_entries",
                                                  "stream=codec_name,pix_fmt,color_space,color_primaries,color_transfer,color_range,start_time",
                                                  "-of", "default=noprint_wrappers=1", path}, &ok);
        QVERIFY(ok);
        for (const char *line : {"codec_name=h264", "pix_fmt=yuv420p", "color_space=bt709", "color_primaries=bt709",
                                 "color_transfer=bt709", "color_range=tv", "start_time=0.000000"})
            QVERIFY2(stream.contains(line), stream.constData());

        // Accurate seeks land on the frame shown at that time, in both rates.
        for (const int index : {20, 44, 60}) {
            const QString seconds = QString::number((times[index] - zero - 1000) / 1e6, 'f', 6);
            const QByteArray picture = run("ffmpeg", {"-v", "error", "-ss", seconds, "-i", path, "-frames:v", "1",
                                                      "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"}, &ok);
            QVERIFY(ok);
            QVERIFY(!picture.isEmpty());
            QVERIFY2(std::abs(int(uchar(picture[0])) - (16 + 2 * index)) <= 3,
                     qPrintable(QStringLiteral("seek to frame %1 found luma %2").arg(index).arg(int(uchar(picture[0])))));
        }

        // Fragmented: an empty moov up front, then a fragment from each keyframe.
        QFile file(path);
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QByteArray bytes = file.readAll();
        const auto boxes = topLevelBoxes(bytes);
        QVERIFY(boxes.size() >= 4);
        QCOMPARE(boxes[0].first, QByteArray("ftyp"));
        QCOMPARE(boxes[1].first, QByteArray("moov"));
        qsizetype fragments = 0, moovs = 0;
        for (const auto &box : boxes) {
            fragments += box.first == "moof";
            moovs += box.first == "moov";
        }
        QVERIFY(fragments >= 2);
        QCOMPARE(moovs, qsizetype(1));

        // A recorder killed while writing the last fragment leaves the ones before it.
        qint64 lastFragment = 0;
        for (const auto &box : boxes)
            if (box.first == "moof") lastFragment = box.second;
        const QString truncated = directory.filePath(QStringLiteral("killed.mp4"));
        QFile cut(truncated);
        QVERIFY(cut.open(QIODevice::WriteOnly));
        cut.write(bytes.left(lastFragment + 16));
        cut.close();
        const int survived = run("ffprobe", {"-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries",
                                             "stream=nb_read_frames", "-of", "csv=p=0", truncated}, &ok).trimmed().toInt();
        QString layout;
        for (const auto &box : boxes) layout += QString::fromLatin1(box.first) + '@' + QString::number(box.second) + ' ';
        QVERIFY2(survived >= 30 && survived < times.size(),
                 qPrintable(QStringLiteral("%1 frames survived; %2 bytes: %3").arg(survived).arg(bytes.size()).arg(layout)));
    }

    void everyPackedFormatEncodes_data()
    {
        QTest::addColumn<int>("format");
        using F = QVideoFrameFormat;
        for (const auto format : {F::Format_NV12, F::Format_NV21, F::Format_YUV420P, F::Format_YV12,
                                  F::Format_YUV422P, F::Format_UYVY, F::Format_YUYV, F::Format_BGRA8888,
                                  F::Format_RGBA8888, F::Format_ARGB8888, F::Format_ABGR8888})
            QTest::newRow(qPrintable(F::pixelFormatToString(format))) << int(format);
    }

    void everyPackedFormatEncodes()
    {
        // Every byte 128 is mid grey in each of them: Matroska names each format so that
        // ffmpeg reads it back as what it is.
        if (!haveFfmpeg()) QSKIP("ffmpeg is not installed");
        QFETCH(int, format);
        const QSize size(64, 48);
        QVideoFrame frame(QVideoFrameFormat(size, QVideoFrameFormat::PixelFormat(format)));
        QVERIFY(frame.map(QVideoFrame::WriteOnly));
        for (int plane = 0; plane < frame.planeCount(); ++plane) std::memset(frame.bits(plane), 128, frame.mappedBytes(plane));
        frame.unmap();
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path = directory.filePath(QStringLiteral("camera.mp4"));
        const EncodeResult result = encode(path, {{frame, 1000000}, {frame, 1033333}});
        QCOMPARE(result.error, QString());
        QCOMPARE(result.encoded, qint64(2));
        bool ok = false;
        const QByteArray rgb = run("ffmpeg", {"-v", "error", "-i", path, "-frames:v", "1", "-f", "rawvideo",
                                              "-pix_fmt", "rgb24", "-"}, &ok);
        QVERIFY(ok);
        QCOMPARE(rgb.size(), qsizetype(size.width() * size.height() * 3));
        for (int channel = 0; channel < 3; ++channel)
            QVERIFY2(std::abs(int(uchar(rgb[channel])) - 129) <= 6, qPrintable(QString::number(uchar(rgb[channel]))));
    }

    void cameraColoursAreBt709_data()
    {
        QTest::addColumn<QString>("source");
        QTest::newRow("yuyv bt601") << QString("yuyv");
        QTest::newRow("nv12 bt709") << QString("nv12-709");
        QTest::newRow("bgra") << QString("bgra");
        QTest::newRow("mjpeg") << QString("jpeg");
    }

    void cameraColoursAreBt709()
    {
        // Whatever the camera sent, the file decodes to the colour it showed. BT.601 read as
        // BT.709, or full range as limited, misses this red by far more than the tolerance.
        if (!haveFfmpeg()) QSKIP("ffmpeg is not installed");
        QFETCH(QString, source);
        const QColor colour(200, 60, 40);
        const QSize size(64, 48);
        QVideoFrame frame;
        if (source == "yuyv") frame = yuyvFrame(size, yuv(colour, false));
        else if (source == "bgra") frame = bgraFrame(size, colour);
        else if (source == "jpeg") frame = jpegFrame(size, colour);
        else {
            const auto value = yuv(colour, true);
            frame = nv12Frame(size, value[0], value[1], value[2], QVideoFrameFormat::ColorSpace_BT709);
        }
        QVERIFY(frame.isValid());
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path = directory.filePath(QStringLiteral("camera.mp4"));
        const EncodeResult result = encode(path, {{frame, 1000000}, {frame, 1033333}, {frame, 1066666}});
        QCOMPARE(result.error, QString());
        QCOMPARE(result.encoded, qint64(3));
        bool ok = false;
        const QByteArray rgb = run("ffmpeg", {"-v", "error", "-i", path, "-frames:v", "1", "-f", "rawvideo",
                                              "-pix_fmt", "rgb24", "-"}, &ok);
        QVERIFY(ok);
        QCOMPARE(rgb.size(), qsizetype(size.width() * size.height() * 3));
        const qsizetype centre = (size.height() / 2 * size.width() + size.width() / 2) * 3;
        const int decoded[] = {uchar(rgb[centre]), uchar(rgb[centre + 1]), uchar(rgb[centre + 2])};
        const int expected[] = {colour.red(), colour.green(), colour.blue()};
        for (int channel = 0; channel < 3; ++channel)
            QVERIFY2(std::abs(decoded[channel] - expected[channel]) <= 6,
                     qPrintable(QStringLiteral("decoded %1,%2,%3").arg(decoded[0]).arg(decoded[1]).arg(decoded[2])));
    }

    void cameraPlanesAreTightlyPacked_data()
    {
        QTest::addColumn<int>("format");
        QTest::addColumn<QString>("ffmpegFormat");
        QTest::newRow("nv12") << int(QVideoFrameFormat::Format_NV12) << QString("nv12");
        QTest::newRow("420") << int(QVideoFrameFormat::Format_YUV420P) << QString("yuv420p");
        QTest::newRow("422") << int(QVideoFrameFormat::Format_YUV422P) << QString("yuv422p");
        QTest::newRow("bgra") << int(QVideoFrameFormat::Format_BGRA8888) << QString("bgra");
        QTest::newRow("rgba") << int(QVideoFrameFormat::Format_RGBA8888) << QString("rgba");
    }

    void cameraPlanesAreTightlyPacked()
    {
        QFETCH(int, format);
        QFETCH(QString, ffmpegFormat);
        // Width 14 forces padded rows in Qt's allocated video buffer.
        QVideoFrame frame(QVideoFrameFormat(QSize(14, 6), QVideoFrameFormat::PixelFormat(format)));
        QVERIFY(frame.map(QVideoFrame::WriteOnly));
        QByteArray expected;
        for (int plane = 0; plane < frame.planeCount(); ++plane) {
            const bool rgb = ffmpegFormat == "bgra" || ffmpegFormat == "rgba";
            const int width = rgb ? 56 : plane == 0 || ffmpegFormat == "nv12" ? 14 : 7;
            const int rows = plane == 0 || ffmpegFormat == "yuv422p" ? 6 : 3;
            std::memset(frame.bits(plane), 0xee, frame.mappedBytes(plane));
            for (int row = 0; row < rows; ++row) {
                const char value = char(16 + plane*20 + row);
                std::memset(frame.bits(plane) + row*frame.bytesPerLine(plane), value, width);
                expected += QByteArray(width, value);
            }
        }
        frame.unmap();
        const auto packed = packCameraFrame(frame);
        QCOMPARE(packed.size, QSize(14,6));
        QCOMPARE(packed.pixelFormat, ffmpegFormat);
        QCOMPARE(packed.pixels, expected);
    }

    void cameraJpegDecodesDirectlyToYuv()
    {
        QImage source(16, 8, QImage::Format_RGB32);
        source.fill(Qt::red);
        QByteArray jpeg;
        QBuffer buffer(&jpeg);
        QVERIFY(buffer.open(QIODevice::WriteOnly));
        QVERIFY(source.save(&buffer, "JPEG", 95));
        QVideoFrame frame(std::make_unique<JpegTestBuffer>(jpeg));
        const auto packed = packCameraFrame(frame);
        QCOMPARE(packed.size, QSize(16, 8));
        QCOMPARE(packed.pixelFormat, QString("yuvj444p"));
        QCOMPARE(packed.pixels.size(), 16*8*3);
        // Full-range JPEG red: Y approximately 76, Cb 85, Cr 255.
        QVERIFY(std::abs(int(uchar(packed.pixels[0])) - 76) < 4);
        QVERIFY(std::abs(int(uchar(packed.pixels[128])) - 85) < 4);
        QVERIFY(int(uchar(packed.pixels[256])) > 250);
        QVideoFrame broken(std::make_unique<JpegTestBuffer>(QByteArray("invalid JPEG")));
        QVERIFY(packCameraFrame(broken).pixels.isEmpty());
    }

    void cameraNodesFollowSavedPaths()
    {
        // The node a saved /dev/v4l link points at now, and the node in a Qt camera id, which
        // cameraDeviceForPath matches to pick the QCameraDevice to open.
        QTemporaryDir root;
        QVERIFY(root.isValid());
        const QString node = root.filePath(QStringLiteral("video4"));
        QFile file(node);
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.close();
        const QString saved = root.filePath(QStringLiteral("usb-Insta360_Insta360_Link_2-video-index0"));
        QVERIFY(QFile::link(node, saved));
        QCOMPARE(cameraNodePath(saved), QFileInfo(node).canonicalFilePath());
        QCOMPARE(cameraNodePath(node), QFileInfo(node).canonicalFilePath());
        // Unplugged: the link dangles and there is no node.
        QVERIFY(QFile::remove(node));
        QCOMPARE(cameraNodePath(saved), QString());

        QCOMPARE(cameraIdNode("/dev/video6"), QStringLiteral("/dev/video6"));
        QCOMPARE(cameraIdNode("v4l2 /dev/video6 usb"), QStringLiteral("/dev/video6"));
        QCOMPARE(cameraIdNode("obs-virtual"), QString());
    }
};

QTEST_GUILESS_MAIN(CameraCaptureTest)
#include "test_cameracapture.moc"
