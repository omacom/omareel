#include "core/Project.h"

#include <QTemporaryDir>
#include <QtTest>

using namespace OmaRecord;

class ProjectTest : public QObject
{
    Q_OBJECT
private slots:
    void defaultsAndRoundTrip()
    {
        const Project original = Project::defaults(QStringLiteral("Demo"), 12.5);
        const auto json = original.toJson();
        QCOMPARE(json.value("version").toInt(), 1);
        QVERIFY(json.value("aspect").isNull());
        QCOMPARE(json.value("clips").toArray().first().toObject().value("out").toDouble(), 12.5);
        QCOMPARE(original.frame.padding, 0.10);
        QCOMPARE(original.frame.shadow.intensity, 0.75);
        QCOMPARE(original.zoomStyle.spring.mass, 2.25);
        QCOMPARE(original.cursor.spring.stiffness, 470.0);
        QCOMPARE(original.cursor.clickShrink, 0.8);
        QCOMPARE(original.exportSettings.height, 1080);
        QCOMPARE(original.exportSettings.quality, QStringLiteral("social"));
        QCOMPARE(original.exportSettings.gif.height, 480);
        QVERIFY(allowedAspects().contains(QStringLiteral("3:4")));
        QCOMPARE(allowedClipSpeeds().last(), 24.0);
        QTemporaryDir directory;
        const QString path = directory.filePath(QStringLiteral("project.json"));
        QString error;
        QVERIFY2(original.save(path, &error), qPrintable(error));
        const auto loaded = Project::load(path, &error);
        QVERIFY2(error.isEmpty(), qPrintable(error));
        QCOMPARE(loaded.toJson(), original.toJson());
    }
};

QTEST_APPLESS_MAIN(ProjectTest)
#include "test_project.moc"
