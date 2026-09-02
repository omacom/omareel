#pragma once

#include <QPointF>
#include <QVector>

namespace OmaRecord {

struct CursorSample {
    double time = 0.0;
    QPointF position;
};

struct ClickAnimation {
    double scale = 1.0;
    bool rippleVisible = false;
    double rippleRadius = 0.0;
    double rippleOpacity = 0.0;
};

class CursorPath
{
public:
    static QVector<CursorSample> resample(const QVector<CursorSample> &raw,
                                          const QVector<double> &frameTimes);
    static QVector<CursorSample> smooth(const QVector<CursorSample> &samples,
                                        double smoothing);
    static ClickAnimation clickAnimation(double time, const QVector<double> &downTimes,
                                         double clickShrink = 0.85,
                                         double outputScale1080 = 1.0);
    static double idleOpacity(double time, double lastMovementTime, int hideWhenIdleMs);
};

} // namespace OmaRecord
