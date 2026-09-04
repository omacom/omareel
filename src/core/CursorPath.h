#pragma once

#include <QPointF>
#include <QVector>

namespace Omareel {

struct CursorSample { double time = 0.0; QPointF position; };

class CursorPath
{
public:
    // Drops samples closer than one frame window (1/fps) or less than minDistance from the
    // last kept sample. minDistance is in the same units as the sample positions: pass 1.0
    // for pixel coordinates, or 1/max(width,height) for normalized ones.
    static QVector<CursorSample> decimate(const QVector<CursorSample> &raw, double fps,
                                          double minDistance = 1.0);
    static QPointF positionAt(const QVector<CursorSample> &samples, double time);
};

} // namespace Omareel
