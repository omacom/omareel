#include "core/InputLog.h"

#include <QtTest>

using namespace OmaRecord;

class InputLogTest : public QObject
{
    Q_OBJECT
private slots:
    void parsesConvertsAndTrims()
    {
        const QString fixture = QFINDTESTDATA("fixtures/input.jsonl");
        QVERIFY(!fixture.isEmpty());
        QString error;
        const auto log = InputLog::load(fixture, 1000000, 1.0, &error);
        QVERIFY2(error.isEmpty(), qPrintable(error));
        QCOMPARE(log.events().size(), 4);
        QCOMPARE(log.moves().size(), 1);
        QCOMPARE(log.events().first().time, 0.0);
        QCOMPARE(log.clickDowns().size(), 1);
        QCOMPARE(log.keyCount(), 1);
        QCOMPARE(log.events()[2].modifiers, QStringList{QStringLiteral("ctrl")});
    }
};

QTEST_APPLESS_MAIN(InputLogTest)
#include "test_inputlog.moc"
