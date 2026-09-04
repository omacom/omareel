#pragma once

#include <QString>

namespace Omareel::RecordingMetadata {

// Returns zero when capture.json is missing or does not contain a valid interval.
double captureDuration(const QString &bundlePath);

} // namespace Omareel::RecordingMetadata
