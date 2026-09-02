#pragma once

#include <QPointF>
#include <QVector>

namespace OmaRecord {

struct CursorSample { double time = 0.0; QPointF position; };

class CursorPath
{
public:
    static QVector<CursorSample> decimate(const QVector<CursorSample> &raw, double fps);
    static QPointF positionAt(const QVector<CursorSample> &samples, double time);
};

} // namespace OmaRecord
