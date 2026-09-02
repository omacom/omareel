#pragma once

#include <QString>

namespace OmaRecord {

enum class CaptureMode { Fullscreen, Region, Window };

struct CaptureRegion {
    QString monitorName;
    double x = 0.0;
    double y = 0.0;
    double width = 0.0;
    double height = 0.0;
    double scale = 1.0;
    int physicalWidth = 0;
    int physicalHeight = 0;
    CaptureMode mode = CaptureMode::Fullscreen;
};

class RegionPicker
{
public:
    static bool pick(CaptureMode mode, CaptureRegion *region, QString *error = nullptr);
};

} // namespace OmaRecord
