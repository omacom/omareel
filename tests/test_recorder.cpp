#include "record/CaptureExclusion.h"
#include "CameraCapture.h"
#include "core/RecordingPreferences.h"

#include <QtTest>
#include <QTemporaryDir>
#include <QScopeGuard>
#include <limits>
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

class RecorderTest : public QObject
{
    Q_OBJECT
private slots:
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
        QCOMPARE(preferences.webcamChosenDevice, preferences.webcamDevice);
        QCOMPARE(preferences.webcamHeight, 1080);
        QVERIFY(!preferences.countdownBeforeRecording);
        preferences.systemAudio = false;
        preferences.microphoneDevice = QStringLiteral("test_input");
        preferences.webcamEnabled = true;
        preferences.webcamDevice = QStringLiteral("/dev/video8");
        preferences.webcamChosenDevice = QStringLiteral("/dev/v4l/by-id/usb-Insta360_Insta360_Link_2-video-index0");
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
        QCOMPARE(loaded.webcamChosenDevice,
                 QStringLiteral("/dev/v4l/by-id/usb-Insta360_Insta360_Link_2-video-index0"));
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

    void recordingHoldsHardwareCursors()
    {
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QByteArray oldPath = qgetenv("PATH");
        const auto restore = qScopeGuard([&] {
            qputenv("PATH", oldPath);
            qunsetenv("OMAREEL_TEST_CURSOR");
            qunsetenv("OMAREEL_TEST_EVAL_EXIT");
        });
        const QString logPath = temporary.filePath("calls");
        QFile mock(temporary.filePath("hyprctl"));
        QVERIFY(mock.open(QIODevice::WriteOnly));
        mock.write(("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '" + logPath + "'\n"
                    "case \"$*\" in\n"
                    " '-j getoption cursor:no_hardware_cursors') printf '{\"option\": \"cursor:no_hardware_cursors\", "
                    "\"int\": %s, \"set\": false }' \"$OMAREEL_TEST_CURSOR\";;\n"
                    " eval*) exit \"${OMAREEL_TEST_EVAL_EXIT:-0}\";;\nesac\n").toUtf8());
        mock.close();
        QVERIFY(mock.setPermissions(QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner));
        qputenv("PATH", temporary.path().toUtf8());
        const auto calls = [&] {
            QFile log(logPath);
            const QStringList lines = log.open(QIODevice::ReadOnly)
                ? QString::fromUtf8(log.readAll()).split('\n', Qt::SkipEmptyParts) : QStringList();
            log.remove();
            return lines;
        };

        // Hyprland's automatic choice is held at hardware and put back afterwards.
        qputenv("OMAREEL_TEST_CURSOR", "2");
        { const HardwareCursorHold hold; }
        QCOMPARE(calls(), QStringList({"-j getoption cursor:no_hardware_cursors",
            "eval hl.config({ cursor = { no_hardware_cursors = 0 } })",
            "eval hl.config({ cursor = { no_hardware_cursors = 2 } })"}));
        // Hardware cursors already: nothing to change.
        qputenv("OMAREEL_TEST_CURSOR", "0");
        { const HardwareCursorHold hold; }
        QCOMPARE(calls(), QStringList({"-j getoption cursor:no_hardware_cursors"}));
        // A hyprlang config takes keyword instead of eval.
        qputenv("OMAREEL_TEST_CURSOR", "1");
        qputenv("OMAREEL_TEST_EVAL_EXIT", "1");
        { const HardwareCursorHold hold; }
        QCOMPARE(calls(), QStringList({"-j getoption cursor:no_hardware_cursors",
            "eval hl.config({ cursor = { no_hardware_cursors = 0 } })",
            "keyword cursor:no_hardware_cursors 0",
            "eval hl.config({ cursor = { no_hardware_cursors = 1 } })",
            "keyword cursor:no_hardware_cursors 1"}));
    }
};

QTEST_APPLESS_MAIN(RecorderTest)
#include "test_recorder.moc"
