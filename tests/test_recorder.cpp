#include "record/Recorder.h"

#include <QtTest>
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
};

QTEST_APPLESS_MAIN(RecorderTest)
#include "test_recorder.moc"
