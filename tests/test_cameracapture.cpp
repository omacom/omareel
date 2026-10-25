#include "CameraCapture.h"

#include <QtTest>
#include <QTemporaryDir>
#include <QVideoFrame>
#include <QAbstractVideoBuffer>
#include <QBuffer>
#include <cmath>
#include <cstring>

using namespace Omareel;

class JpegTestBuffer : public QAbstractVideoBuffer
{
public:
    explicit JpegTestBuffer(QByteArray bytes): data(std::move(bytes)) {}
    MapData map(QVideoFrame::MapMode) override
    {
        MapData mapped;
        mapped.planeCount = 1;
        mapped.data[0] = reinterpret_cast<uchar *>(data.data());
        mapped.dataSize[0] = data.size();
        return mapped;
    }
    QVideoFrameFormat format() const override
    { return QVideoFrameFormat(QSize(16, 8), QVideoFrameFormat::Format_Jpeg); }
    QByteArray data;
};

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

    void cameraPacingDoesNotAccumulateMillisecondRounding()
    {
        const qint64 first = 1234567;
        QCOMPARE(cameraFrameDeadlineUs(first, 0), first);
        QCOMPARE(cameraFrameDeadlineUs(first, 1), first + 33333);
        QCOMPARE(cameraFrameDeadlineUs(first, 2), first + 66666);
        QCOMPARE(cameraFrameDeadlineUs(first, 240), first + 8000000);
        QCOMPARE(cameraFrameDeadlineUs(first, 108000), first + 3600000000LL);
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

QTEST_APPLESS_MAIN(CameraCaptureTest)
#include "test_cameracapture.moc"
