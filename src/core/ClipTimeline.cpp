#include "ClipTimeline.h"

#include <algorithm>
#include <cmath>
#include <utility>

using namespace Omareel;

static double duration(const Clip &clip)
{
    return (clip.out - clip.in) / std::max(0.000001, clip.speed);
}

double ClipTimeline::totalDuration() const
{
    double total = 0.0;
    for (const auto &clip : m_clips) total += std::max(0.0, duration(clip));
    return total;
}

double ClipTimeline::sourceTime(double output) const
{
    if (m_clips.isEmpty()) return -1.0;
    double cursor = 0.0;
    for (int i = 0; i < m_clips.size(); ++i) {
        const Clip &clip = m_clips[i];
        const double length = duration(clip);
        if (output < cursor + length || i == m_clips.size() - 1)
            return std::clamp(clip.in + (output - cursor) * clip.speed, clip.in, clip.out);
        cursor += length;
    }
    return m_clips.last().out;
}

double ClipTimeline::outputTime(int clipIndex, double source) const
{
    if (clipIndex < 0 || clipIndex >= m_clips.size()) return -1.0;
    double output = 0.0;
    for (int i = 0; i < clipIndex; ++i) output += duration(m_clips[i]);
    const auto &clip = m_clips[clipIndex];
    return output + (std::clamp(source, clip.in, clip.out) - clip.in) / clip.speed;
}

bool ClipTimeline::split(double output)
{
    double cursor = 0.0;
    for (int i = 0; i < m_clips.size(); ++i) {
        const double length = duration(m_clips[i]);
        if (output > cursor + 1e-9 && output < cursor + length - 1e-9) {
            Clip left = m_clips[i];
            Clip right = m_clips[i];
            const double source = left.in + (output - cursor) * left.speed;
            left.out = source;
            right.in = source;
            right.id = QStringLiteral("%1-s%2").arg(right.id).arg(m_nextId++);
            m_clips[i] = left;
            m_clips.insert(i + 1, right);
            return true;
        }
        cursor += length;
    }
    return false;
}

bool ClipTimeline::trim(int index, double newIn, double newOut)
{
    if (index < 0 || index >= m_clips.size() || newIn < 0.0 || newOut <= newIn) return false;
    m_clips[index].in = newIn;
    m_clips[index].out = newOut;
    return true;
}

bool ClipTimeline::remove(int index)
{
    if (index < 0 || index >= m_clips.size()) return false;
    m_clips.removeAt(index);
    return true;
}

bool ClipTimeline::deleteRange(double from, double to, const QString &remainderId)
{
    if (!std::isfinite(from) || !std::isfinite(to)) return false;
    const double total = totalDuration();
    from = std::clamp(from, 0.0, total);
    to = std::clamp(to, 0.0, total);
    if (to - from < 1e-9) return false;

    QVector<Clip> kept;
    double cursor = 0.0;
    for (const Clip &clip : std::as_const(m_clips)) {
        const double length = duration(clip);
        const double end = cursor + length;
        if (end <= from || cursor >= to) {
            kept << clip;
        } else {
            const double leftLength = std::clamp(from - cursor, 0.0, length);
            const double rightLength = std::clamp(end - to, 0.0, length);
            if (leftLength > 1e-9) {
                Clip left = clip;
                left.out = clip.in + leftLength * clip.speed;
                if (left.out - left.in < 0.1) return false;
                kept << left;
            }
            if (rightLength > 1e-9) {
                Clip right = clip;
                right.in = clip.out - rightLength * clip.speed;
                if (right.out - right.in < 0.1) return false;
                if (leftLength > 1e-9) {
                    if (remainderId.isEmpty()) return false;
                    for (const Clip &existing : std::as_const(m_clips))
                        if (existing.id == remainderId) return false;
                    right.id = remainderId;
                }
                kept << right;
            }
        }
        cursor = end;
    }
    if (kept.isEmpty()) return false;
    m_clips = std::move(kept);
    return true;
}
