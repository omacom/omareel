#include "core/ZoomTimeline.h"

#include <QtTest>

using namespace Omareel;

class ZoomTimelineTest : public QObject
{
    Q_OBJECT
private slots:
    void amendmentGenerationRules()
    {
        const auto single = ZoomTimeline::generate({5.0}, 20.0);
        QCOMPARE(single.size(), 1);
        QCOMPARE(single[0].start, 4.7);
        QCOMPARE(single[0].end, 7.5);
        const auto merged = ZoomTimeline::generate({5.0, 7.0}, 20.0);
        QCOMPARE(merged.size(), 1);
        QCOMPARE(merged[0].start, 4.7);
        QCOMPARE(merged[0].end, 9.5);
        QVERIFY(ZoomTimeline::generate({19.5}, 20.0).isEmpty());
        const auto clamped = ZoomTimeline::generate({18.9}, 20.0);
        QCOMPARE(clamped.first().end, 19.2);
    }

    void autoGroupsHoldAndRetarget()
    {
        ZoomStyle style;
        style.snapToEdgesRatio = 0.0;
        QVector<ZoomSegment> zooms{{"z", 0.0, 4.0, 2.0, true, {}}};
        QVector<CursorSample> samples{{0.0, {0.20, 0.30}}, {1.0, {0.30, 0.40}},
                                      {2.0, {0.70, 0.65}}, {3.0, {0.75, 0.70}}};
        const auto first = ZoomTimeline::targetAt(zooms, samples, 0.5, style);
        const auto held = ZoomTimeline::targetAt(zooms, samples, 1.5, style);
        QCOMPARE(first.center, held.center);
        QVERIFY(std::abs(first.center.x() - 0.25) < 0.0001);
        QVERIFY(std::abs(first.center.y() - 0.35) < 0.0001);
        const auto second = ZoomTimeline::targetAt(zooms, samples, 2.1, style);
        QVERIFY(second.center.x() > 0.6);
        QVERIFY(second.center != first.center);
    }

    void edgeSnapKeepsViewportInsideFrame()
    {
        ZoomStyle style;
        QVector<ZoomSegment> zooms{{"z", 0.0, 2.0, 2.0, true, {}}};
        const auto frame = ZoomTimeline::targetAt(zooms, {{0.0, {0.02, 0.98}}}, 1.0, style);
        QCOMPARE(frame.center, QPointF(0.25, 0.75));
        const double half = 0.5 / frame.scale;
        QVERIFY(frame.center.x() - half >= 0.0);
        QVERIFY(frame.center.y() + half <= 1.0);
    }
};

QTEST_APPLESS_MAIN(ZoomTimelineTest)
#include "test_zoomtimeline.moc"
