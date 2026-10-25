#include "core/OmarchyPaths.h"

#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QtTest>

using namespace Omareel;

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
        qputenv("OMAREEL_OMARCHY_STATE_DIR", state.path().toUtf8());
        const QString link = state.filePath(QStringLiteral("background"));
        QVERIFY(QFile::link(current, link));
        QCOMPARE(OmarchyPaths::currentBackground(), current);
        QVERIFY(QFile::remove(link));
        QCOMPARE(OmarchyPaths::currentBackground(), first);
        qunsetenv("OMAREEL_OMARCHY_STATE_DIR");
    }
};

QTEST_APPLESS_MAIN(OmarchyPathsTest)
#include "test_omarchypaths.moc"
