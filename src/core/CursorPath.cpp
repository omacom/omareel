#include "CursorPath.h"
#include "Easing.h"

#include <algorithm>
#include <cmath>

using namespace OmaRecord;

QVector<CursorSample> CursorPath::resample(const QVector<CursorSample> &raw,
                                           const QVector<double> &frameTimes)
{
    QVector<CursorSample> output;
    if (raw.isEmpty()) return output;
    output.reserve(frameTimes.size());
    qsizetype index = 0;
    for (double time : frameTimes) {
        while (index + 1 < raw.size() && raw[index + 1].time < time) ++index;
        QPointF position = raw[index].position;
        if (index + 1 < raw.size() && raw[index + 1].time > raw[index].time) {
            const double alpha = std::clamp((time - raw[index].time)
                                            / (raw[index + 1].time - raw[index].time), 0.0, 1.0);
            position = raw[index].position + (raw[index + 1].position - raw[index].position) * alpha;
        }
        output << CursorSample{time, position};
    }
    return output;
}

QVector<CursorSample> CursorPath::smooth(const QVector<CursorSample> &samples, double smoothing)
{
    if (samples.size() < 2 || smoothing <= 0.0) return samples;
    QVector<CursorSample> output;
    output.reserve(samples.size());
    QPointF position = samples.first().position;
    QPointF velocity;
    output << CursorSample{samples.first().time, position};

    const double settle = 0.2 * std::pow(std::clamp(smoothing, 0.0, 1.0), 3.58);
    const double omega = 5.84 / std::max(0.001, settle);
    for (qsizetype i = 1; i < samples.size(); ++i) {
        const double dt = std::max(0.0, samples[i].time - samples[i - 1].time);
        const QPointF target = samples[i].position;
        const QPointF displacement = position - target;
        const QPointF coefficient = velocity + displacement * omega;
        const double decay = std::exp(-omega * dt);
        position = target + (displacement + coefficient * dt) * decay;
        velocity = (velocity - coefficient * (omega * dt)) * decay;
        output << CursorSample{samples[i].time, position};
    }
    return output;
}

ClickAnimation CursorPath::clickAnimation(double time, const QVector<double> &downTimes,
                                          double clickShrink, double outputScale1080)
{
    ClickAnimation result;
    for (double down : downTimes) {
        const double age = time - down;
        if (age < 0.0 || age > 0.45) continue;
        if (age <= 0.08)
            result.scale = std::min(result.scale, lerp(1.0, clickShrink, ease(age / 0.08)));
        else if (age <= 0.24)
            result.scale = std::min(result.scale,
                                    lerp(clickShrink, 1.0, ease((age - 0.08) / 0.16)));
        result.rippleVisible = true;
        const double progress = age / 0.45;
        result.rippleRadius = std::max(result.rippleRadius, 60.0 * outputScale1080 * progress);
        result.rippleOpacity = std::max(result.rippleOpacity, 1.0 - progress);
    }
    return result;
}

double CursorPath::idleOpacity(double time, double lastMovementTime, int hideWhenIdleMs)
{
    if (hideWhenIdleMs < 0) return 1.0;
    const double idle = time - lastMovementTime - double(hideWhenIdleMs) / 1000.0;
    return 1.0 - std::clamp(idle / 0.3, 0.0, 1.0);
}
