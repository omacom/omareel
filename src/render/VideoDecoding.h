#pragma once

namespace Omareel {

// Keeps Qt Multimedia's decoder off NVIDIA's VA-API driver. Call before the first media player
// exists; an explicit QT_FFMPEG_DECODING_HW_DEVICE_TYPES is left alone.
void configureVideoDecoding();

} // namespace Omareel
