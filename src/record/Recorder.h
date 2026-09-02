#pragma once

#include "RegionPicker.h"

#include <QString>

namespace OmaRecord {

struct RecordOptions {
    CaptureMode mode = CaptureMode::Region;
    int fps = 60;
    QString outputDirectory;
    bool desktopAudio = false;
    bool microphoneAudio = false;
    QString microphoneDevice = QStringLiteral("default_input");
    bool noOpen = false;
    bool noBar = false;
};

class Recorder
{
public:
    enum class GsrExitClassification { UserStop, ExternalStop, Failure };

    static QString stateFilePath();
    static bool isRecording();
    static qint64 recordingStartedUs();
    static QString recordedMonitor();
    static bool signalExisting(bool cancel, QString *error = nullptr);
    static GsrExitClassification classifyGsrExit(int exitCode, bool stopRequested,
                                                 qint64 fileSize, double probedDuration);
    static int startDetached(const RecordOptions &options, QString *message);
    static int stopExisting(bool cancel, QString *bundlePath, QString *error);
    static int daemonMain(const QStringList &arguments);
};

} // namespace OmaRecord
