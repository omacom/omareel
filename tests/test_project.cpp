#include "core/Project.h"

#include <QJsonArray>
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
        const auto json = original.json();
        QCOMPARE(json.value("version").toInt(), 1);
        QCOMPARE(json.value("aspect").toString(), QStringLiteral("auto"));
        QCOMPARE(json.value("clips").toArray().first().toObject().value("out").toDouble(), 12.5);
        QCOMPARE(json.value("cursor").toObject().value("smoothing").toDouble(), 0.8);
        QTemporaryDir directory;
        const QString path = directory.filePath(QStringLiteral("project.json"));
        QString error;
        QVERIFY2(original.save(path, &error), qPrintable(error));
        const auto loaded = Project::load(path, &error);
        QVERIFY2(error.isEmpty(), qPrintable(error));
        QCOMPARE(loaded.json(), original.json());
    }
};

QTEST_APPLESS_MAIN(ProjectTest)
#include "test_project.moc"
