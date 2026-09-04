#pragma once

#include "RegionPicker.h"

#include <QJsonObject>
#include <QString>
#include <functional>

namespace Omareel {

struct RecordOptions {
    CaptureMode mode = CaptureMode::Region;
    int fps = 60;
    QString outputDirectory;
    bool desktopAudio = false;
    bool microphoneAudio = false;
    QString microphoneDevice = QStringLiteral("default_input");
    bool webcam = false;
    QString webcamDevice = QStringLiteral("/dev/video2");
    int webcamHeight = 1080;
    int webcamRotation = 0;
    bool webcamFlipHorizontal = false;
    bool selfView = true;
    QString selfViewSize = QStringLiteral("M");
    QString captureBackend = QStringLiteral("auto");
    bool noOpen = false;
    bool noBar = false;
};

class Recorder
{
public:
    struct DiscardActions {
        std::function<void()> abortCapture;
        std::function<void()> killAudio;
        std::function<void()> requestCameraStop;
        std::function<void(int)> waitForCameraStop;
        std::function<bool()> removeBundle;
        std::function<void()> removeState;
    };
    enum class GsrExitClassification { UserStop, ExternalStop, Failure };

    static QString stateFilePath();
    static bool isRecording();
    static qint64 recordingStartedUs();
    static QString recordedMonitor();
    static bool recordingHasWebcam();
    static QJsonObject recordingState();
    static bool updateRecordingState(const QJsonObject &values, QString *error = nullptr);
    static bool signalExisting(bool cancel, QString *error = nullptr);
    static GsrExitClassification classifyGsrExit(int exitCode, bool stopRequested,
                                                 qint64 fileSize, double probedDuration);
    static QJsonObject cameraCaptureBlock(const QString &device, int requestedHeight,
                                          int width, int height, double fps,
                                          qint64 firstFrameUs, const QString &backend,
                                          int rotation, bool flipHorizontal);
    static bool discardTimeline(const DiscardActions &actions);
    static int startDetached(const RecordOptions &options, QString *message);
    static int stopExisting(bool cancel, QString *bundlePath, QString *error);
    static int daemonMain(const QStringList &arguments);
};

} // namespace Omareel
