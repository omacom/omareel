#include "MotionTrack.h"

#include "CursorPath.h"
#include "ZoomTimeline.h"

#include <algorithm>
#include <cmath>

using namespace OmaRecord;

struct SpringState { double x = 0.0; double velocity = 0.0; };

static void integrate(SpringState &state, double target, const Spring &spring, double dt)
{
    if (spring.mass <= 0.0 || spring.stiffness <= 0.0) {
        state.x = target;
        state.velocity = 0.0;
        return;
    }
    const double acceleration = (spring.stiffness * (target - state.x)
                                 - spring.damping * state.velocity) / spring.mass;
    state.velocity += acceleration * dt;
    state.x += state.velocity * dt;
}

MotionTrack MotionTrack::build(double duration, int sourceWidth, int sourceHeight,
                               const QVector<InputEvent> &events, const Project &project,
                               double captureFps)
{
    MotionTrack track;
    track.m_duration = std::max(0.0, duration);
    QVector<CursorSample> raw;
    for (const auto &event : events) {
        const QPointF normalized(event.position.x() / std::max(1, sourceWidth),
                                 event.position.y() / std::max(1, sourceHeight));
        if (event.kind == InputKind::Move) {
            raw << CursorSample{event.time, normalized};
        } else if (event.kind == InputKind::ButtonDown) {
            track.m_clicks << Click{event.time, std::min(duration, event.time + 0.13), normalized};
        } else if (event.kind == InputKind::ButtonUp) {
            for (auto it = track.m_clicks.rbegin(); it != track.m_clicks.rend(); ++it) {
                if (it->up <= it->down + 0.130001) {
                    it->up = std::max(it->down, event.time);
                    break;
                }
            }
        }
    }
    raw = CursorPath::decimate(raw, captureFps);
    if (raw.isEmpty()) raw << CursorSample{0.0, {0.5, 0.5}};

    const int count = std::max(1, int(std::ceil(track.m_duration * SampleRate)) + 1);
    track.m_samples.reserve(count);
    const QPointF initialCursor = CursorPath::positionAt(raw, 0.0);
    SpringState zoomScale{1.0, 0.0}, zoomX{0.5, 0.0}, zoomY{0.5, 0.0};
    SpringState cursorX{initialCursor.x(), 0.0}, cursorY{initialCursor.y(), 0.0};
    SpringState cursorScale{1.0, 0.0}, cursorOpacity{project.cursor.visible ? 1.0 : 0.0, 0.0};
    SpringState cursorRotation{0.0, 0.0};
    const Spring clickSpring{0.3, 300.0, 30.0};

    for (int i = 0; i < count; ++i) {
        const double time = std::min(track.m_duration, i / SampleRate);
        const QPointF cursorTarget = CursorPath::positionAt(raw, time);
        const ZoomFrame zoomTarget = ZoomTimeline::targetAt(project.zooms, raw, time, project.zoomStyle);
        bool pressed = false;
        for (const auto &click : track.m_clicks)
            if (time >= click.down - 0.13 && time <= click.up) { pressed = true; break; }

        bool hidden = !project.cursor.visible;
        if (!hidden && project.cursor.hideWhenIdleMs >= 0) {
            double previous = raw.first().time;
            double next = track.m_duration + 1.0;
            for (const auto &move : raw) {
                if (move.time <= time) previous = move.time;
                else { next = move.time; break; }
            }
            hidden = time - previous > project.cursor.hideWhenIdleMs / 1000.0 && time < next - 0.25;
        }
        const double scaleTarget = pressed ? project.cursor.clickShrink : (hidden ? 0.8 : 1.0);
        const double rotationTarget = std::clamp(
            (cursorTarget.x() - CursorPath::positionAt(raw, time - 0.4).x())
                * sourceWidth * 0.03 * project.cursor.rotateOnXMovementRatio,
            -20.0, 20.0);
        bool nearBoundary = false;
        if (project.zoomStyle.instantAnimation) {
            for (const auto &z : project.zooms) {
                if (std::abs(time - z.start) <= 0.1 || std::abs(time - z.end) <= 0.1) {
                    nearBoundary = true;
                    break;
                }
            }
        }
        if (nearBoundary) {
            zoomScale = {zoomTarget.scale, 0.0};
            zoomX = {zoomTarget.center.x(), 0.0};
            zoomY = {zoomTarget.center.y(), 0.0};
        } else if (i > 0) {
            integrate(zoomScale, zoomTarget.scale, project.zoomStyle.spring, 1.0 / SampleRate);
            integrate(zoomX, zoomTarget.center.x(), project.zoomStyle.spring, 1.0 / SampleRate);
            integrate(zoomY, zoomTarget.center.y(), project.zoomStyle.spring, 1.0 / SampleRate);
        }
        if (i > 0) {
            if (project.cursor.smoothing) {
                integrate(cursorX, cursorTarget.x(), project.cursor.spring, 1.0 / SampleRate);
                integrate(cursorY, cursorTarget.y(), project.cursor.spring, 1.0 / SampleRate);
            } else {
                cursorX = {cursorTarget.x(), 0.0};
                cursorY = {cursorTarget.y(), 0.0};
            }
            integrate(cursorScale, scaleTarget, clickSpring, 1.0 / SampleRate);
            integrate(cursorOpacity, hidden ? 0.0 : 1.0, clickSpring, 1.0 / SampleRate);
            integrate(cursorRotation, rotationTarget, project.cursor.spring, 1.0 / SampleRate);
        }
        track.m_samples << MotionSample{zoomScale.x, zoomX.x, zoomY.x,
                                        cursorX.x, cursorY.x, cursorScale.x,
                                        cursorOpacity.x, cursorRotation.x};
    }
    // Ripples ride on the cursor the viewer actually sees (the smoothed one),
    // not on the raw click position the spring may still be catching up to.
    for (auto &click : track.m_clicks) {
        const MotionSample at = track.sample(click.down);
        click.position = QPointF(at.cursorX, at.cursorY);
    }
    return track;
}

MotionSample MotionTrack::sample(double time) const
{
    if (m_samples.isEmpty()) return {};
    const double index = std::clamp(time, 0.0, m_duration) * SampleRate;
    const int a = std::min(int(index), int(m_samples.size() - 1));
    const int b = std::min(a + 1, int(m_samples.size() - 1));
    const double t = index - a;
    const auto mix = [t](double x, double y) { return x + (y - x) * t; };
    return {mix(m_samples[a].zoomScale, m_samples[b].zoomScale),
            mix(m_samples[a].zoomCx, m_samples[b].zoomCx),
            mix(m_samples[a].zoomCy, m_samples[b].zoomCy),
            mix(m_samples[a].cursorX, m_samples[b].cursorX),
            mix(m_samples[a].cursorY, m_samples[b].cursorY),
            mix(m_samples[a].cursorScale, m_samples[b].cursorScale),
            mix(m_samples[a].cursorOpacity, m_samples[b].cursorOpacity),
            mix(m_samples[a].cursorRotation, m_samples[b].cursorRotation)};
}

QVector<RippleSample> MotionTrack::ripples(double time) const
{
    QVector<RippleSample> result;
    for (const auto &click : m_clicks) {
        const double age = time - click.down;
        if (age >= 0.0 && age <= 0.45)
            result << RippleSample{click.position.x(), click.position.y(), age / 0.45};
    }
    return result;
}
