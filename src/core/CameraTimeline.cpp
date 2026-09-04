#include "CameraTimeline.h"

#include <algorithm>

using namespace Omareel;

CameraTime Omareel::mapCameraTime(double screenTime, double cameraOffset,
                                    double cameraDuration)
{
    const double raw = screenTime - cameraOffset;
    return {std::clamp(raw, 0.0, std::max(0.0, cameraDuration)),
            raw < 0.0, raw > cameraDuration};
}
