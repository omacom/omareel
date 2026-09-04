#include "core/CameraTimeline.h"

#include <QtTest>

using namespace Omareel;

class CameraTimelineTest : public QObject
{
    Q_OBJECT
private slots:
    void mapsOffsetAndHoldsEnds()
    {
        const CameraTime delayed = mapCameraTime(1.0, 0.4, 2.0);
        QCOMPARE(delayed.seconds, 0.6);
        QVERIFY(!delayed.beforeStart);
        QVERIFY(!delayed.beyondEnd);

        const CameraTime before = mapCameraTime(0.1, 0.4, 2.0);
        QCOMPARE(before.seconds, 0.0);
        QVERIFY(before.beforeStart);

        const CameraTime earlyCamera = mapCameraTime(0.25, -0.5, 2.0);
        QCOMPARE(earlyCamera.seconds, 0.75);

        const CameraTime held = mapCameraTime(4.0, 0.4, 2.0);
        QCOMPARE(held.seconds, 2.0);
        QVERIFY(held.beyondEnd);
    }
};

QTEST_APPLESS_MAIN(CameraTimelineTest)
#include "test_cameratimeline.moc"
