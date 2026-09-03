#include "record/Recorder.h"
#include "record/CameraCapture.h"
#include "core/RecordingPreferences.h"

#include <QtTest>
#include <QTemporaryDir>
#include <limits>

using namespace OmaRecord;

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
        preferences.systemAudio = false;
        preferences.microphoneDevice = QStringLiteral("test_input");
        preferences.webcamEnabled = true;
        preferences.webcamDevice = QStringLiteral("/dev/video8");
        preferences.webcamHeight = 720;
        preferences.webcamRotation = 270;
        preferences.webcamFlipHorizontal = true;
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
};

QTEST_APPLESS_MAIN(RecorderTest)
#include "test_recorder.moc"
