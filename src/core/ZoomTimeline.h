#pragma once

#include <QJsonValue>
#include <QPointF>
#include <QString>
#include <QVector>

namespace OmaRecord {

struct ZoomSegment {
    QString id;
    double start = 0.0;
    double end = 0.0;
    double level = 2.0;
    bool automaticTarget = true;
    QPointF target = QPointF(0.5, 0.5);
};

struct ZoomStyle {
    double transitionIn = 0.7;
    double transitionOut = 0.7;
    QString easing = QStringLiteral("easeInOutCubic");
    double followSmoothing = 0.85;
    double followDeadZone = 0.12;
};

struct ZoomFrame {
    double scale = 1.0;
    QPointF center = QPointF(0.5, 0.5);
};

class ZoomTimeline
{
public:
    static QVector<ZoomSegment> generate(const QVector<double> &clickDownTimes,
                                         double duration);
    static double levelAt(const QVector<ZoomSegment> &segments, double time,
                          const ZoomStyle &style = {});
    static QPointF centerAt(const QVector<ZoomSegment> &segments, double time,
                            const QPointF &cursor, const QPointF &previousCenter,
                            double deltaTime, const ZoomStyle &style = {});
    static QPointF clampCenter(const QPointF &center, double level);
    static QJsonValue targetToJson(const ZoomSegment &segment);
};

} // namespace OmaRecord
