#include "core/ClipTimeline.h"

#include <QtTest>

using namespace Omareel;

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
        QCOMPARE(timeline.sourceTime(240 / fps), 10.0);
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

    void deletesMiddleAndMapsTheNewCut()
    {
        ClipTimeline timeline({Clip{"first", 0.0, 3.0, 1.0}});
        QVERIFY(timeline.deleteRange(1.0, 2.0, QStringLiteral("remainder")));
        QCOMPARE(timeline.clips().size(), 2);
        QCOMPARE(timeline.clips()[0].id, QStringLiteral("first"));
        QCOMPARE(timeline.clips()[0].out, 1.0);
        QCOMPARE(timeline.clips()[1].id, QStringLiteral("remainder"));
        QCOMPARE(timeline.clips()[1].in, 2.0);
        QCOMPARE(timeline.totalDuration(), 2.0);
        QCOMPARE(timeline.sourceTime(0.99), 0.99);
        QCOMPARE(timeline.sourceTime(1.0), 2.0);
    }

    void deletesAcrossClipsWithDifferentSpeeds()
    {
        ClipTimeline timeline({Clip{"normal", 0.0, 2.0, 1.0},
                               Clip{"fast", 4.0, 8.0, 2.0}});
        QVERIFY(timeline.deleteRange(1.5, 2.5, QStringLiteral("unused")));
        QCOMPARE(timeline.clips().size(), 2);
        QCOMPARE(timeline.clips()[0].out, 1.5);
        QCOMPARE(timeline.clips()[1].in, 5.0);
        QCOMPARE(timeline.totalDuration(), 3.0);
        QCOMPARE(timeline.sourceTime(1.5), 5.0);
        QCOMPARE(timeline.sourceTime(2.0), 6.0);
    }

    void rejectsUnrepresentableOrCompleteCutsWithoutChangingClips()
    {
        ClipTimeline timeline({Clip{"only", 0.0, 2.0, 1.0}});
        QVERIFY(!timeline.deleteRange(0.0, 2.0, QStringLiteral("right")));
        QVERIFY(!timeline.deleteRange(0.05, 1.0, QStringLiteral("right")));
        QVERIFY(!timeline.deleteRange(1.0, 1.0, QStringLiteral("right")));
        QVERIFY(!timeline.deleteRange(0.5, 1.5, QStringLiteral("only")));
        QCOMPARE(timeline.clips().size(), 1);
        QCOMPARE(timeline.totalDuration(), 2.0);
    }
};

QTEST_APPLESS_MAIN(ClipTimelineTest)
#include "test_cliptimeline.moc"
