#pragma once

#include <QPointF>

namespace Omareel {

// Where the pointer is now, asked of Hyprland, for the recording bar's self-view drag. The
// recorder's cursor sampling is Rust (omareel_record::cursor_sampler); this calls it.
class CursorSampler
{
public:
    // Leaves point alone when Hyprland did not answer.
    static bool cursorPosition(QPointF *point);
};

} // namespace Omareel
