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

using namespace Omareel;

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

    void cameraCaptureBlockIncludesSettings()
    {
        const QJsonObject camera = Recorder::cameraCaptureBlock(
            QStringLiteral("/dev/video2"), 1080, 1920, 1080, 30.0, 1234567,
            QStringLiteral("qt-multimedia"), 90, true);
        QCOMPARE(camera.value(QStringLiteral("backend")).toString(), QStringLiteral("qt-multimedia"));
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
