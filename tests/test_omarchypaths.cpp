#include "core/OmarchyPaths.h"
#include "core/RecordingMetadata.h"
#include "core/Theme.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryDir>
#include <QtTest>

using namespace OmaRecord;

class OmarchyPathsTest : public QObject
{
    Q_OBJECT
private slots:
    void currentBackgroundUsesSymlinkThenFallback()
    {
        QTemporaryDir state;
        QVERIFY(state.isValid());
        const QString backgrounds = state.filePath(QStringLiteral("theme/backgrounds"));
        QVERIFY(QDir().mkpath(backgrounds));
        const QString first = QDir(backgrounds).filePath(QStringLiteral("a-first.jpg"));
        const QString current = QDir(backgrounds).filePath(QStringLiteral("z-current.jpg"));
        for (const QString &path : {first, current}) {
            QFile file(path);
            QVERIFY(file.open(QIODevice::WriteOnly));
            QVERIFY(file.write("image") > 0);
        }
        qputenv("OMARECORD_OMARCHY_STATE_DIR", state.path().toUtf8());
        const QString link = state.filePath(QStringLiteral("background"));
        QVERIFY(QFile::link(current, link));
        QCOMPARE(OmarchyPaths::currentBackground(), current);
        QVERIFY(QFile::remove(link));
        QCOMPARE(OmarchyPaths::currentBackground(), first);
        qunsetenv("OMARECORD_OMARCHY_STATE_DIR");
    }

    void themeDerivesShadesAndMode()
    {
        QTemporaryDir state;
        QVERIFY(state.isValid());
        QVERIFY(QDir().mkpath(state.filePath(QStringLiteral("theme"))));
        QFile colors(state.filePath(QStringLiteral("theme/colors.toml")));
        QVERIFY(colors.open(QIODevice::WriteOnly | QIODevice::Text));
        colors.write("background = \"#faf4ed\"\nforeground = \"#575279\"\naccent = \"#907aa9\"\n");
        colors.close();
        qputenv("OMARECORD_OMARCHY_STATE_DIR", state.path().toUtf8());
        Theme inferred;
        QCOMPARE(inferred.lighterBackground(), QColor(QStringLiteral("#faf4ed")).lighter(112));
        QCOMPARE(inferred.darkBackground(), QColor(qRound(250 * .75), qRound(244 * .75), qRound(237 * .75)));
        QVERIFY(!inferred.dark());

        QVERIFY(colors.open(QIODevice::Append | QIODevice::Text));
        colors.write("mode = \"dark\"\n");
        colors.close();
        Theme explicitMode;
        QVERIFY(explicitMode.dark());
        qunsetenv("OMARECORD_OMARCHY_STATE_DIR");
    }

    void readsCaptureDuration()
    {
        QTemporaryDir bundle;
        QVERIFY(bundle.isValid());
        QFile capture(bundle.filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(QJsonDocument(QJsonObject{{QStringLiteral("first_frame_us"), 1250000},
                                                {QStringLiteral("stopped_us"), 9750000}}).toJson());
        capture.close();
        QCOMPARE(RecordingMetadata::captureDuration(bundle.path()), 8.5);
    }
};

QTEST_APPLESS_MAIN(OmarchyPathsTest)
#include "test_omarchypaths.moc"
