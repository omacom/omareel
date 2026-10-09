#include "record/CaptureExclusion.h"
#include "record/Recorder.h"
#include "record/CameraCapture.h"
#include "record/ScreenCapture.h"
#include "core/RecordingPreferences.h"
#include "ui/RecordingBar.h"

#include <QtTest>
#include <QTemporaryDir>
#include <QScopeGuard>
#include <limits>
#include <QVideoFrame>
#include <QAbstractVideoBuffer>
#include <QBuffer>
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

class RecorderTest : public QObject
{
    Q_OBJECT
private slots:
    void classifiesGsrExit_data()
    {
        QTest::addColumn<int>("exitCode");
        QTest::addColumn<bool>("stopRequested");
        QTest::addColumn<qint64>("fileSize");
        QTest::addColumn<double>("duration");
        QTest::addColumn<int>("expected");

        using Classification = Recorder::GsrExitClassification;
        QTest::newRow("requested clean stop") << 0 << true << qint64(1024) << 1.5 << int(Classification::UserStop);
        QTest::newRow("external clean stop") << 0 << false << qint64(1024) << 1.5 << int(Classification::ExternalStop);
        QTest::newRow("non-zero exit") << 1 << false << qint64(1024) << 1.5 << int(Classification::Failure);
        QTest::newRow("empty file") << 0 << false << qint64(0) << 1.5 << int(Classification::Failure);
        QTest::newRow("zero duration") << 0 << false << qint64(1024) << 0.0 << int(Classification::Failure);
        QTest::newRow("invalid duration") << 0 << false << qint64(1024)
                                               << std::numeric_limits<double>::quiet_NaN()
                                               << int(Classification::Failure);
    }

    void classifiesGsrExit()
    {
        QFETCH(int, exitCode);
        QFETCH(bool, stopRequested);
        QFETCH(qint64, fileSize);
        QFETCH(double, duration);
        QFETCH(int, expected);
        QCOMPARE(int(Recorder::classifyGsrExit(exitCode, stopRequested, fileSize, duration)), expected);
    }

    void trimsCaptureRegionToEvenSize()
    {
        CaptureRegion region;
        region.x = 10.0;
        region.y = 20.0;
        region.width = 1521.0;
        region.height = 835.0;
        region.physicalWidth = 1521;
        region.physicalHeight = 835;
        const CaptureRegion trimmed = Recorder::evenCaptureRegion(region);
        QCOMPARE(trimmed.physicalWidth, 1520);
        QCOMPARE(trimmed.physicalHeight, 834);
        QCOMPARE(trimmed.width, 1520.0);
        QCOMPARE(trimmed.height, 834.0);
        QCOMPARE(trimmed.x, 10.0);
        QCOMPARE(trimmed.y, 20.0);

        region.scale = 2.0;
        region.width = 760.5;
        region.height = 418.0;
        region.physicalWidth = 1521;
        region.physicalHeight = 836;
        const CaptureRegion scaled = Recorder::evenCaptureRegion(region);
        QCOMPARE(scaled.physicalWidth, 1520);
        QCOMPARE(scaled.physicalHeight, 836);
        QCOMPARE(scaled.width, 760.0);
        QCOMPARE(scaled.height, 418.0);

        region.scale = 1.0;
        region.width = 1520.0;
        region.height = 836.0;
        region.physicalWidth = 1520;
        region.physicalHeight = 836;
        const CaptureRegion even = Recorder::evenCaptureRegion(region);
        QCOMPARE(even.physicalWidth, 1520);
        QCOMPARE(even.physicalHeight, 836);
        QCOMPARE(even.width, 1520.0);
        QCOMPARE(even.height, 836.0);
    }

    void recordingPreferencesRoundTrip()
    {
        QTemporaryDir config;
        QVERIFY(config.isValid());
        qputenv("XDG_CONFIG_HOME", config.path().toUtf8());
        RecordingPreferences preferences = RecordingPreferences::load();
        QVERIFY(preferences.systemAudio);
        QVERIFY(preferences.microphone);
        QCOMPARE(preferences.microphoneDevice, QStringLiteral("default_input"));
        QCOMPARE(preferences.webcam.value(QStringLiteral("enabled")).toBool(), false);
        QVERIFY(!preferences.webcamEnabled);
        QCOMPARE(preferences.webcamDevice, QStringLiteral("/dev/video2"));
        QCOMPARE(preferences.webcamHeight, 1080);
        QVERIFY(!preferences.countdownBeforeRecording);
        preferences.systemAudio = false;
        preferences.microphoneDevice = QStringLiteral("test_input");
        preferences.webcamEnabled = true;
        preferences.webcamDevice = QStringLiteral("/dev/video8");
        preferences.webcamHeight = 720;
        preferences.webcamRotation = 270;
        preferences.webcamFlipHorizontal = true;
        preferences.countdownBeforeRecording = true;
        QString error;
        QVERIFY2(preferences.save(&error), qPrintable(error));
        const RecordingPreferences loaded = RecordingPreferences::load();
        QVERIFY(!loaded.systemAudio);
        QCOMPARE(loaded.microphoneDevice, QStringLiteral("test_input"));
        QCOMPARE(loaded.webcam.value(QStringLiteral("shape")).toString(), QStringLiteral("round"));
        QVERIFY(loaded.webcamEnabled);
        QCOMPARE(loaded.webcamDevice, QStringLiteral("/dev/video8"));
        QCOMPARE(loaded.webcamHeight, 720);
        QCOMPARE(loaded.webcamRotation, 270);
        QVERIFY(loaded.webcamFlipHorizontal);
        QVERIFY(loaded.countdownBeforeRecording);
    }

    void migratesLegacyRecordingPreferencesByCopy()
    {
        QTemporaryDir config;
        QVERIFY(config.isValid());
        qputenv("XDG_CONFIG_HOME", config.path().toUtf8());
        const QString legacyPath = config.filePath(QStringLiteral("omarecord/settings.json"));
        QVERIFY(QDir().mkpath(QFileInfo(legacyPath).absolutePath()));
        QFile legacy(legacyPath);
        QVERIFY(legacy.open(QIODevice::WriteOnly));
        legacy.write("{\"systemAudio\":false,\"microphone\":false}");
        legacy.close();

        const RecordingPreferences loaded = RecordingPreferences::load();
        QVERIFY(!loaded.systemAudio);
        QVERIFY(!loaded.microphone);
        QVERIFY(QFileInfo::exists(legacyPath));
        QVERIFY(QFileInfo::exists(config.filePath(QStringLiteral("omareel/settings.json"))));
    }

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

    void selfViewHostLifetime()
    {
        for (bool active : {false, true}) {
            for (bool quit : {false, true}) {
                for (qint64 owner : {qint64(-1), qint64(0), qint64(42)}) {
                    for (bool alive : {false, true}) {
                        const bool expected = active ? false : quit || owner <= 0 || !alive;
                        QCOMPARE(selfViewHostShouldExit(active, quit, owner, alive), expected);
                    }
                }
            }
        }
        // Completing an adoption changes only active: a dead owner must exit,
        // a live owner returns to standby, and quit waits for recording to finish.
        QVERIFY(!selfViewHostShouldExit(true, false, 42, false));
        QVERIFY(selfViewHostShouldExit(false, false, 42, false));
        QVERIFY(!selfViewHostShouldExit(false, false, 42, true));
        QVERIFY(!selfViewHostShouldExit(true, true, 42, true));
        QVERIFY(selfViewHostShouldExit(false, true, 42, true));
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

    void cameraCaptureBlockIncludesSettings()
    {
        const QJsonObject camera = Recorder::cameraCaptureBlock(
            QStringLiteral("/dev/video2"), 1080, 1920, 1080, 30.0, 1234567,
            QStringLiteral("ffmpeg-pipe"), 90, true);
        QCOMPARE(camera.value(QStringLiteral("backend")).toString(), QStringLiteral("ffmpeg-pipe"));
        QCOMPARE(camera.value(QStringLiteral("requestedHeight")).toInt(), 1080);
        QCOMPARE(camera.value(QStringLiteral("width")).toInt(), 1920);
        QCOMPARE(camera.value(QStringLiteral("height")).toInt(), 1080);
        QCOMPARE(camera.value(QStringLiteral("fps")).toDouble(), 30.0);
        QCOMPARE(camera.value(QStringLiteral("first_frame_us")).toVariant().toLongLong(), qint64(1234567));
        QCOMPARE(camera.value(QStringLiteral("rotation")).toInt(), 90);
        QVERIFY(camera.value(QStringLiteral("flipHorizontal")).toBool());
    }

    void discardTimelineNeverDrains()
    {
        QStringList calls;
        int cameraWaitMs = -1;
        const bool removed = Recorder::discardTimeline(Recorder::DiscardActions{
            [&] { calls << QStringLiteral("abort-capture"); },
            [&] { calls << QStringLiteral("kill-audio"); },
            [&] { calls << QStringLiteral("request-camera-stop"); },
            [&](int timeoutMs) {
                cameraWaitMs = timeoutMs;
                calls << QStringLiteral("wait-camera");
            },
            [&] {
                calls << QStringLiteral("remove-bundle");
                return true;
            },
            [&] { calls << QStringLiteral("remove-state"); }
        });

        QVERIFY(removed);
        QCOMPARE(cameraWaitMs, 1000);
        QCOMPARE(calls, QStringList({QStringLiteral("abort-capture"),
                                    QStringLiteral("kill-audio"),
                                    QStringLiteral("request-camera-stop"),
                                    QStringLiteral("wait-camera"),
                                    QStringLiteral("remove-bundle"),
                                    QStringLiteral("remove-state")}));
        QVERIFY(!calls.contains(QStringLiteral("drain-capture")));
        QVERIFY(!calls.contains(QStringLiteral("mux-audio")));
    }

    void captureRingTracksSlotsAndDrops()
    {
        CaptureRingBookkeeping ring(3);
        const int first = ring.acquire();
        const int second = ring.acquire();
        const int third = ring.acquire();
        QCOMPARE(first, 0);
        QCOMPARE(second, 1);
        QCOMPARE(third, 2);
        QCOMPARE(ring.count(CaptureRingBookkeeping::State::Capturing), 3);
        QCOMPARE(ring.acquire(), -1);
        QCOMPARE(ring.drops(), 1);

        QVERIFY(ring.markQueued(second));
        QVERIFY(ring.markQueued(first));
        QCOMPARE(ring.takeQueued(), second);
        QCOMPARE(ring.takeQueued(), first);
        QCOMPARE(ring.state(second), CaptureRingBookkeeping::State::Writing);
        QVERIFY(ring.release(second));
        QCOMPARE(ring.state(second), CaptureRingBookkeeping::State::Available);
        QCOMPARE(ring.acquire(), second);
        QVERIFY(!ring.markQueued(first + 8));
    }

    void captureRegionRowCropMath()
    {
        const QVector<CaptureRowCopy> rows = captureCropRows(
            QSize(3840, 2160), 3840 * 4, QRect(320, 240, 1280, 960));
        QCOMPARE(rows.size(), 960);
        QCOMPARE(rows.first().sourceOffset, qsizetype(240 * 3840 * 4 + 320 * 4));
        QCOMPARE(rows.first().destinationOffset, qsizetype(0));
        QCOMPARE(rows.first().bytes, qsizetype(1280 * 4));
        QCOMPARE(rows.last().sourceOffset, qsizetype((240 + 959) * 3840 * 4 + 320 * 4));
        QCOMPARE(rows.last().destinationOffset, qsizetype(959 * 1280 * 4));

        const QVector<CaptureRowCopy> full = captureCropRows(
            QSize(3840, 2160), 3840 * 4, QRect(0, 0, 3840, 2160));
        QCOMPARE(full.size(), 1);
        QCOMPARE(full.first().sourceOffset, qsizetype(0));
        QCOMPARE(full.first().bytes, qsizetype(3840) * 2160 * 4);
    }

    void captureStreamKeepsFrameTimesAndColours()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString video = directory.filePath(QStringLiteral("screen.mp4"));
        const QSize size(64, 32);
        // A capture that stalls between 0.117 s and 0.5 s must not close that gap up.
        const QList<qint64> timesUs{0, 16667, 100000, 116667, 500000};
        QByteArray stream = captureStreamHeader(size, 60);
        for (const qint64 timeUs : timesUs) {
            QByteArray frame;
            for (int pixel = 0; pixel < size.width() * size.height(); ++pixel)
                frame += QByteArray::fromHex("c8320aff"); // B, G, R, X
            stream += captureFrameHeader(timeUs, frame.size()) + frame;
        }
        QProcess encoder;
        encoder.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("matroska"), QStringLiteral("-i"), QStringLiteral("pipe:0"),
            QStringLiteral("-c:v"), QStringLiteral("libx264"), QStringLiteral("-bf"), QStringLiteral("0"),
            QStringLiteral("-pix_fmt"), QStringLiteral("yuv420p"), QStringLiteral("-fps_mode"),
            QStringLiteral("vfr"), QStringLiteral("-enc_time_base"), QStringLiteral("1/60"), video});
        QVERIFY(encoder.waitForStarted(5000));
        encoder.write(stream);
        encoder.closeWriteChannel();
        QVERIFY(encoder.waitForFinished(30000));
        QVERIFY2(encoder.exitCode() == 0, encoder.readAllStandardError().constData());

        QProcess probe;
        probe.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-select_streams"), QStringLiteral("v"), QStringLiteral("-show_entries"),
            QStringLiteral("packet=pts_time:format=duration"), QStringLiteral("-of"),
            QStringLiteral("csv=p=0"), video});
        QVERIFY(probe.waitForFinished(10000));
        const QStringList lines = QString::fromUtf8(probe.readAllStandardOutput())
            .split(QLatin1Char('\n'), Qt::SkipEmptyParts);
        QCOMPARE(lines.size(), timesUs.size() + 1);
        for (int index = 0; index < timesUs.size(); ++index)
            QVERIFY2(qAbs(lines[index].toDouble() - timesUs[index] / 1e6) < 0.001, qPrintable(lines[index]));
        QVERIFY2(qAbs(lines.last().toDouble() - (0.5 + 1.0 / 60.0)) < 0.001, qPrintable(lines.last()));

        QProcess decoder;
        decoder.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-i"), video, QStringLiteral("-frames:v"), QStringLiteral("1"),
            QStringLiteral("-pix_fmt"), QStringLiteral("rgb24"), QStringLiteral("-f"),
            QStringLiteral("rawvideo"), QStringLiteral("-")});
        QVERIFY(decoder.waitForFinished(10000));
        const QByteArray rgb = decoder.readAllStandardOutput();
        QVERIFY(rgb.size() >= 3);
        QVERIFY2(qAbs(uchar(rgb[0]) - 10) <= 4 && qAbs(uchar(rgb[1]) - 50) <= 4
                     && qAbs(uchar(rgb[2]) - 200) <= 4,
                 qPrintable(QStringLiteral("%1,%2,%3").arg(uchar(rgb[0])).arg(uchar(rgb[1])).arg(uchar(rgb[2]))));
    }

    void captureExclusionRequiresExactValidHash()
    {
        const QString hash(40, 'a');
        QVERIFY(CaptureExclusion::compatibleHash(hash, hash));
        QVERIFY(!CaptureExclusion::compatibleHash("", ""));
        QVERIFY(!CaptureExclusion::compatibleHash("unknown", "unknown"));
        QVERIFY(!CaptureExclusion::compatibleHash(hash, QString(40, 'b')));
    }

    void captureExclusionPlacesOverlaysAwayInFallback()
    {
        const QStringList monitors{"DP-3", "DP-5"};
        QCOMPARE(CaptureExclusion::overlayMonitor(true, "DP-3", monitors), "DP-3");
        QCOMPARE(CaptureExclusion::overlayMonitor(false, "DP-3", monitors), "DP-5");
        QCOMPARE(CaptureExclusion::overlayMonitor(false, "DP-5", monitors), "DP-3");
        QVERIFY(CaptureExclusion::overlayMonitor(false, "DP-3", {"DP-3"}).isEmpty());
        QVERIFY(CaptureExclusion::overlayMonitor(false, "DP-3", {}).isEmpty());
    }

    void captureExclusionChecksHashBeforeLoading()
    {
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QByteArray oldPath = qgetenv("PATH");
        const QByteArray oldPlugin = qgetenv("OMAREEL_PLUGIN_PATH");
        const auto restore = qScopeGuard([&] {
            qputenv("PATH", oldPath);
            if (oldPlugin.isNull()) qunsetenv("OMAREEL_PLUGIN_PATH");
            else qputenv("OMAREEL_PLUGIN_PATH", oldPlugin);
        });
        const QString plugin = temporary.filePath("plugin.so");
        QFile binary(plugin);
        QVERIFY(binary.open(QIODevice::WriteOnly));
        binary.write("fixture");
        binary.close();
        const QString logPath = temporary.filePath("calls");
        QFile mock(temporary.filePath("hyprctl"));
        QVERIFY(mock.open(QIODevice::WriteOnly));
        mock.write(("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '" + logPath + "'\n"
                    "case \"$*\" in\n"
                    " '-j plugin list') printf '[]';;\n"
                    " '-j version') printf '{\"commit\":\"" + QString(40, 'a') + "\"}';;\n"
                    " *) printf 'Plugin could not be loaded';;\nesac\n").toUtf8());
        mock.close();
        QVERIFY(mock.setPermissions(QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner));
        qputenv("PATH", temporary.path().toUtf8());
        qputenv("OMAREEL_PLUGIN_PATH", plugin.toUtf8());
        QFile sidecar(plugin + ".hash");
        QVERIFY(sidecar.open(QIODevice::WriteOnly));
        sidecar.write(QByteArray(40, 'b'));
        sidecar.close();
        const auto mismatch = CaptureExclusion::ensureLoaded();
        QVERIFY(!mismatch.loaded);
        QVERIFY(mismatch.reason.contains("rebuilding"));
        QFile log(logPath);
        QVERIFY(log.open(QIODevice::ReadOnly));
        QVERIFY(!log.readAll().contains("plugin load"));
        log.close();
        QVERIFY(sidecar.open(QIODevice::WriteOnly | QIODevice::Truncate));
        sidecar.write(QByteArray(40, 'a'));
        sidecar.close();
        const auto refused = CaptureExclusion::ensureLoaded();
        QVERIFY(!refused.loaded);
        QVERIFY(refused.reason.contains("refused"));
        QVERIFY(log.open(QIODevice::ReadOnly));
        QVERIFY(log.readAll().contains("plugin load"));
        qputenv("OMAREEL_PLUGIN_PATH", temporary.filePath("absent.so").toUtf8());
        QVERIFY(CaptureExclusion::ensureLoaded().reason.contains("does not exist"));
    }

    void selfViewDragUsesStableGlobalCoordinates()
    {
        const QSize screenSize(2400, 1350);
        QCOMPARE(clampedSelfViewDragPosition(QPointF(100, 100), QPoint(400, 300),
                                             QPointF(160, 140), screenSize, 160),
                 QPoint(460, 340));
        QCOMPARE(clampedSelfViewDragPosition(QPointF(100, 100), QPoint(2200, 1200),
                                             QPointF(500, 500), screenSize, 160),
                 QPoint(2240, 1190));
        QCOMPARE(clampedSelfViewDragPosition(QPointF(100, 100), QPoint(20, 20),
                                             QPointF(-100, -100), screenSize, 160),
                 QPoint(0, 0));
    }
};

QTEST_APPLESS_MAIN(RecorderTest)
#include "test_recorder.moc"
