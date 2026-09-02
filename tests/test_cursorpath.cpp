#include "core/CursorPath.h"

#include <QtTest>
#include <cmath>

using namespace OmaRecord;

class CursorPathTest : public QObject
{
    Q_OBJECT
private slots:
    void resamplesAndSpringConverges()
    {
        QVector<CursorSample> raw{{0.0, {0, 0}}, {1.0, {100, 50}}};
        const auto sampled = CursorPath::resample(raw, {0.0, 0.5, 1.0});
        QCOMPARE(sampled[1].position, QPointF(50, 25));

        QVector<CursorSample> step{{0.0, {0, 0}}};
        for (int i = 1; i <= 60; ++i) step << CursorSample{i / 60.0, {100, 0}};
        const auto smooth = CursorPath::smooth(step, 0.8);
        QVERIFY(std::abs(smooth.last().position.x() - 100.0) < 0.01);
        for (const auto &sample : smooth) QVERIFY(sample.position.x() <= 102.0);
        QCOMPARE(CursorPath::smooth(step, 0.0)[1].position, QPointF(100, 0));
    }

    void clickTiming()
    {
        const QVector<double> clicks{1.0};
        QCOMPARE(CursorPath::clickAnimation(0.99, clicks).scale, 1.0);
        QVERIFY(std::abs(CursorPath::clickAnimation(1.08, clicks).scale - 0.85) < 0.001);
        QVERIFY(CursorPath::clickAnimation(1.225, clicks).scale < 1.0);
        QCOMPARE(CursorPath::clickAnimation(1.24, clicks).scale, 1.0);
        const auto middle = CursorPath::clickAnimation(1.225, clicks);
        QVERIFY(middle.rippleVisible);
        QVERIFY(middle.rippleRadius > 0.0);
        QVERIFY(!CursorPath::clickAnimation(1.46, clicks).rippleVisible);
    }
};

QTEST_APPLESS_MAIN(CursorPathTest)
#include "test_cursorpath.moc"
