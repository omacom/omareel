#include "core/CursorPath.h"
#include "core/MotionTrack.h"

#include <QtTest>

using namespace Omareel;

class CursorPathTest : public QObject
{
    Q_OBJECT
private slots:
    void decimatesAndInterpolates()
    {
        QVector<CursorSample> raw{{0.0, {0, 0}}, {0.005, {10, 0}},
                                  {0.020, {0.5, 0}}, {0.040, {20, 10}}};
        const auto kept = CursorPath::decimate(raw, 60.0);
        QCOMPARE(kept.size(), 2);
        QCOMPARE(CursorPath::positionAt({{0.0, {0, 0}}, {1.0, {100, 50}}}, 0.5), QPointF(50, 25));
    }

    // Regression: samples are normalized before decimation, so the one-pixel threshold must be
    // normalized too. With the bug only the first and last samples survived and the cursor
    // drifted in a straight line from start to end for the whole recording.
    void decimationKeepsIntermediateSamplesInNormalizedSpace()
    {
        Project project = Project::defaults(QStringLiteral("decimate"), 2.0);
        project.zooms.clear();
        QVector<InputEvent> events;
        for (int i = 0; i <= 200; ++i) {
            InputEvent move; move.kind = InputKind::Move; move.time = i / 100.0;
            const double leg = i <= 100 ? i : 200 - i;      // out to (100,100) and back to (0,0)
            move.position = {leg, leg};
            events << move;
        }
        const auto track = MotionTrack::build(2.0, 200, 200, events, project);
        const MotionSample mid = track.sample(1.05);        // just after the turnaround, spring settled
        QVERIFY2(mid.cursorX > 0.35 && mid.cursorY > 0.35,
                 qPrintable(QStringLiteral("cursor at %1,%2 — path collapsed to its endpoints")
                                .arg(mid.cursorX).arg(mid.cursorY)));
    }

    void springConvergesWithoutTwoPercentOvershoot()
    {
        Project project = Project::defaults(QStringLiteral("spring"), 2.0);
        project.zooms.clear();
        QVector<InputEvent> events;
        InputEvent first; first.kind = InputKind::Move; first.time = 0.0; first.position = {0, 50};
        InputEvent second = first; second.time = 0.1; second.position = {100, 50};
        events << first << second;
        const auto track = MotionTrack::build(2.0, 100, 100, events, project);
        double maximum = 0.0;
        for (int i = 0; i <= 480; ++i) maximum = std::max(maximum, track.sample(i / 240.0).cursorX);
        QVERIFY(maximum <= 1.02);
        QVERIFY(track.sample(2.0).cursorX > 0.999);
    }
};

QTEST_APPLESS_MAIN(CursorPathTest)
#include "test_cursorpath.moc"
