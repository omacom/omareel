#include "render/Exporter.h"
#include "core/Project.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QImage>
#include <QProcess>
#include <QScopeGuard>
#include <QTemporaryDir>
#include <QTimer>
#include <QtTest>

using namespace Omareel;

class ExporterSmokeTest : public QObject
{
    Q_OBJECT
private slots:
    void padsOddDimensionsToEven()
    {
        QCOMPARE(paddedEvenSize(1367, 781), QSize(1368, 782));
        QCOMPARE(paddedEvenSize(1920, 1080), QSize(1920, 1080));
    }

    void backpressure_data()
    {
        QTest::addColumn<QByteArray>("mode");
        QTest::newRow("slow-real-encoder") << QByteArray("slow");
        QTest::newRow("cancel-full-queue") << QByteArray("stalled");
        QTest::newRow("cancel-finalization") << QByteArray("finalizing");
        QTest::newRow("silent-encoder-failure") << QByteArray("failed");
    }

    void backpressure()
    {
        QFETCH(QByteArray, mode);
        const QString ffmpeg = QStandardPaths::findExecutable(QStringLiteral("ffmpeg"));
        const QString python = QStandardPaths::findExecutable(QStringLiteral("python3"));
        if (ffmpeg.isEmpty() || python.isEmpty()) QSKIP("ffmpeg/python3 unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QString bundle = temporary.filePath(QStringLiteral("test.omareel"));
        QVERIFY(QDir().mkpath(bundle));
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write("{\"fps\":30,\"first_frame_us\":0}");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();
        QProcess source;
        source.start(ffmpeg, {QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
            QStringLiteral("testsrc2=size=320x180:rate=30"), QStringLiteral("-t"),
            QStringLiteral("1"), QStringLiteral("-c:v"), QStringLiteral("libx264"),
            QDir(bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(source.waitForFinished(15000));
        QCOMPARE(source.exitCode(), 0);
        Project project = Project::defaults(QStringLiteral("Backpressure"), 1.0);
        project.background.type = QStringLiteral("color");
        project.cursor.visible = false;
        project.camera.enabled = false;
        QString error;
        QVERIFY2(project.save(QDir(bundle).filePath(QStringLiteral("project.json")), &error), qPrintable(error));

        const QString shim = QFINDTESTDATA("tools/export-encoder.py");
        QVERIFY(!shim.isEmpty());
        const auto quote = [](QString value) {
            return QLatin1Char('\'') + value.replace(QLatin1Char('\''), QStringLiteral("'\\''")) + QLatin1Char('\'');
        };
        QFile wrapper(temporary.filePath(QStringLiteral("ffmpeg")));
        QVERIFY(wrapper.open(QIODevice::WriteOnly));
        wrapper.write((QStringLiteral("#!/bin/sh\nexec ") + quote(python) + QLatin1Char(' ')
                       + quote(shim) + QStringLiteral(" \"$@\"\n")).toUtf8());
        wrapper.close();
        QVERIFY(wrapper.setPermissions(QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner));
        const QList<QByteArray> names{"PATH", "OMAREEL_DISABLE_NVENC", "OMAREEL_TEST_ENCODER_MODE", "OMAREEL_TEST_REAL_FFMPEG"};
        QList<QByteArray> previous;
        for (const auto &name : names) previous << qgetenv(name.constData());
        const auto restore = qScopeGuard([&] {
            for (int i = 0; i < names.size(); ++i) {
                if (previous[i].isNull()) qunsetenv(names[i].constData());
                else qputenv(names[i].constData(), previous[i]);
            }
        });
        qputenv("PATH", temporary.path().toUtf8() + ':' + previous[0]);
        qputenv("OMAREEL_DISABLE_NVENC", "1");
        qputenv("OMAREEL_TEST_ENCODER_MODE", mode);
        qputenv("OMAREEL_TEST_REAL_FFMPEG", ffmpeg.toUtf8());

        const QString output = temporary.filePath(QStringLiteral("out.mp4"));
        QFile sentinel(output);
        QVERIFY(sentinel.open(QIODevice::WriteOnly));
        sentinel.write("keep");
        sentinel.close();
        Exporter exporter;
        QSignalSpy finished(&exporter, &Exporter::finished);
        QSignalSpy failed(&exporter, &Exporter::failed);
        bool cancelScheduled = false;
        connect(&exporter, &Exporter::progress, this, [&](int frame, int total) {
            if (!cancelScheduled && ((mode == "stalled" && frame == 1)
                                     || (mode == "finalizing" && frame == total))) {
                cancelScheduled = true;
                QTimer::singleShot(150, &exporter, &Exporter::cancel);
            }
        });
        QElapsedTimer elapsed;
        elapsed.start();
        exporter.exportBundle({bundle, output, 30, 1280, QStringLiteral("web-low"), 0, 0});
        if (mode == "slow") {
            QVERIFY2(finished.count() == 1, failed.isEmpty() ? "No completion" : qPrintable(failed.first().first().toString()));
            QVERIFY(failed.isEmpty());
            QVERIFY(elapsed.elapsed() >= 1000);
            QProcess probe;
            probe.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
                QStringLiteral("-count_frames"), QStringLiteral("-select_streams"), QStringLiteral("v:0"),
                QStringLiteral("-show_entries"), QStringLiteral("stream=nb_read_frames"),
                QStringLiteral("-of"), QStringLiteral("csv=p=0"), output});
            QVERIFY(probe.waitForFinished(15000));
            QCOMPARE(probe.exitCode(), 0);
            QCOMPARE(probe.readAllStandardOutput().trimmed(), QByteArray("30"));
        } else {
            QVERIFY(finished.isEmpty());
            QCOMPARE(failed.count(), 1);
            const QString failure = failed.first().first().toString();
            QVERIFY(!failure.isEmpty());
            if (mode == "failed") QVERIFY2(failure.contains(QStringLiteral("encoder"), Qt::CaseInsensitive), qPrintable(failure));
            if (mode != "failed") {
                QVERIFY2(cancelScheduled, qPrintable(failure));
                QVERIFY2(failure.contains(QStringLiteral("cancelled")), qPrintable(failure));
            }
            QVERIFY2(elapsed.elapsed() < 10000, "Cancellation/failure must not wait for the write or finish timeout");
            QVERIFY(sentinel.open(QIODevice::ReadOnly));
            QCOMPARE(sentinel.readAll(), QByteArray("keep"));
        }
        QCOMPARE(QDir(temporary.path()).entryList({QStringLiteral(".omareel-export-*.mp4")},
                                                  QDir::Files | QDir::Hidden), QStringList());
    }

    void exportsTwoSecondH264()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QString fixture = QFINDTESTDATA("fixtures/synthetic");
        QVERIFY(!fixture.isEmpty());
        const QString bundle = temporary.filePath(QStringLiteral("synthetic.omareel"));
        QVERIFY(QDir().mkpath(bundle));
        QVERIFY(QFile::copy(QDir(fixture).filePath(QStringLiteral("capture.json")), QDir(bundle).filePath(QStringLiteral("capture.json"))));
        QVERIFY(QFile::copy(QDir(fixture).filePath(QStringLiteral("input.jsonl")), QDir(bundle).filePath(QStringLiteral("input.jsonl"))));
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text));
        input.write("{\"t\":2100000,\"k\":\"kd\",\"code\":25,\"name\":\"KEY_P\",\"mods\":[\"ctrl\",\"shift\"]}\n");
        input.write("{\"t\":2200000,\"k\":\"ku\",\"code\":25}\n");
        input.close();
        QProcess makeVideo;
        makeVideo.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("testsrc2=size=640x360:rate=60"),
            QStringLiteral("-t"), QStringLiteral("2"), QStringLiteral("-c:v"), QStringLiteral("libx264"),
            QDir(bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(makeVideo.waitForFinished(30000));
        QCOMPARE(makeVideo.exitCode(), 0);

        QProcess makeCamera;
        makeCamera.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("testsrc2=size=320x180:rate=30"),
            QStringLiteral("-t"), QStringLiteral("2"), QStringLiteral("-c:v"), QStringLiteral("libx264"),
            QDir(bundle).filePath(QStringLiteral("camera.mp4"))});
        QVERIFY(makeCamera.waitForFinished(30000));
        QCOMPARE(makeCamera.exitCode(), 0);
        QFile cameraTimestamp(QDir(bundle).filePath(QStringLiteral("camera.mp4.ts")));
        QVERIFY(cameraTimestamp.open(QIODevice::WriteOnly | QIODevice::Text));
        const QByteArray timestampData("monotonic_microsec\trealtime_microsec\n1200000\t1200000\n");
        QCOMPARE(cameraTimestamp.write(timestampData), qint64(timestampData.size()));
        cameraTimestamp.close();
        QFile captureFile(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(captureFile.open(QIODevice::ReadOnly));
        QJsonObject capture = QJsonDocument::fromJson(captureFile.readAll()).object();
        captureFile.close();
        capture.insert(QStringLiteral("camera"), QJsonObject{
            {QStringLiteral("device"), QStringLiteral("synthetic")},
            {QStringLiteral("width"), 320}, {QStringLiteral("height"), 180},
            {QStringLiteral("fps"), 30}, {QStringLiteral("first_frame_us"), 1200000},
            {QStringLiteral("backend"), QStringLiteral("synthetic")}});
        QVERIFY(captureFile.open(QIODevice::WriteOnly | QIODevice::Truncate));
        captureFile.write(QJsonDocument(capture).toJson(QJsonDocument::Indented));
        captureFile.close();
        Project project = Project::defaults(QStringLiteral("Camera smoke"), 2.0);
        project.background.type = QStringLiteral("color");
        project.background.color = QColor(QStringLiteral("#101020"));
        project.cursor.visible = false;
        project.cursor.clickSound = QStringLiteral("soft");
        project.keystrokes.enabled = true;
        project.camera.enabled = true;
        project.camera.shadow.enabled = false;
        QString projectError;
        QVERIFY2(project.save(QDir(bundle).filePath(QStringLiteral("project.json")), &projectError),
                 qPrintable(projectError));

        const QString output = temporary.filePath(QStringLiteral("out.mp4"));
        Exporter exporter;
        QString failure;
        bool finished = false;
        connect(&exporter, &Exporter::finished, this, [&](const QString &) { finished = true; });
        connect(&exporter, &Exporter::failed, this, [&](const QString &message) { failure = message; });
        exporter.exportBundle({bundle, output, 30, 640, QStringLiteral("low"), 0, 0});
        if (!finished && (failure.contains(QStringLiteral("OpenGL"), Qt::CaseInsensitive)
                          || failure.contains(QStringLiteral("offscreen"), Qt::CaseInsensitive)
                          || failure.contains(QStringLiteral("render control"), Qt::CaseInsensitive)))
            QSKIP(qPrintable(failure));
        QVERIFY2(finished, qPrintable(failure));
        QVERIFY(QFileInfo(output).size() > 0);

        QProcess inspect;
        inspect.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-show_entries"), QStringLiteral("stream=codec_type,codec_name,width,height:format=duration"),
            QStringLiteral("-of"), QStringLiteral("json"), output});
        QVERIFY(inspect.waitForFinished(15000));
        QCOMPARE(inspect.exitCode(), 0);
        const auto root = QJsonDocument::fromJson(inspect.readAllStandardOutput()).object();
        const QJsonArray streams = root.value("streams").toArray();
        const auto stream = streams.first().toObject();
        QCOMPARE(stream.value("codec_name").toString(), QStringLiteral("h264"));
        QCOMPARE(stream.value("width").toInt(), 640);
        QCOMPARE(stream.value("height").toInt(), 360);
        const double duration = root.value("format").toObject().value("duration").toString().toDouble();
        QVERIFY(duration > 1.9 && duration < 2.1);
        bool hasAudio = false;
        for (const QJsonValue &value : streams)
            hasAudio = hasAudio || value.toObject().value(QStringLiteral("codec_type")) == QLatin1String("audio");
        QVERIFY2(hasAudio, "A silent source with soft clicks must export an audio track");

        const QString stillPath = temporary.filePath(QStringLiteral("overlay.png"));
        QProcess still;
        still.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-ss"), QStringLiteral("0.8"), QStringLiteral("-i"), output,
            QStringLiteral("-frames:v"), QStringLiteral("1"), stillPath});
        QVERIFY(still.waitForFinished(15000));
        QCOMPARE(still.exitCode(), 0);
        const QImage exportedFrame(stillPath);
        QVERIFY(!exportedFrame.isNull());
        const QColor backgroundPixel = exportedFrame.pixelColor(4, 4);
        const QColor cameraPixel = exportedFrame.pixelColor(582, 308);
        const int colorDistance = std::abs(cameraPixel.red() - backgroundPixel.red())
            + std::abs(cameraPixel.green() - backgroundPixel.green())
            + std::abs(cameraPixel.blue() - backgroundPixel.blue());
        QVERIFY2(colorDistance > 60, "The exported camera overlay did not differ from the background");

        project.camera.rotation = 180;
        QVERIFY2(project.save(QDir(bundle).filePath(QStringLiteral("project.json")), &projectError),
                 qPrintable(projectError));
        const QString rotatedOutput = temporary.filePath(QStringLiteral("rotated.mp4"));
        Exporter rotatedExporter;
        QString rotatedFailure;
        bool rotatedFinished = false;
        connect(&rotatedExporter, &Exporter::finished, this,
                [&](const QString &) { rotatedFinished = true; });
        connect(&rotatedExporter, &Exporter::failed, this,
                [&](const QString &message) { rotatedFailure = message; });
        rotatedExporter.exportBundle({bundle, rotatedOutput, 30, 640,
                                      QStringLiteral("low"), 0, 0});
        QVERIFY2(rotatedFinished, qPrintable(rotatedFailure));
        const QString rotatedStillPath = temporary.filePath(QStringLiteral("rotated.png"));
        QProcess rotatedStill;
        rotatedStill.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"),
            QStringLiteral("error"), QStringLiteral("-ss"), QStringLiteral("0.8"),
            QStringLiteral("-i"), rotatedOutput, QStringLiteral("-frames:v"),
            QStringLiteral("1"), rotatedStillPath});
        QVERIFY(rotatedStill.waitForFinished(15000));
        QCOMPARE(rotatedStill.exitCode(), 0);
        const QImage rotatedFrame(rotatedStillPath);
        QVERIFY(!rotatedFrame.isNull());
        int changedCameraPixels = 0;
        const QRect cameraRegion(468, 263, 160, 90);
        for (int y = cameraRegion.top(); y <= cameraRegion.bottom(); ++y) {
            for (int x = cameraRegion.left(); x <= cameraRegion.right(); ++x) {
                const QColor a = exportedFrame.pixelColor(x, y);
                const QColor b = rotatedFrame.pixelColor(x, y);
                const int distance = std::abs(a.red() - b.red())
                    + std::abs(a.green() - b.green()) + std::abs(a.blue() - b.blue());
                if (distance > 30) ++changedCameraPixels;
            }
        }
        QVERIFY2(changedCameraPixels > 500,
                 "The 640x360 offscreen composition did not react to camera rotation");

        const QString earlyStillPath = temporary.filePath(QStringLiteral("before-camera.png"));
        QProcess earlyStill;
        earlyStill.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-ss"), QStringLiteral("0.05"), QStringLiteral("-i"), output,
            QStringLiteral("-frames:v"), QStringLiteral("1"), earlyStillPath});
        QVERIFY(earlyStill.waitForFinished(15000));
        QCOMPARE(earlyStill.exitCode(), 0);
        const QImage earlyFrame(earlyStillPath);
        QVERIFY(!earlyFrame.isNull());
        const QColor earlyBackground = earlyFrame.pixelColor(4, 4);
        const QColor earlyCameraArea = earlyFrame.pixelColor(582, 308);
        const int earlyDistance = std::abs(earlyCameraArea.red() - earlyBackground.red())
            + std::abs(earlyCameraArea.green() - earlyBackground.green())
            + std::abs(earlyCameraArea.blue() - earlyBackground.blue());
        QVERIFY2(earlyDistance < 35, "The camera overlay must stay hidden before its first frame");

        const QString cancelledOutput = temporary.filePath(QStringLiteral("cancelled.mp4"));
        QFile sentinel(cancelledOutput);
        QVERIFY(sentinel.open(QIODevice::WriteOnly));
        QCOMPARE(sentinel.write("keep"), qint64(4));
        sentinel.close();
        Exporter cancelledExporter;
        QString cancellationFailure;
        connect(&cancelledExporter, &Exporter::failed, this,
                [&](const QString &message) { cancellationFailure = message; });
        QTimer::singleShot(1, &cancelledExporter, &Exporter::cancel);
        cancelledExporter.exportBundle({bundle, cancelledOutput, 60, 640,
                                        QStringLiteral("low"), 0, 0});
        QVERIFY(cancellationFailure.contains(QStringLiteral("cancelled"), Qt::CaseInsensitive));
        QVERIFY(sentinel.open(QIODevice::ReadOnly));
        QCOMPARE(sentinel.readAll(), QByteArray("keep"));
        QCOMPARE(QDir(temporary.path()).entryList({QStringLiteral(".omareel-export-*.mp4")},
                                                  QDir::Files), QStringList());
    }
};

QTEST_MAIN(ExporterSmokeTest)
#include "test_exporter_smoke.moc"
