#include "VideoDecoding.h"

#include <QByteArray>
#include <QFileInfo>
#include <QString>

// Mirrors libva's choice: an explicit driver name, otherwise the driver of the first render node.
static bool vaapiDriverIsNvidia()
{
    const QByteArray driver = qgetenv("LIBVA_DRIVER_NAME");
    if (!driver.isEmpty()) return driver == "nvidia";
    return QFileInfo(QStringLiteral("/sys/class/drm/renderD128/device/driver"))
        .symLinkTarget().endsWith(QLatin1String("/nvidia"));
}

void Omareel::configureVideoDecoding()
{
    // Qt decodes on CUDA first and VA-API second, and reads preview frames back to the CPU.
    // Reading back through nvidia-vaapi-driver segfaults in vaGetImage (direct backend) or hangs
    // (EGL backend) on the N1X, where Arch Linux ARM's FFmpeg has no CUDA to try first. Never hand
    // NVIDIA's VA-API driver to Qt: x86 keeps CUDA, FFmpeg builds without CUDA decode in software.
    if (!qEnvironmentVariableIsSet("QT_FFMPEG_DECODING_HW_DEVICE_TYPES") && vaapiDriverIsNvidia())
        qputenv("QT_FFMPEG_DECODING_HW_DEVICE_TYPES", "cuda");
}
