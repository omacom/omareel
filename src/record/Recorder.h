#pragma once

#include "RegionPicker.h"

#include <QJsonObject>
#include <QString>
#include <QStringList>
#include <functional>

class QProcess;

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
    bool countdown = false;
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
    static QString selfViewHostPath();
    static QJsonObject readStateFile(const QString &path);
    static bool updateStateFile(const QString &path, const QJsonObject &values, QString *error = nullptr);
    static bool sendHostCommand(const QString &command);
    static bool isRecording();
    static qint64 recordingStartedUs();
    static QString recordedMonitor();
    static bool recordingHasWebcam();
    static QJsonObject recordingState();
    static bool updateRecordingState(const QJsonObject &values, QString *error = nullptr);
    static bool signalExisting(bool cancel, QString *error = nullptr);
    static GsrExitClassification classifyGsrExit(int exitCode, bool stopRequested,
                                                 qint64 fileSize, double probedDuration);
    static CaptureRegion evenCaptureRegion(CaptureRegion region);
    static QJsonObject cameraCaptureBlock(const QString &device, int requestedHeight,
                                          int width, int height, double fps,
                                          qint64 firstFrameUs, const QString &backend,
                                          int rotation, bool flipHorizontal,
                                          const QString &shape = QStringLiteral("round"),
                                          double radius = 16);
    static bool discardTimeline(const DiscardActions &actions);
    static bool finishAudioCapture(QProcess *process, const QString &screenPath,
                                   const QString &audioPath, const QString &outputPath,
                                   qint64 screenStartedUs, qint64 audioStartedUs,
                                   qint64 screenDurationUs);
    // Whether the chosen microphone is missing from the audio server's sources, so the default
    // one has to stand in. An empty list (pactl failed) says nothing either way.
    static bool microphoneMissing(const QString &device, const QStringList &sources);
    // The loudest microphone sample ffmpeg's volumedetect logged as it exited, in dBFS:
    // -infinity when no sound arrived at all, NaN when the log has no report.
    static double microphonePeakDb(const QString &ffmpegLog);
    static int startDetached(const RecordOptions &options, QString *message);
    static int stopExisting(bool cancel, QString *bundlePath, QString *error);
    static int daemonMain(const QStringList &arguments);
};

} // namespace Omareel
