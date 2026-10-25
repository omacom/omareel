#include "CursorSampler.h"

#include "omareel_qt/src/record.cxxqt.h"

using namespace Omareel;

bool CursorSampler::cursorPosition(QPointF *point)
{
    double x = 0;
    double y = 0;
    if (!omareel::record::cursorPosition(x, y)) return false;
    *point = QPointF(x, y);
    return true;
}
