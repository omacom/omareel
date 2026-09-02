#include "core/CursorPath.h"
#include "core/MotionTrack.h"

#include <QtTest>

using namespace OmaRecord;

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
