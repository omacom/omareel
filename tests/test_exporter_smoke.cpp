#include "render/Exporter.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QProcess>
#include <QTemporaryDir>
#include <QTimer>
#include <QtTest>

using namespace OmaRecord;

class ExporterSmokeTest : public QObject
{
    Q_OBJECT
private slots:
    void padsOddDimensionsToEven()
    {
        QCOMPARE(paddedEvenSize(1367, 781), QSize(1368, 782));
        QCOMPARE(paddedEvenSize(1920, 1080), QSize(1920, 1080));
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
        const QString bundle = temporary.filePath(QStringLiteral("synthetic.omarecord"));
        QVERIFY(QDir().mkpath(bundle));
        QVERIFY(QFile::copy(QDir(fixture).filePath(QStringLiteral("capture.json")), QDir(bundle).filePath(QStringLiteral("capture.json"))));
        QVERIFY(QFile::copy(QDir(fixture).filePath(QStringLiteral("input.jsonl")), QDir(bundle).filePath(QStringLiteral("input.jsonl"))));
        QProcess makeVideo;
        makeVideo.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("testsrc2=size=640x360:rate=60"),
            QStringLiteral("-t"), QStringLiteral("2"), QStringLiteral("-c:v"), QStringLiteral("libx264"),
            QDir(bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(makeVideo.waitForFinished(30000));
        QCOMPARE(makeVideo.exitCode(), 0);

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
            QStringLiteral("-select_streams"), QStringLiteral("v:0"),
            QStringLiteral("-show_entries"), QStringLiteral("stream=codec_name,width,height:format=duration"),
            QStringLiteral("-of"), QStringLiteral("json"), output});
        QVERIFY(inspect.waitForFinished(15000));
        QCOMPARE(inspect.exitCode(), 0);
        const auto root = QJsonDocument::fromJson(inspect.readAllStandardOutput()).object();
        const auto stream = root.value("streams").toArray().first().toObject();
        QCOMPARE(stream.value("codec_name").toString(), QStringLiteral("h264"));
        QCOMPARE(stream.value("width").toInt(), 640);
        QCOMPARE(stream.value("height").toInt(), 360);
        const double duration = root.value("format").toObject().value("duration").toString().toDouble();
        QVERIFY(duration > 1.9 && duration < 2.1);

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
        QCOMPARE(QDir(temporary.path()).entryList({QStringLiteral(".omarecord-export-*.mp4")},
                                                  QDir::Files), QStringList());
    }
};

QTEST_MAIN(ExporterSmokeTest)
#include "test_exporter_smoke.moc"
