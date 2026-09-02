#include "core/ClipTimeline.h"

#include <QtTest>

using namespace OmaRecord;

class ClipTimelineTest : public QObject
{
    Q_OBJECT
private slots:
    void frameSourceMappingWithSpeedChanges()
    {
        ClipTimeline timeline({Clip{"slow", 1.0, 3.0, 0.5},
                               Clip{"fast", 10.0, 14.0, 2.0}});
        const double fps = 60.0;
        QCOMPARE(timeline.sourceTime(0 / fps), 1.0);
        QCOMPARE(timeline.sourceTime(120 / fps), 2.0);
        QCOMPARE(timeline.sourceTime(240 / fps), 3.0);
        QCOMPARE(timeline.sourceTime(270 / fps), 11.0);
        QCOMPARE(timeline.sourceTime(359 / fps), 13.966666666666667);
    }

    void mappingAndOperations()
    {
        ClipTimeline timeline({Clip{"a", 2.0, 6.0, 1.0}, Clip{"b", 10.0, 14.0, 2.0}});
        QCOMPARE(timeline.totalDuration(), 6.0);
        QCOMPARE(timeline.sourceTime(1.5), 3.5);
        QCOMPARE(timeline.sourceTime(5.0), 12.0);
        QCOMPARE(timeline.outputTime(1, 12.0), 5.0);
        QVERIFY(timeline.split(1.0));
        QCOMPARE(timeline.clips().size(), 3);
        QCOMPARE(timeline.clips()[0].out, 3.0);
        QCOMPARE(timeline.clips()[1].in, 3.0);
        QVERIFY(timeline.trim(1, 3.5, 5.5));
        QCOMPARE(timeline.totalDuration(), 5.0);
        QVERIFY(timeline.remove(0));
        QCOMPARE(timeline.sourceTime(0.0), 3.5);
    }
};

QTEST_APPLESS_MAIN(ClipTimelineTest)
#include "test_cliptimeline.moc"
