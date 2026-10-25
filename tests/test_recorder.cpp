#include "record/CaptureExclusion.h"
#include "record/CameraCapture.h"
#include "core/RecordingPreferences.h"
#include "ui/MicrophoneMeter.h"
#include "ui/RecordingBar.h"

#include <QtTest>
#include <QTemporaryDir>
#include <QScopeGuard>
#include <limits>
#include <QVideoFrame>
#include <QAbstractVideoBuffer>
#include <QBuffer>
#include <QAudioFormat>
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

    void cameraPathsSurviveRenumbering()
    {
        // A laptop webcam whose IR sensor shares its USB serial, so both index-0 nodes claim one
        // by-id name and udev left it on the IR node, and an Insta360 on a dock.
        QTemporaryDir root;
        QVERIFY(root.isValid());
        const QString dev = root.path() + QStringLiteral("/dev");
        const QString sys = root.path() + QStringLiteral("/sys");
        const auto write = [](const QString &path, const QByteArray &contents) {
            QDir().mkpath(QFileInfo(path).absolutePath());
            QFile file(path);
            QVERIFY(file.open(QIODevice::WriteOnly));
            file.write(contents);
        };
        const auto link = [](const QString &target, const QString &path) {
            QDir().mkpath(QFileInfo(path).absolutePath());
            QFile::remove(path);
            QVERIFY(QFile::link(target, path));
        };
        const auto node = [&](const QString &name, const QString &interface, int index) {
            write(dev + QLatin1Char('/') + name, {});
            QDir().mkpath(sys + QStringLiteral("/devices/") + interface);
            const QString classDirectory = sys + QStringLiteral("/class/video4linux/") + name;
            write(classDirectory + QStringLiteral("/index"), QByteArray::number(index) + '\n');
            link(sys + QStringLiteral("/devices/") + interface, classDirectory + QStringLiteral("/device"));
        };
        write(sys + QStringLiteral("/devices/usb7/7-1/idVendor"), "636e\n");
        write(sys + QStringLiteral("/devices/usb7/7-1/idProduct"), "0bdb\n");
        write(sys + QStringLiteral("/devices/usb7/7-1/serial"), "200901010001\n");
        write(sys + QStringLiteral("/devices/usb1/1-1.1/idVendor"), "2e1a\n");
        write(sys + QStringLiteral("/devices/usb1/1-1.1/idProduct"), "4c04\n");
        node(QStringLiteral("video0"), QStringLiteral("usb7/7-1/7-1:1.0"), 0);
        node(QStringLiteral("video1"), QStringLiteral("usb7/7-1/7-1:1.0"), 1);
        node(QStringLiteral("video2"), QStringLiteral("usb7/7-1/7-1:1.2"), 0);
        node(QStringLiteral("video4"), QStringLiteral("usb1/1-1.1/1-1.1:1.0"), 0);
        const QString byId = dev + QStringLiteral("/v4l/by-id/");
        const QString byPath = dev + QStringLiteral("/v4l/by-path/");
        link(dev + QStringLiteral("/video2"), byId + QStringLiteral("usb-ASUS_FHD_webcam_200901010001-video-index0"));
        link(dev + QStringLiteral("/video1"), byId + QStringLiteral("usb-ASUS_FHD_webcam_200901010001-video-index1"));
        link(dev + QStringLiteral("/video4"), byId + QStringLiteral("usb-Insta360_Insta360_Link_2-video-index0"));
        link(dev + QStringLiteral("/video0"), byPath + QStringLiteral("platform-x-usbv2-0:1:1.0-video-index0"));
        link(dev + QStringLiteral("/video0"), byPath + QStringLiteral("platform-x-usb-0:1:1.0-video-index0"));
        link(dev + QStringLiteral("/video2"), byPath + QStringLiteral("platform-x-usb-0:1:1.2-video-index0"));
        link(dev + QStringLiteral("/video4"), byPath + QStringLiteral("platform-y-usb-0:1.1:1.0-video-index0"));

        const QString insta360 = stableCameraPath(dev + QStringLiteral("/video4"), dev, sys);
        QCOMPARE(insta360, byId + QStringLiteral("usb-Insta360_Insta360_Link_2-video-index0"));
        QCOMPARE(stableCameraPath(dev + QStringLiteral("/video0"), dev, sys),
                 byPath + QStringLiteral("platform-x-usb-0:1:1.0-video-index0"));
        QCOMPARE(stableCameraPath(dev + QStringLiteral("/video2"), dev, sys),
                 byPath + QStringLiteral("platform-x-usb-0:1:1.2-video-index0"));
        QCOMPARE(stableCameraPath(insta360, dev, sys), insta360);
        QCOMPARE(cameraNodePath(insta360), QFileInfo(dev + QStringLiteral("/video4")).canonicalFilePath());

        // The dock comes back with a capture card ahead of the camera.
        QFile::remove(dev + QStringLiteral("/video4"));
        node(QStringLiteral("video6"), QStringLiteral("usb1/1-1.1/1-1.1:1.0"), 0);
        link(dev + QStringLiteral("/video6"), byId + QStringLiteral("usb-Insta360_Insta360_Link_2-video-index0"));
        QCOMPARE(cameraNodePath(insta360), QFileInfo(dev + QStringLiteral("/video6")).canonicalFilePath());

        // Unplugged: the saved path stays, and there is no node.
        QFile::remove(dev + QStringLiteral("/video6"));
        QCOMPARE(cameraNodePath(insta360), QString());
        QCOMPARE(stableCameraPath(insta360, dev, sys), insta360);

        QCOMPARE(cameraIdNode("/dev/video6"), QStringLiteral("/dev/video6"));
        QCOMPARE(cameraIdNode("v4l2 /dev/video6 usb"), QStringLiteral("/dev/video6"));
        QCOMPARE(cameraIdNode("obs-virtual"), QString());
    }

    void microphoneMeterReadsPeakLevel()
    {
        QAudioFormat format;
        format.setSampleRate(48000);
        format.setChannelCount(1);
        format.setSampleFormat(QAudioFormat::Int16);
        const auto samples = [](std::initializer_list<qint16> values) {
            QByteArray bytes;
            for (const qint16 value : values) bytes.append(reinterpret_cast<const char *>(&value), sizeof value);
            return bytes;
        };
        QCOMPARE(MicrophoneMeter::peakLevel(format, samples({0, 0, 0})), 0.0);
        QVERIFY(MicrophoneMeter::peakLevel(format, samples({0, 32767, 0})) > 0.99);
        // -20 dBFS is two thirds of the way up the meter's 60 dB.
        QVERIFY(std::abs(MicrophoneMeter::peakLevel(format, samples({100, -3277, 50})) - 2.0 / 3.0) < 0.01);
        QCOMPARE(MicrophoneMeter::peakLevel(format, samples({16})), 0.0);
        QCOMPARE(MicrophoneMeter::peakLevel(format, QByteArray()), 0.0);
        format.setSampleFormat(QAudioFormat::Float);
        const float half = 0.5f;
        const double level = MicrophoneMeter::peakLevel(format, QByteArray(reinterpret_cast<const char *>(&half), sizeof half));
        QVERIFY(std::abs(level - (20.0 * std::log10(0.5) + 60.0) / 60.0) < 0.001);
    }
};

QTEST_APPLESS_MAIN(RecorderTest)
#include "test_recorder.moc"
