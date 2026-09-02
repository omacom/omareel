#include "core/ZoomTimeline.h"

#include <QtTest>
#include <cmath>

using namespace OmaRecord;

class ZoomTimelineTest : public QObject
{
    Q_OBJECT
private slots:
    void generationRules()
    {
        const auto zooms = ZoomTimeline::generate({1.0, 2.5, 6.0, 6.4}, 10.0);
        QCOMPARE(zooms.size(), 2);
        QCOMPARE(zooms[0].start, 0.5);
        QCOMPARE(zooms[0].end, 4.25);
        QCOMPARE(zooms[1].start, 5.5);
        QCOMPARE(zooms[1].end, 8.15);
        QVERIFY(ZoomTimeline::generate({}, 10.0).isEmpty());
        const auto edge = ZoomTimeline::generate({0.1}, 0.6);
        QCOMPARE(edge.first().start, 0.0);
        QCOMPARE(edge.first().end, 0.6);
    }

    void levelsAndOverlap()
    {
        ZoomStyle style;
        style.transitionIn = style.transitionOut = 1.0;
        QVector<ZoomSegment> one{{"z", 1.0, 4.0, 2.0, true, {}}};
        QCOMPARE(ZoomTimeline::levelAt(one, 0.9, style), 1.0);
        QCOMPARE(ZoomTimeline::levelAt(one, 1.0, style), 1.0);
        QCOMPARE(ZoomTimeline::levelAt(one, 1.5, style), 1.5);
        QCOMPARE(ZoomTimeline::levelAt(one, 2.0, style), 2.0);
        QCOMPARE(ZoomTimeline::levelAt(one, 4.0, style), 1.0);
        QVector<ZoomSegment> touching{{"a", 0.0, 2.0, 2.0, true, {}},
                                      {"b", 2.0, 4.0, 2.0, true, {}}};
        QCOMPARE(ZoomTimeline::levelAt(touching, 2.0, style), 2.0);
        QVector<ZoomSegment> overlap{{"a", 0.0, 3.0, 2.0, true, {}},
                                     {"b", 2.0, 5.0, 3.0, true, {}}};
        QCOMPARE(ZoomTimeline::levelAt(overlap, 2.5, style), 2.5);
    }

    void centerClampAndDeadZone()
    {
        QCOMPARE(ZoomTimeline::clampCenter({0.0, 1.0}, 2.0), QPointF(0.25, 0.75));
        ZoomStyle style;
        style.transitionIn = style.transitionOut = 0.0;
        style.followDeadZone = 0.12;
        QVector<ZoomSegment> zoom{{"z", 0.0, 5.0, 2.0, true, {}}};
        const QPointF previous(0.5, 0.5);
        QCOMPARE(ZoomTimeline::centerAt(zoom, 1.0, {0.52, 0.52}, previous, 1.0 / 60.0, style), previous);
        const auto followed = ZoomTimeline::centerAt(zoom, 1.0, {0.8, 0.5}, previous, 0.1, style);
        QVERIFY(followed.x() > 0.5);
        QVERIFY(followed.x() <= 0.75);
    }
};

QTEST_APPLESS_MAIN(ZoomTimelineTest)
#include "test_zoomtimeline.moc"
