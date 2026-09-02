#include "ZoomTimeline.h"
#include "Easing.h"

#include <QJsonObject>
#include <algorithm>
#include <cmath>

using namespace OmaRecord;

QVector<ZoomSegment> ZoomTimeline::generate(const QVector<double> &clickDownTimes, double duration)
{
    QVector<double> clicks = clickDownTimes;
    std::sort(clicks.begin(), clicks.end());
    QVector<ZoomSegment> segments;
    if (clicks.isEmpty() || duration <= 0.0) return segments;

    int groupStart = 0;
    for (int i = 1; i <= clicks.size(); ++i) {
        if (i < clicks.size() && clicks[i] - clicks[i - 1] < 2.0) continue;
        double start = std::max(0.0, clicks[groupStart] - 0.5);
        double end = std::min(duration, clicks[i - 1] + 1.75);
        if (end - start < 1.0) {
            end = std::min(duration, start + 1.0);
            start = std::max(0.0, end - 1.0);
        }
        if (!segments.isEmpty() && start - segments.last().end < 0.75) {
            segments.last().end = std::max(segments.last().end, end);
        } else {
            segments << ZoomSegment{QStringLiteral("z%1").arg(segments.size() + 1),
                                    start, end, 2.0, true, QPointF(0.5, 0.5)};
        }
        groupStart = i;
    }
    return segments;
}

double ZoomTimeline::levelAt(const QVector<ZoomSegment> &segments, double time,
                             const ZoomStyle &style)
{
    QVector<ZoomSegment> ordered = segments;
    std::sort(ordered.begin(), ordered.end(), [](const auto &a, const auto &b) {
        return a.start < b.start;
    });
    for (int i = 0; i + 1 < ordered.size(); ++i) {
        const auto &left = ordered[i];
        const auto &right = ordered[i + 1];
        if (right.start > left.end + 1e-9 || time < right.start || time > left.end) continue;
        const double overlap = left.end - right.start;
        if (overlap <= 1e-9) return std::max(left.level, right.level);
        return lerp(left.level, right.level,
                    ease((time - right.start) / overlap, style.easing));
    }
    double result = 1.0;
    for (int i = 0; i < ordered.size(); ++i) {
        const auto &segment = ordered[i];
        if (time < segment.start || time > segment.end) continue;
        double amount = 1.0;
        const double inDuration = std::min(style.transitionIn, segment.end - segment.start);
        const double outDuration = std::min(style.transitionOut, segment.end - segment.start);
        const bool joinedAtStart = i > 0 && ordered[i - 1].end + 1e-9 >= segment.start;
        const bool joinedAtEnd = i + 1 < ordered.size()
            && segment.end + 1e-9 >= ordered[i + 1].start;
        if (!joinedAtStart && inDuration > 0.0 && time < segment.start + inDuration)
            amount = ease((time - segment.start) / inDuration, style.easing);
        if (!joinedAtEnd && outDuration > 0.0 && time > segment.end - outDuration)
            amount = std::min(amount, ease((segment.end - time) / outDuration, style.easing));
        result = std::max(result, lerp(1.0, segment.level, amount));
    }
    return result;
}

QPointF ZoomTimeline::clampCenter(const QPointF &center, double level)
{
    const double half = 0.5 / std::max(1.0, level);
    return QPointF(std::clamp(center.x(), half, 1.0 - half),
                   std::clamp(center.y(), half, 1.0 - half));
}

QPointF ZoomTimeline::centerAt(const QVector<ZoomSegment> &segments, double time,
                               const QPointF &cursor, const QPointF &previousCenter,
                               double deltaTime, const ZoomStyle &style)
{
    const ZoomSegment *active = nullptr;
    for (const auto &segment : segments) {
        if (time >= segment.start && time <= segment.end) {
            if (!active || segment.level > active->level) active = &segment;
        }
    }
    if (!active) return QPointF(0.5, 0.5);
    const double level = levelAt(segments, time, style);
    QPointF target = active->automaticTarget ? cursor : active->target;

    if (active->automaticTarget) {
        const double halfDeadZone = style.followDeadZone / (2.0 * level);
        const QPointF difference = target - previousCenter;
        if (std::abs(difference.x()) <= halfDeadZone
            && std::abs(difference.y()) <= halfDeadZone) {
            target = previousCenter;
        } else {
            const double smoothing = std::clamp(style.followSmoothing, 0.0, 1.0);
            const double tau = smoothing <= 0.0 ? 0.0 : 0.25 * smoothing / 0.85;
            const double alpha = tau <= 0.0 ? 1.0 : 1.0 - std::exp(-deltaTime / tau);
            target = previousCenter + difference * alpha;
        }
    }

    double phase = 1.0;
    if (style.transitionIn > 0.0 && time < active->start + style.transitionIn)
        phase = ease((time - active->start) / style.transitionIn, style.easing);
    if (style.transitionOut > 0.0 && time > active->end - style.transitionOut)
        phase = std::min(phase, ease((active->end - time) / style.transitionOut, style.easing));
    return clampCenter(QPointF(0.5, 0.5) + (target - QPointF(0.5, 0.5)) * phase, level);
}

QJsonValue ZoomTimeline::targetToJson(const ZoomSegment &segment)
{
    if (segment.automaticTarget) return QStringLiteral("auto");
    return QJsonObject{{"x", segment.target.x()}, {"y", segment.target.y()}};
}
