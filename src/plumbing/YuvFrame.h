#pragma once

#include <QMetaType>
#include <QtGlobal>
#include <memory>

namespace Omareel {

// A YUV 4:2:0 frame in the video's own matrix and range: the Y plane (width x height), then U
// and V (half each way), tightly packed in one buffer that `owner` keeps alive. The export hands
// these to FrameSource::setYuvFrame, which converts them on the GPU.
struct YuvFrame
{
    std::shared_ptr<const void> owner;
    const uchar *planes = nullptr;
    int width = 0;
    int height = 0;
    bool bt709 = false;
    bool fullRange = false;
    bool isNull() const { return !planes || width <= 0 || height <= 0; }
};

} // namespace Omareel

Q_DECLARE_METATYPE(Omareel::YuvFrame)
