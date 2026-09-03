#pragma once

namespace OmaRecord {

struct CameraTime {
    double seconds = 0.0;
    bool beforeStart = false;
    bool beyondEnd = false;
};

CameraTime mapCameraTime(double screenTime, double cameraOffset, double cameraDuration);

} // namespace OmaRecord
