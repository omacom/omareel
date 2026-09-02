#pragma once

#include <algorithm>
#include <cmath>
#include <QString>

namespace OmaRecord {

inline double ease(double value, const QString &name = QStringLiteral("easeInOutCubic"))
{
    const double t = std::clamp(value, 0.0, 1.0);
    if (name == QLatin1String("linear"))
        return t;
    if (name == QLatin1String("easeOutExpo"))
        return t >= 1.0 ? 1.0 : 1.0 - std::pow(2.0, -10.0 * t);
    return t < 0.5 ? 4.0 * t * t * t
                   : 1.0 - std::pow(-2.0 * t + 2.0, 3.0) / 2.0;
}

inline double lerp(double a, double b, double t) { return a + (b - a) * t; }

} // namespace OmaRecord
