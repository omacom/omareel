#pragma once

#include "CursorPath.h"
#include "Project.h"

#include <QJsonValue>

namespace Omareel {

struct ZoomFrame {
    double scale = 1.0;
    QPointF center = QPointF(0.5, 0.5);
};

class ZoomTimeline
{
public:
    static QVector<ZoomSegment> generate(const QVector<double> &clickDownTimes,
                                         double duration);
    static ZoomFrame targetAt(const QVector<ZoomSegment> &segments,
                              const QVector<CursorSample> &cursorSamples,
                              double time, const ZoomStyle &style = {});
    static QPointF clampCenter(const QPointF &center, double level);
    static QJsonValue targetToJson(const ZoomSegment &segment);
};

} // namespace Omareel
