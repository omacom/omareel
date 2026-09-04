#include "ZoomTimeline.h"

#include <QJsonObject>
#include <algorithm>

using namespace Omareel;

QVector<ZoomSegment> ZoomTimeline::generate(const QVector<double> &clickDownTimes, double duration)
{
    QVector<double> clicks = clickDownTimes;
    std::sort(clicks.begin(), clicks.end());
    QVector<ZoomSegment> result;
    for (double click : clicks) {
        if (duration <= 0.0 || click < 0.0 || click >= duration - 1.0) continue;
        const double start = std::max(0.0, click - 0.3);
        const double end = std::max(start, std::min(click + 2.5, duration - 0.8));
        if (!result.isEmpty() && start - result.last().end <= 2.5) {
            result.last().end = std::max(result.last().end, end);
        } else {
            result << ZoomSegment{QStringLiteral("z%1").arg(result.size() + 1),
                                  start, end, 2.0, true, {0.5, 0.5}};
        }
    }
    return result;
}

QPointF ZoomTimeline::clampCenter(const QPointF &center, double level)
{
    const double half = 0.5 / std::max(1.0, level);
    return {std::clamp(center.x(), half, 1.0 - half),
            std::clamp(center.y(), half, 1.0 - half)};
}

static QPointF snapEdges(QPointF target, double level, double ratio)
{
    const double visible = 1.0 / std::max(1.0, level);
    const double r = std::min(std::max(0.0, ratio), visible);
    const double denominator = std::max(0.0001, 1.0 - 2.0 * r + 0.0001);
    target.setX(std::clamp((target.x() - r) / denominator, 0.0, 1.0));
    target.setY(std::clamp((target.y() - r) / denominator, 0.0, 1.0));
    return ZoomTimeline::clampCenter(target, level);
}

ZoomFrame ZoomTimeline::targetAt(const QVector<ZoomSegment> &segments,
                                 const QVector<CursorSample> &samples,
                                 double time, const ZoomStyle &style)
{
    const ZoomSegment *active = nullptr;
    for (const auto &segment : segments) {
        if (time >= segment.start && time <= segment.end
            && (!active || segment.start > active->start)) active = &segment;
    }
    if (!active) return {};
    if (!active->automaticTarget)
        return {active->level, clampCenter(active->target, active->level)};

    QVector<CursorSample> relevant;
    for (const auto &sample : samples)
        if (sample.time >= active->start && sample.time <= active->end) relevant << sample;
    if (relevant.isEmpty() && !samples.isEmpty()) {
        const auto it = std::lower_bound(samples.begin(), samples.end(), active->start,
            [](const CursorSample &sample, double t) { return sample.time < t; });
        if (it == samples.begin()) relevant << *it;
        else if (it == samples.end()) relevant << samples.last();
        else relevant << (it->time - active->start < active->start - (it - 1)->time ? *it : *(it - 1));
    }
    if (relevant.isEmpty()) return {active->level, {0.5, 0.5}};

    const double maxWidth = 0.5 / active->level;
    const double maxHeight = 0.7 / active->level;
    struct Group { double start; QPointF minimum; QPointF maximum; };
    QVector<Group> groups{{relevant.first().time, relevant.first().position, relevant.first().position}};
    for (qsizetype i = 1; i < relevant.size(); ++i) {
        auto &group = groups.last();
        const QPointF nextMin(std::min(group.minimum.x(), relevant[i].position.x()),
                              std::min(group.minimum.y(), relevant[i].position.y()));
        const QPointF nextMax(std::max(group.maximum.x(), relevant[i].position.x()),
                              std::max(group.maximum.y(), relevant[i].position.y()));
        if (nextMax.x() - nextMin.x() <= maxWidth && nextMax.y() - nextMin.y() <= maxHeight) {
            group.minimum = nextMin;
            group.maximum = nextMax;
        } else {
            groups << Group{relevant[i].time, relevant[i].position, relevant[i].position};
        }
    }
    const Group *activeGroup = nullptr;
    for (const auto &group : groups) {
        if (group.start <= time) activeGroup = &group;
        else break;
    }
    if (!activeGroup) return {active->level, {0.5, 0.5}};
    const QPointF activeCenter = (activeGroup->minimum + activeGroup->maximum) / 2.0;
    return {active->level, snapEdges(activeCenter, active->level, style.snapToEdgesRatio)};
}

QJsonValue ZoomTimeline::targetToJson(const ZoomSegment &segment)
{
    if (segment.automaticTarget) return QStringLiteral("auto");
    return QJsonObject{{"x", segment.target.x()}, {"y", segment.target.y()}};
}
