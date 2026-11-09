#pragma once

namespace Omareel {

// Keeps Qt Multimedia's decoder, and its probe of encoding devices, off NVIDIA's VA-API driver.
// Call before the first media player exists; explicit QT_FFMPEG_DECODING_HW_DEVICE_TYPES and
// QT_FFMPEG_ENCODING_HW_DEVICE_TYPES are left alone.
void configureVideoDecoding();

} // namespace Omareel
