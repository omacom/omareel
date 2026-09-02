#include "CursorPath.h"

#include <algorithm>
#include <cmath>

using namespace OmaRecord;

QVector<CursorSample> CursorPath::decimate(const QVector<CursorSample> &raw, double fps,
                                           double minDistance)
{
    if (raw.isEmpty()) return {};
    QVector<CursorSample> result{raw.first()};
    const double window = 1.0 / std::max(1.0, fps);
    for (qsizetype i = 1; i < raw.size(); ++i) {
        const auto &last = result.last();
        const QPointF delta = raw[i].position - last.position;
        if (raw[i].time - last.time < window || std::hypot(delta.x(), delta.y()) < minDistance) continue;
        result << raw[i];
    }
    if (raw.last().time > result.last().time) result << raw.last();
    return result;
}

QPointF CursorPath::positionAt(const QVector<CursorSample> &samples, double time)
{
    if (samples.isEmpty()) return {0.5, 0.5};
    const auto upper = std::upper_bound(samples.begin(), samples.end(), time,
        [](double t, const CursorSample &sample) { return t < sample.time; });
    if (upper == samples.begin()) return upper->position;
    if (upper == samples.end()) return samples.last().position;
    const auto &b = *upper;
    const auto &a = *(upper - 1);
    const double alpha = b.time > a.time ? std::clamp((time - a.time) / (b.time - a.time), 0.0, 1.0) : 0.0;
    return a.position + (b.position - a.position) * alpha;
}
