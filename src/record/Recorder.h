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
    bool noOpen = false;
};

class Recorder
{
public:
    static QString stateFilePath();
    static bool isRecording();
    static int startDetached(const RecordOptions &options, QString *message);
    static int stopExisting(bool onlyStop, QString *bundlePath, QString *error);
    static int daemonMain(const QStringList &arguments);
};

} // namespace OmaRecord
