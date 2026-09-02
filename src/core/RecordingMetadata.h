#pragma once

#include <QString>

namespace OmaRecord::RecordingMetadata {

// Returns zero when capture.json is missing or does not contain a valid interval.
double captureDuration(const QString &bundlePath);

} // namespace OmaRecord::RecordingMetadata
