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

// Mean level of the exported audio between two times through an audio filter, in dB.
static double meanVolume(const QString &video, double from, double to, const QString &filter)
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("info"),
        QStringLiteral("-ss"), QString::number(from, 'f', 3), QStringLiteral("-t"),
        QString::number(to - from, 'f', 3), QStringLiteral("-i"), video, QStringLiteral("-vn"),
        QStringLiteral("-af"), filter + QStringLiteral(",volumedetect"), QStringLiteral("-f"), QStringLiteral("null"),
        QStringLiteral("-")});
    if (!process.waitForFinished(15000)) return 0.0;
    const QString log = QString::fromUtf8(process.readAllStandardError());
    const QRegularExpressionMatch match = QRegularExpression(
        QStringLiteral("mean_volume: (-?[0-9.]+|-inf) dB")).match(log);
    if (!match.hasMatch()) return 0.0;
    return match.captured(1) == QLatin1String("-inf") ? -200.0 : match.captured(1).toDouble();
}

static bool exportBundle(const QString &bundle, const QString &output, QString *error, int fps = 30)
{
    Exporter exporter;
    QString failure;
    QObject::connect(&exporter, &Exporter::failed, [&](const QString &message) { failure = message; });
    bool done = false;
    QObject::connect(&exporter, &Exporter::finished, [&] { done = true; });
    ExportOptions options;
    options.bundlePath = bundle;
    options.outputPath = output;
    options.fps = fps;
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

static QImage frameAt(const QString &video, double seconds)
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-ss"), QString::number(seconds, 'f', 3), QStringLiteral("-i"), video,
        QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-f"), QStringLiteral("image2pipe"),
        QStringLiteral("-c:v"), QStringLiteral("png"), QStringLiteral("-")});
    if (!process.waitForFinished(15000)) return {};
    return QImage::fromData(process.readAllStandardOutput(), "PNG");
}

// Variance of the grey level over `area`: high over text, near zero once it is hidden.
static double greyVariance(const QImage &image, const QRect &area)
{
    double sum = 0, squares = 0;
    int count = 0;
    for (int y = area.top(); y <= area.bottom(); ++y)
        for (int x = area.left(); x <= area.right(); ++x) {
            const double grey = qGray(image.pixel(x, y));
            sum += grey;
            squares += grey * grey;
            ++count;
        }
    if (count == 0) return -1;
    const double mean = sum / count;
    return squares / count - mean * mean;
}

// The grey-level variance over `area` (0–1 of the frame) in every frame of `video`, decoded at
// `width` x `height`.
static QList<double> frameVariances(const QString &video, int width, int height, const QRectF &area)
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"), QStringLiteral("-i"), video,
        QStringLiteral("-vf"), QStringLiteral("scale=%1:%2").arg(width).arg(height),
        QStringLiteral("-f"), QStringLiteral("rawvideo"), QStringLiteral("-pix_fmt"), QStringLiteral("gray"), QStringLiteral("-")});
    if (!process.waitForFinished(60000)) return {};
    const QByteArray raw = process.readAllStandardOutput();
    const QRect pixels(int(area.x() * width), int(area.y() * height), int(area.width() * width), int(area.height() * height));
    QList<double> variances;
    for (qsizetype offset = 0; offset + width * height <= raw.size(); offset += width * height) {
        double sum = 0, squares = 0;
        for (int y = pixels.top(); y <= pixels.bottom(); ++y)
            for (int x = pixels.left(); x <= pixels.right(); ++x) {
                const double grey = quint8(raw[offset + y * width + x]);
                sum += grey;
                squares += grey * grey;
            }
        const double count = double(pixels.width()) * pixels.height();
        variances << squares / count - (sum / count) * (sum / count);
    }
    return variances;
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
        // Falls back to libx264 where VA-API is there to fail; plain libx264 elsewhere.
        QTest::newRow("gpu-encoder-start-failure") << QByteArray("vaapi-fails");
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
        const QList<QByteArray> names{"PATH", "OMAREEL_DISABLE_NVENC", "OMAREEL_TEST_ENCODER_MODE", "OMAREEL_TEST_REAL_FFMPEG",
                                      "OMAREEL_EXPORT_ENCODER"};
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
        if (mode == "vaapi-fails") qputenv("OMAREEL_EXPORT_ENCODER", "vaapi");

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
        if (mode == "slow" || mode == "vaapi-fails") {
            QVERIFY2(finished.count() == 1, failed.isEmpty() ? "No completion" : qPrintable(failed.first().first().toString()));
            QVERIFY(failed.isEmpty());
            if (mode == "slow") QVERIFY(elapsed.elapsed() >= 1000);
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

    // Blur overlays hide what they cover in the export, on whichever renderer runs (ctest runs
    // this on the GPU and again as exporter_overlays_software): the exporter bakes them into the
    // recording's pixels, the RGBA frames of the software path and the YUV ones of the GPU path.
    void blurRegionsHideWhatTheyCover()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QString bundle = temporary.filePath(QStringLiteral("blur.omareel"));
        QVERIFY(QDir().mkpath(bundle));
        // Black text on white, line after line, over the whole frame.
        QStringList lines;
        for (int i = 0; i < 20; ++i)
            for (int x : {6, 326})
                lines << QStringLiteral("drawtext=text='Password hunter2 token %1 ABCDEFGH':"
                                        "fontsize=15:fontcolor=black:x=%2:y=%3").arg(i).arg(x).arg(4 + i * 18);
        QProcess makeVideo;
        makeVideo.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
            QStringLiteral("color=c=white:size=640x360:rate=30:duration=2"),
            QStringLiteral("-vf"), lines.join(QLatin1Char(',')),
            QStringLiteral("-c:v"), QStringLiteral("libx264"), QStringLiteral("-crf"), QStringLiteral("12"),
            QStringLiteral("-pix_fmt"), QStringLiteral("yuv420p"), QDir(bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(makeVideo.waitForFinished(30000) && makeVideo.exitCode() == 0);
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(R"({"fps":30,"first_frame_us":1000})");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();

        // No background: the recording fills the 640x360 output, so recording coordinates are
        // output pixels. Pixelate the left region and blur the right one from 0.5 s to 1.5 s.
        QJsonObject project = ProjectJson::defaults(QStringLiteral("Blur"), 2.0);
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("none"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        const auto region = [](const char *id, const char *style, double x) {
            return QJsonObject{{"id", id}, {"kind", "blur"}, {"anchor", "screen"}, {"start", 0.5}, {"end", 1.5},
                {"rect", QJsonObject{{"x", x}, {"y", 0.25}, {"w", 0.4}, {"h", 0.5}}},
                {"style", style}, {"amount", 0.5}};
        };
        project.insert(QStringLiteral("overlays"), QJsonArray{region("p", "pixelate", 0.05), region("b", "blur", 0.55)});
        QString error;
        QVERIFY2(ProjectJson::save(project, QDir(bundle).filePath(QStringLiteral("project.json")), &error),
                 qPrintable(error));
        const QString output = temporary.filePath(QStringLiteral("blur.mp4"));
        QVERIFY2(exportBundle(bundle, output, &error), qPrintable(error));

        // Inside each region, a few pixels in from its edges; and a strip of text outside both.
        const QRect pixelated(40, 100, 240, 160), blurred(360, 100, 240, 160), outside(40, 290, 560, 60);
        const QImage before = frameAt(output, 0.2), during = frameAt(output, 1.0), after = frameAt(output, 1.8);
        QVERIFY(!before.isNull() && !during.isNull() && !after.isNull());
        QCOMPARE(during.size(), QSize(640, 360));
        const double textVariance = greyVariance(before, pixelated);
        qInfo("text %.0f, pixelated %.0f, blurred %.0f, outside %.0f, after %.0f", textVariance,
              greyVariance(during, pixelated), greyVariance(during, blurred), greyVariance(during, outside),
              greyVariance(after, blurred));
        QVERIFY(textVariance > 2000);
        QVERIFY(greyVariance(before, blurred) > 2000);
        QVERIFY(greyVariance(during, pixelated) < 400);
        QVERIFY(greyVariance(during, blurred) < 400);
        // Only there, and only then.
        QVERIFY(greyVariance(during, outside) > 2000);
        QVERIFY(greyVariance(after, blurred) > 2000);
    }

    // A blur is a privacy feature: not one frame of what it covers may show, even where the
    // frame the decoder picks is a frame away from the time the timeline names (spans and cuts
    // off the frame grid, sped-up clips) or a frame is held on after its decoder ran out. The
    // recording here is busy noise from 1 s to 2 s only (and ends at 2.2 s), so any busy frame
    // in the export is a leak.
    void blurNeverLeaksAFrame_data()
    {
        QTest::addColumn<QJsonArray>("clips");
        QTest::addColumn<double>("start");
        QTest::addColumn<double>("end");
        QTest::addColumn<int>("fps");
        const auto clip = [](const char *id, double in, double out, double speed) {
            return QJsonObject{{"id", id}, {"in", in}, {"out", out}, {"speed", speed}};
        };
        QTest::newRow("span starts off the grid") << QJsonArray{clip("c1", 0, 2.2, 1)} << 1.005 << 2.0 << 60;
        QTest::newRow("clip starts off the grid") << QJsonArray{clip("c1", 0.495, 2.2, 1)} << 1.0 << 2.0 << 30;
        // Each of these showed a frame of the noise before the blur was padded.
        QTest::newRow("sped-up clip") << QJsonArray{clip("c1", 0.678, 2.2, 4)} << 0.9937 << 1.9956 << 30;
        QTest::newRow("span ends at a cut off the grid")
            << QJsonArray{clip("c1", 0, 1.534, 1), clip("c2", 1.991, 2.2, 1)} << 1.0 << 2.0 << 60;
        // The second clip is past the recording's last frame: the first one's last frame (under
        // the blur) holds on through it.
        QTest::newRow("frame held past the end")
            << QJsonArray{clip("c1", 0, 1.5, 1), clip("c2", 2.4, 2.9, 1)} << 1.0 << 1.5 << 30;
    }

    void blurNeverLeaksAFrame()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QFETCH(QJsonArray, clips);
        QFETCH(double, start);
        QFETCH(double, end);
        QFETCH(int, fps);
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QString bundle = temporary.filePath(QStringLiteral("leak.omareel"));
        QVERIFY(QDir().mkpath(bundle));
        QProcess makeVideo;
        makeVideo.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
            QStringLiteral("nullsrc=s=320x180:r=60:d=2.2,format=gray,"
                           "geq=lum='if(between(N,60,119),random(1)*255,128)'"),
            QStringLiteral("-c:v"), QStringLiteral("libx264"), QStringLiteral("-crf"), QStringLiteral("12"),
            QStringLiteral("-pix_fmt"), QStringLiteral("yuv420p"), QDir(bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(makeVideo.waitForFinished(30000) && makeVideo.exitCode() == 0);
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(R"({"fps":60,"first_frame_us":1000})");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();

        QJsonObject project = ProjectJson::defaults(QStringLiteral("Leak"), 2.2);
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("none"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        project.insert(QStringLiteral("clips"), clips);
        project.insert(QStringLiteral("overlays"), QJsonArray{QJsonObject{{"id", "b"}, {"kind", "blur"}, {"anchor", "screen"},
            {"start", start}, {"end", end}, {"rect", QJsonObject{{"x", 0.2}, {"y", 0.2}, {"w", 0.6}, {"h", 0.6}}},
            {"style", "pixelate"}, {"amount", 0.0}}});
        QString error;
        QVERIFY2(ProjectJson::save(project, QDir(bundle).filePath(QStringLiteral("project.json")), &error),
                 qPrintable(error));
        const QString output = temporary.filePath(QStringLiteral("leak.mp4"));
        QVERIFY2(exportBundle(bundle, output, &error, fps), qPrintable(error));

        // A few pixels in from the region's edges.
        const QList<double> variances = frameVariances(output, 320, 180, QRectF(0.22, 0.22, 0.56, 0.56));
        QVERIFY(variances.size() > 10);
        QStringList leaks;
        for (qsizetype i = 0; i < variances.size(); ++i)
            if (variances[i] > 300) leaks << QStringLiteral("%1 (%2)").arg(i).arg(qRound(variances[i]));
        QVERIFY2(leaks.isEmpty(), qPrintable(QStringLiteral("busy frames: ") + leaks.join(QStringLiteral(", "))));
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

    // `omareel frame`: stills seek straight to their frame, so they meet the screen's gaps where
    // the export's decoder had walked to them (the same recording as above).
    void rendersStillsAndAContactSheet()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QString bundle = temporary.filePath(QStringLiteral("frames.omareel"));
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
        QJsonObject project = ProjectJson::defaults(QStringLiteral("Frames"), 3.0);
        project.insert(QStringLiteral("clips"), QJsonArray{ProjectJson::clip(QStringLiteral("a"), 1.0, 2.0),
                                                           ProjectJson::clip(QStringLiteral("b"), 2.0, 3.0)});
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"background", "color"}, QStringLiteral("#00c000"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        QString error;
        QVERIFY2(ProjectJson::save(project, QDir(bundle).filePath(QStringLiteral("project.json")), &error),
                 qPrintable(error));

        Exporter exporter;
        StillOptions options;
        options.bundlePath = bundle;
        options.width = 640;
        options.fps = 30;
        // Past the end shows the last frame.
        options.times = {0.5, 1.3, 1.7, 99.0};
        QList<Still> stills;
        double duration = 0.0;
        QVERIFY2(exporter.renderStills(options, &stills, &duration, &error), qPrintable(error));
        QVERIFY(qAbs(duration - 2.0) < 1e-9);
        QCOMPARE(stills.size(), 4);
        for (const Still &still : std::as_const(stills)) {
            QCOMPARE(still.image.size(), QSize(640, 360));
            QVERIFY(isGreen(still.image.pixelColor(4, 4)));
        }
        QVERIFY(isRed(stills[0].image.pixelColor(320, 180)));
        // The second clip starts on a still screen and holds red until its source reaches 2.5 s.
        QVERIFY(isRed(stills[1].image.pixelColor(320, 180)));
        QVERIFY(isBlue(stills[2].image.pixelColor(320, 180)));
        QVERIFY(isBlue(stills[3].image.pixelColor(320, 180)));
        // Each still shows the start of its frame.
        QCOMPARE(stills[1].time, 39 / 30.0);
        QCOMPARE(stills[3].time, 59 / 30.0);

        // A contact sheet of four spread over the video, two by two with a gap around each.
        options.times.clear();
        options.count = 4;
        options.width = 320;
        QVERIFY2(exporter.renderStills(options, &stills, &duration, &error), qPrintable(error));
        QCOMPARE(stills.size(), 4);
        QCOMPARE(stills.first().image.size(), QSize(320, 180));
        QCOMPARE(stills[0].time, 7 / 30.0);
        const QImage sheet = Exporter::contactSheet(stills, 2, 8);
        QCOMPARE(sheet.size(), QSize(2 * 320 + 3 * 8, 2 * 180 + 3 * 8));
        QVERIFY(isRed(sheet.pixelColor(8 + 160, 8 + 60)));
        QVERIFY(isBlue(sheet.pixelColor(8 + 320 + 8 + 160, 8 + 180 + 8 + 60)));
        QVERIFY(sheet.pixelColor(2, 2).value() < 40);
    }

    // A zoom punch at the cut between two recording clips grows the screen past its frame for
    // a moment, and the sound fades across the cut; without a transition nothing changes.
    void zoomPunchShowsAcrossACut()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        const QString bundle = temporary.filePath(QStringLiteral("cut.omareel"));
        QVERIFY(QDir().mkpath(bundle));
        QVERIFY(makeToneVideo(QStringLiteral("blue"), QDir(bundle).filePath(QStringLiteral("screen.mp4")), 2.0,
                              QStringLiteral("640x360"), 440, 48000, 2));
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(R"({"fps":30,"first_frame_us":1000})");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();

        // 0.4 s cut out of the middle: the cut plays at 0.8 s.
        QJsonObject project = ProjectJson::defaults(QStringLiteral("Cut"), 2.0);
        project.insert(QStringLiteral("clips"), QJsonArray{ProjectJson::clip(QStringLiteral("a"), 0.0, 0.8),
                                                           ProjectJson::clip(QStringLiteral("b"), 1.2, 2.0)});
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"background", "color"}, QStringLiteral("#00c000"));
        ProjectJson::set(project, {"frame", "shadow", "enabled"}, false);
        ProjectJson::set(project, {"cursor", "visible"}, false);
        const QString projectPath = QDir(bundle).filePath(QStringLiteral("project.json"));
        QString error;
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));
        const QString plain = temporary.filePath(QStringLiteral("plain.mp4"));
        QVERIFY2(exportBundle(bundle, plain, &error), qPrintable(error));

        project.insert(QStringLiteral("transition"), QStringLiteral("zoom-punch"));
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));
        const QString punched = temporary.filePath(QStringLiteral("punch.mp4"));
        QVERIFY2(exportBundle(bundle, punched, &error), qPrintable(error));
        QVERIFY(qAbs(probeDuration(punched) - 1.6) < 0.05);

        // The 16:9 screen fills x 64..576 of the 640x360 canvas; 6% bigger at the cut, it
        // reaches x 49.
        for (const double t : {0.5, 0.8, 1.4})
            QVERIFY2(isGreen(pixelAt(plain, t, 58, 180)), qPrintable(QString::number(t)));
        QVERIFY(isGreen(pixelAt(punched, 0.5, 58, 180)));
        QVERIFY(isBlue(pixelAt(punched, 0.8, 58, 180)));
        QVERIFY(isGreen(pixelAt(punched, 1.4, 58, 180)));
        QVERIFY(isBlue(pixelAt(punched, 0.8, 320, 180)));

        // The tone dips at the cut only with the transition.
        const double steady = maxVolume(punched, 0.3, 0.6);
        QVERIFY2(maxVolume(punched, 0.795, 0.805) < steady - 3.0, qPrintable(QString::number(steady)));
        QVERIFY(maxVolume(plain, 0.795, 0.805) > maxVolume(plain, 0.3, 0.6) - 3.0);
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

    // Background music runs under the whole edited video, looping, and drops under the
    // recording's own sound when it ducks.
    void backgroundMusicPlaysUnderTheVideo()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QTemporaryDir temporary;
        QVERIFY(temporary.isValid());
        // A recording with a loud 2 kHz tone from 1.5 s on, and 1 s of 220 Hz music.
        const QString bundle = temporary.filePath(QStringLiteral("music.omareel"));
        QVERIFY(QDir(bundle).mkpath(QStringLiteral("media")));
        QProcess makeScreen;
        makeScreen.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("color=c=blue:size=640x360:rate=30"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
            QStringLiteral("sine=frequency=2000:sample_rate=48000,volume=4,volume=enable='lt(t,1.5)':volume=0"),
            QStringLiteral("-t"), QStringLiteral("3"), QStringLiteral("-ac"), QStringLiteral("2"),
            QStringLiteral("-c:v"), QStringLiteral("libx264"), QStringLiteral("-pix_fmt"), QStringLiteral("yuv420p"),
            QStringLiteral("-c:a"), QStringLiteral("aac"), QDir(bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(makeScreen.waitForFinished(30000));
        QCOMPARE(makeScreen.exitCode(), 0);
        QProcess makeMusic;
        makeMusic.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
            QStringLiteral("sine=frequency=220:sample_rate=44100:duration=1"),
            QDir(bundle).filePath(QStringLiteral("media/song.wav"))});
        QVERIFY(makeMusic.waitForFinished(30000));
        QCOMPARE(makeMusic.exitCode(), 0);
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(R"({"fps":30,"first_frame_us":1000,"audio":{"desktop":true,"mic":true}})");
        capture.close();
        QFile input(QDir(bundle).filePath(QStringLiteral("input.jsonl")));
        QVERIFY(input.open(QIODevice::WriteOnly));
        input.close();

        QJsonObject project = ProjectJson::defaults(QStringLiteral("Music"), 3.0);
        ProjectJson::set(project, {"background", "type"}, QStringLiteral("color"));
        ProjectJson::set(project, {"cursor", "visible"}, false);
        ProjectJson::set(project, {"camera", "enabled"}, false);
        // A cut: the music follows the edited timeline, not the recording.
        project.insert(QStringLiteral("clips"), QJsonArray{
            ProjectJson::clip(QStringLiteral("a"), 0.0, 1.0), ProjectJson::clip(QStringLiteral("b"), 1.5, 3.0)});
        QJsonObject music{{"file", "media/song.wav"}, {"volume", 1.0}, {"fadeIn", 0.0}, {"fadeOut", 0.0},
                          {"duck", false}, {"loop", true}, {"start", 0.0}};
        project.insert(QStringLiteral("music"), music);
        const QString projectPath = QDir(bundle).filePath(QStringLiteral("project.json"));
        QString error;
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));

        const QString lows = QStringLiteral("lowpass=f=400,lowpass=f=400,lowpass=f=400");
        const QString highs = QStringLiteral("highpass=f=1200,highpass=f=1200,highpass=f=1200");
        const QString plain = temporary.filePath(QStringLiteral("plain.mp4"));
        QVERIFY2(exportBundle(bundle, plain, &error), qPrintable(error));
        QVERIFY2(qAbs(probeDuration(plain) - 2.5) < 0.06, qPrintable(QString::number(probeDuration(plain))));
        // The music alone over the first clip, then still there (looped) under the recording's
        // tone, at the same level.
        const double alone = meanVolume(plain, 0.1, 0.9, lows);
        const double under = meanVolume(plain, 1.2, 2.4, lows);
        QVERIFY2(alone > -30.0, qPrintable(QString::number(alone)));
        QVERIFY2(qAbs(under - alone) < 2.0, qPrintable(QStringLiteral("%1 vs %2").arg(under).arg(alone)));
        QVERIFY(meanVolume(plain, 1.2, 2.4, highs) > -20.0);

        music.insert(QStringLiteral("duck"), true);
        project.insert(QStringLiteral("music"), music);
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));
        const QString ducked = temporary.filePath(QStringLiteral("ducked.mp4"));
        QVERIFY2(exportBundle(bundle, ducked, &error), qPrintable(error));
        const double duckedAlone = meanVolume(ducked, 0.1, 0.9, lows);
        const double duckedUnder = meanVolume(ducked, 1.3, 2.4, lows);
        QVERIFY2(qAbs(duckedAlone - alone) < 1.0, qPrintable(QStringLiteral("%1 vs %2").arg(duckedAlone).arg(alone)));
        QVERIFY2(duckedUnder < alone - 10.0, qPrintable(QStringLiteral("%1 vs %2").arg(duckedUnder).arg(alone)));
        // The recording keeps its level.
        QVERIFY(qAbs(meanVolume(ducked, 1.2, 2.4, highs) - meanVolume(plain, 1.2, 2.4, highs)) < 0.5);

        // A WebM written to a pipe, as browsers record, has no length in its header: it is
        // still music, read to its end and looped.
        QProcess makePiped;
        makePiped.setStandardOutputFile(QDir(bundle).filePath(QStringLiteral("media/piped.webm")));
        makePiped.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
            QStringLiteral("sine=frequency=220:sample_rate=48000:duration=1"), QStringLiteral("-c:a"),
            QStringLiteral("libopus"), QStringLiteral("-f"), QStringLiteral("webm"), QStringLiteral("-")});
        QVERIFY(makePiped.waitForFinished(30000));
        QCOMPARE(makePiped.exitCode(), 0);
        music.insert(QStringLiteral("file"), QStringLiteral("media/piped.webm"));
        music.insert(QStringLiteral("duck"), false);
        project.insert(QStringLiteral("music"), music);
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));
        const QString piped = temporary.filePath(QStringLiteral("piped.mp4"));
        QVERIFY2(exportBundle(bundle, piped, &error), qPrintable(error));
        const double pipedUnder = meanVolume(piped, 1.2, 2.4, lows);
        QVERIFY2(qAbs(pipedUnder - alone) < 2.0, qPrintable(QStringLiteral("%1 vs %2").arg(pipedUnder).arg(alone)));
        music.insert(QStringLiteral("file"), QStringLiteral("media/song.wav"));
        project.insert(QStringLiteral("music"), music);
        QVERIFY2(ProjectJson::save(project, projectPath, &error), qPrintable(error));

        // Missing music fails the export early, saying so.
        QVERIFY(QFile::remove(QDir(bundle).filePath(QStringLiteral("media/song.wav"))));
        QVERIFY(!exportBundle(bundle, temporary.filePath(QStringLiteral("missing.mp4")), &error));
        QVERIFY2(error.contains(QStringLiteral("Music")), qPrintable(error));
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
