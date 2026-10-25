#include "Exporter.h"
#include "ProjectJson.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QImage>
#include <QProcess>
#include <QRegularExpression>
#include <QScopeGuard>
#include <QTemporaryDir>
#include <QTimer>
#include <QtTest>

using namespace Omareel;

static bool makeColourVideo(const QString &colour, const QString &path, double seconds = 2.0,
                            const QString &size = QStringLiteral("640x360"))
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
        QStringLiteral("color=c=%1:size=%2:rate=30").arg(colour, size),
        QStringLiteral("-t"), QString::number(seconds), QStringLiteral("-c:v"), QStringLiteral("libx264"),
        QStringLiteral("-pix_fmt"), QStringLiteral("yuv420p"), path});
    return process.waitForFinished(30000) && process.exitCode() == 0;
}

// A solid colour clip with a sine tone, for checking which source plays where.
static bool makeToneVideo(const QString &colour, const QString &path, double seconds,
                          const QString &size, int frequency, int sampleRate, int channels)
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
        QStringLiteral("color=c=%1:size=%2:rate=30").arg(colour, size),
        QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
        QStringLiteral("sine=frequency=%1:sample_rate=%2").arg(frequency).arg(sampleRate),
        QStringLiteral("-t"), QString::number(seconds), QStringLiteral("-ac"), QString::number(channels),
        QStringLiteral("-c:v"), QStringLiteral("libx264"), QStringLiteral("-pix_fmt"), QStringLiteral("yuv420p"),
        QStringLiteral("-c:a"), QStringLiteral("aac"), path});
    return process.waitForFinished(30000) && process.exitCode() == 0;
}

// Loudest sample of the exported audio between two times, in dB (-inf for silence).
static double maxVolume(const QString &video, double from, double to)
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("info"),
        QStringLiteral("-ss"), QString::number(from, 'f', 3), QStringLiteral("-t"),
        QString::number(to - from, 'f', 3), QStringLiteral("-i"), video, QStringLiteral("-vn"),
        QStringLiteral("-af"), QStringLiteral("volumedetect"), QStringLiteral("-f"), QStringLiteral("null"),
        QStringLiteral("-")});
    if (!process.waitForFinished(15000)) return 0.0;
    const QString log = QString::fromUtf8(process.readAllStandardError());
    const QRegularExpressionMatch match = QRegularExpression(
        QStringLiteral("max_volume: (-?[0-9.]+|-inf) dB")).match(log);
    if (!match.hasMatch()) return 0.0;
    return match.captured(1) == QLatin1String("-inf") ? -200.0 : match.captured(1).toDouble();
}

static bool exportBundle(const QString &bundle, const QString &output, QString *error)
{
    Exporter exporter;
    QString failure;
    QObject::connect(&exporter, &Exporter::failed, [&](const QString &message) { failure = message; });
    bool done = false;
    QObject::connect(&exporter, &Exporter::finished, [&] { done = true; });
    ExportOptions options;
    options.bundlePath = bundle;
    options.outputPath = output;
    options.fps = 30;
    options.width = 640;
    exporter.exportBundle(options);
    if (error) *error = failure;
    return done && failure.isEmpty();
}

static double probeDuration(const QString &path)
{
    QProcess probe;
    probe.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-show_entries"), QStringLiteral("format=duration"),
        QStringLiteral("-of"), QStringLiteral("csv=p=0"), path});
    if (!probe.waitForFinished(15000)) return -1.0;
    return probe.readAllStandardOutput().trimmed().toDouble();
}

static QColor pixelAt(const QString &video, double seconds, int x, int y)
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-ss"), QString::number(seconds, 'f', 3), QStringLiteral("-i"), video,
        QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-f"), QStringLiteral("image2pipe"),
        QStringLiteral("-c:v"), QStringLiteral("png"), QStringLiteral("-")});
    if (!process.waitForFinished(15000)) return {};
    const QImage frame = QImage::fromData(process.readAllStandardOutput(), "PNG");
    return frame.valid(x, y) ? frame.pixelColor(x, y) : QColor();
}

static bool isYellow(const QColor &c) { return c.isValid() && c.red() > 150 && c.green() > 150 && c.blue() < 90; }
static bool isRed(const QColor &c) { return c.isValid() && c.red() > 150 && c.green() < 90 && c.blue() < 90; }
static bool isGreen(const QColor &c) { return c.isValid() && c.green() > 130 && c.red() < 90 && c.blue() < 90; }
static bool isBlue(const QColor &c) { return c.isValid() && c.blue() > 150 && c.red() < 90 && c.green() < 90; }

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
        QJsonObject project = ProjectJson::defaults(QStringLiteral("Backpressure"), 1.0);
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        ProjectJson::set(project, {"camera", "enabled"}, false);
        QString error;
        QVERIFY2(ProjectJson::save(project, QDir(bundle).filePath(QStringLiteral("project.json")), &error),
                 qPrintable(error));

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

    void clipsStartingOnAStillScreenShowTheFrameOnScreen()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        // Like a native capture of a screen that sat still: red frames until 0.5 s, then nothing
        // until the screen turns blue at 2.5 s. Encoded as the recorder does, with gaps kept.
        const QString bundle = temporary.filePath(QStringLiteral("still.omareel"));
        QVERIFY(QDir().mkpath(bundle));
        QProcess makeVideo;
        makeVideo.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
            QStringLiteral("color=c=red:size=640x360:rate=30:duration=1.5[r];"
                           "color=c=blue:size=640x360:rate=30:duration=1.5[b];[r][b]concat=n=2"),
            QStringLiteral("-vf"), QStringLiteral("select='lt(t\\,0.5)+gte(t\\,2.5)'"),
            QStringLiteral("-c:v"), QStringLiteral("libx264"), QStringLiteral("-bf"), QStringLiteral("0"),
            QStringLiteral("-g"), QStringLiteral("30"), QStringLiteral("-pix_fmt"), QStringLiteral("yuv420p"),
            QStringLiteral("-fps_mode"), QStringLiteral("vfr"), QStringLiteral("-enc_time_base"),
            QStringLiteral("1/30"), QDir(bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(makeVideo.waitForFinished(30000) && makeVideo.exitCode() == 0);
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(R"({"fps":30,"first_frame_us":1000})");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();

        // The first clip, trimmed into the still part, has no frame of its own; the second starts
        // half a second before the screen changes.
        QJsonObject project = ProjectJson::defaults(QStringLiteral("Still"), 3.0);
        project.insert(QStringLiteral("clips"), QJsonArray{ProjectJson::clip(QStringLiteral("a"), 1.0, 2.0),
                                                           ProjectJson::clip(QStringLiteral("b"), 2.0, 3.0)});
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"background", "color"}, QStringLiteral("#00c000"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        QString error;
        QVERIFY2(ProjectJson::save(project, QDir(bundle).filePath(QStringLiteral("project.json")), &error),
                 qPrintable(error));

        const QString output = temporary.filePath(QStringLiteral("still.mp4"));
        QVERIFY2(exportBundle(bundle, output, &error), qPrintable(error));
        QVERIFY(qAbs(probeDuration(output) - 2.0) < 0.05);
        QVERIFY(isRed(pixelAt(output, 0.5, 320, 180)));
        // The second clip holds red until its source reaches 2.5 s, 1.5 s into the export.
        QVERIFY(isRed(pixelAt(output, 1.3, 320, 180)));
        QVERIFY(isBlue(pixelAt(output, 1.7, 320, 180)));
    }

    void cameraLayoutsRenderFullFrame()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        // Blue screen, red camera, green background: every pixel says which layer it shows.
        const QString bundle = temporary.filePath(QStringLiteral("layouts.omareel"));
        QVERIFY(QDir().mkpath(bundle));
        QVERIFY(makeColourVideo(QStringLiteral("blue"), QDir(bundle).filePath(QStringLiteral("screen.mp4"))));
        QVERIFY(makeColourVideo(QStringLiteral("red"), QDir(bundle).filePath(QStringLiteral("camera.mp4"))));
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(R"({"fps":30,"first_frame_us":1000,"camera":{"first_frame_us":1000}})");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();

        QJsonObject project = ProjectJson::defaults(QStringLiteral("Layouts"), 2.0);
        project.insert(QStringLiteral("clips"), QJsonArray{
            ProjectJson::clip(QStringLiteral("intro"), 0.0, 1.0, 1.0, QStringLiteral("camera")),
            ProjectJson::clip(QStringLiteral("main"), 1.0, 2.0)});
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"background", "color"}, QStringLiteral("#00c000"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        ProjectJson::set(project, {"camera", "enabled"}, true);
        ProjectJson::set(project, {"camera", "shadow", "enabled"}, false);
        const QString projectPath = QDir(bundle).filePath(QStringLiteral("project.json"));
        QString error;
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));

        const QString output = temporary.filePath(QStringLiteral("fill.mp4"));
        QVERIFY2(exportBundle(bundle, output, &error), qPrintable(error));
        QVERIFY(qAbs(probeDuration(output) - 2.0) < 0.05);
        // The camera clip fills the canvas edge to edge.
        QVERIFY(isRed(pixelAt(output, 0.5, 4, 4)));
        QVERIFY(isRed(pixelAt(output, 0.5, 320, 180)));
        // A quarter second into the next clip the camera is halfway back to its bubble, and the
        // screen shows around it.
        QVERIFY(isRed(pixelAt(output, 1.25, 500, 300)));
        QVERIFY(isBlue(pixelAt(output, 1.25, 100, 100)));
        // Then the screen with the round bubble at the bottom right.
        QVERIFY(isBlue(pixelAt(output, 1.9, 320, 180)));
        QVERIFY(isRed(pixelAt(output, 1.9, 582, 308)));

        ProjectJson::set(project, {"camera", "fullFrame"}, QStringLiteral("square"));
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));
        const QString square = temporary.filePath(QStringLiteral("square.mp4"));
        QVERIFY2(exportBundle(bundle, square, &error), qPrintable(error));
        QVERIFY(isRed(pixelAt(square, 0.5, 320, 180)));
        // Beside the square: a blurred copy of the camera, or a dark fill in software rendering,
        // never the screen or the background.
        const QColor side = pixelAt(square, 0.5, 20, 180);
        QVERIFY2(!isBlue(side) && !isGreen(side), qPrintable(side.name()));

        // Switching the bubble off leaves camera clips full frame.
        ProjectJson::set(project, {"camera", "fullFrame"}, QStringLiteral("fill"));
        ProjectJson::set(project, {"camera", "enabled"}, false);
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));
        const QString noBubble = temporary.filePath(QStringLiteral("no-bubble.mp4"));
        QVERIFY2(exportBundle(bundle, noBubble, &error), qPrintable(error));
        QVERIFY(isRed(pixelAt(noBubble, 0.5, 320, 180)));
        QVERIFY(isGreen(pixelAt(noBubble, 1.9, 582, 308)));
    }

    void insertedClipsPlayWithTheirOwnPictureAndSound()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        // Blue recording with a 48 kHz stereo tone, a lime 4:3 insert with a 44.1 kHz mono tone,
        // a red insert without audio, and a yellow background.
        const QString bundle = temporary.filePath(QStringLiteral("inserts.omareel"));
        QVERIFY(QDir(bundle).mkpath(QStringLiteral("media")));
        QVERIFY(makeToneVideo(QStringLiteral("blue"), QDir(bundle).filePath(QStringLiteral("screen.mp4")),
                              2.0, QStringLiteral("640x360"), 440, 48000, 2));
        QVERIFY(makeToneVideo(QStringLiteral("lime"), QDir(bundle).filePath(QStringLiteral("media/lime.mp4")),
                              1.5, QStringLiteral("480x360"), 880, 44100, 1));
        QVERIFY(makeColourVideo(QStringLiteral("red"), QDir(bundle).filePath(QStringLiteral("media/silent.mp4")),
                                1.0));
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(R"({"fps":30,"first_frame_us":1000,"audio":{"desktop":true,"mic":true}})");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();

        const auto insert = [](const char *id, const char *file, double in, double out, const char *fit) {
            return ProjectJson::insertedClip(QString::fromLatin1(id), in, out,
                QStringLiteral("media/") + QString::fromLatin1(file), QString::fromLatin1(fit));
        };
        QJsonObject project = ProjectJson::defaults(QStringLiteral("Inserts"), 2.0);
        project.insert(QStringLiteral("clips"), QJsonArray{
            ProjectJson::clip(QStringLiteral("a"), 0.0, 1.0),           // 0.0-1.0
            insert("fit", "lime.mp4", 0.2, 1.2, "fit"),                 // 1.0-2.0
            insert("fill", "lime.mp4", 0.2, 0.7, "fill"),               // 2.0-2.5
            insert("framed", "lime.mp4", 0.7, 1.2, "framed"),           // 2.5-3.0
            insert("quiet", "silent.mp4", 0.0, 0.5, "fill"),            // 3.0-3.5
            ProjectJson::clip(QStringLiteral("b"), 1.0, 2.0)});         // 3.5-4.5
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"background", "color"}, QStringLiteral("#ffff00"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        ProjectJson::set(project, {"camera", "enabled"}, false);
        const QString projectPath = QDir(bundle).filePath(QStringLiteral("project.json"));
        QString error;
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));

        const QString output = temporary.filePath(QStringLiteral("inserts.mp4"));
        QVERIFY2(exportBundle(bundle, output, &error), qPrintable(error));
        QVERIFY2(qAbs(probeDuration(output) - 4.5) < 0.06, qPrintable(QString::number(probeDuration(output))));
        QVERIFY(isBlue(pixelAt(output, 0.5, 320, 180)));
        // Fit: the whole 4:3 insert, with a blurred copy (or a dark fill in software
        // rendering) beside it instead of the background.
        QVERIFY(isGreen(pixelAt(output, 1.5, 320, 180)));
        const QColor beside = pixelAt(output, 1.5, 4, 4);
        QVERIFY2(!isBlue(beside) && !isYellow(beside), qPrintable(beside.name()));
        // Fill covers the canvas; framed sits in the frame over the background.
        QVERIFY(isGreen(pixelAt(output, 2.25, 4, 4)));
        QVERIFY(isYellow(pixelAt(output, 2.75, 4, 4)));
        QVERIFY(isGreen(pixelAt(output, 2.75, 320, 180)));
        QVERIFY(isRed(pixelAt(output, 3.25, 320, 180)));
        QVERIFY(isBlue(pixelAt(output, 4.0, 320, 180)));
        // Every clip keeps its own sound in place: the recording's, the insert's, silence.
        QVERIFY(maxVolume(output, 0.1, 0.9) > -30.0);
        QVERIFY(maxVolume(output, 1.1, 2.9) > -30.0);
        QVERIFY2(maxVolume(output, 3.1, 3.4) < -60.0, qPrintable(QString::number(maxVolume(output, 3.1, 3.4))));
        QVERIFY(maxVolume(output, 3.6, 4.4) > -30.0);

        // A missing insert fails the export early, naming it.
        QVERIFY(QFile::remove(QDir(bundle).filePath(QStringLiteral("media/silent.mp4"))));
        QVERIFY(!exportBundle(bundle, temporary.filePath(QStringLiteral("missing.mp4")), &error));
        QVERIFY2(error.contains(QStringLiteral("silent.mp4")), qPrintable(error));
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
        QJsonObject project = ProjectJson::defaults(QStringLiteral("Camera smoke"), 2.0);
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"background", "color"}, QStringLiteral("#101020"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        ProjectJson::set(project, {"cursor", "clickSound"}, QStringLiteral("soft"));
        ProjectJson::set(project, {"keystrokes", "enabled"}, true);
        ProjectJson::set(project, {"camera", "enabled"}, true);
        ProjectJson::set(project, {"camera", "shadow", "enabled"}, false);
        QString projectError;
        QVERIFY2(ProjectJson::save(project, QDir(bundle).filePath(QStringLiteral("project.json")), &projectError),
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

        ProjectJson::set(project, {"camera", "rotation"}, 180);
        QVERIFY2(ProjectJson::save(project, QDir(bundle).filePath(QStringLiteral("project.json")), &projectError),
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
