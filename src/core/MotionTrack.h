#pragma once

#include "InputLog.h"
#include "Project.h"

namespace Omareel {

struct MotionSample {
    double zoomScale = 1.0;
    double zoomCx = 0.5;
    double zoomCy = 0.5;
    double zoomCenterVelocity = 0.0;
    double cursorX = 0.5;
    double cursorY = 0.5;
    double cursorScale = 1.0;
    double cursorOpacity = 1.0;
    double cursorRotation = 0.0;
};
struct RippleSample { double x = 0.5; double y = 0.5; double progress = 0.0; };

class MotionTrack
{
public:
    static constexpr double SampleRate = 240.0;
    static MotionTrack build(double duration, int sourceWidth, int sourceHeight,
                             const QVector<InputEvent> &events, const Project &project,
                             double captureFps = 60.0);
    MotionSample sample(double time) const;
    QVector<RippleSample> ripples(double time) const;
    double duration() const { return m_duration; }
    qsizetype size() const { return m_samples.size(); }

private:
    struct Click { double down = 0.0; double up = 0.0; QPointF position; };
    double m_duration = 0.0;
    QVector<MotionSample> m_samples;
    QVector<Click> m_clicks;
};

} // namespace Omareel
