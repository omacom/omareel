#pragma once

// Inputs and cell formats shared by the cliptimeline, layouttimeline, keystroketrack, motiontrack
// and composition families.

#include "common.h"

#include "core/Project.h"

#include <QJsonArray>
#include <QJsonObject>

#include <cmath>
#include <limits>

namespace Oracle {

// A Clip from a corpus object {id, in, out, speed[, layout][, media][, fit]}: missing keys keep
// Clip's defaults (speed 1, layout "default", no media, fit "fit"). No validation, unlike
// Project::fromJson.
Omareel::Clip clipFromJson(const QJsonObject &object);
QVector<Omareel::Clip> clipsFromJson(const QJsonArray &array);
// README "clips": [id in out speed layout media fit;...].
QByteArray clipsText(const QVector<Omareel::Clip> &clips);

inline double before(double value) { return std::nextafter(value, -std::numeric_limits<double>::infinity()); }
inline double after(double value) { return std::nextafter(value, std::numeric_limits<double>::infinity()); }
// Sorted ascending without exact duplicates (NaN and infinities are not expected in the input).
QVector<double> sortedUnique(QVector<double> values);

} // namespace Oracle
