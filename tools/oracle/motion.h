#pragma once

// Inputs shared by the inputlog, cursorpath and zoomtimeline families.

#include "common.h"

#include "core/CursorPath.h"
#include "core/InputLog.h"

namespace Oracle {

struct InputLogCase {
    QString label;
    bool bundle = false;
    QString path;
    qint64 first = 0;
    double duration = -1.0;
};

QList<InputLogCase> inputLogCases(const Context &context);
Omareel::InputLog loadCase(const InputLogCase &c, QString *error);
QByteArray kindName(Omareel::InputKind kind);
QByteArray eventRow(const Omareel::InputEvent &event);

// The synthetic recording (tests/fixtures/synthetic, 640x360 at 60 fps) as MotionTrack sees
// it: InputLog::loadBundle with the capture's own duration.
Omareel::InputLog syntheticLog(const Context &context);
// Its moves as CursorSamples, normalised by the capture size like MotionTrack::build does
// (position / max(1, width) and / max(1, height)), or in pixels.
QVector<Omareel::CursorSample> syntheticSamples(const Context &context, bool normalized);
QVector<double> syntheticClickTimes(const Context &context);

} // namespace Oracle
