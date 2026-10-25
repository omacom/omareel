#pragma once

#include <QJsonObject>
#include <QString>

namespace Omareel {

// The recorder's state files (recording.json, selfview-host.json) for the C++ that still reads
// and writes them: the launcher, the recording bar and its windows. The recorder itself, its
// daemon and `omareel record` are Rust (omareel_record); these call its state layer
// (omareel_record::recorder::state) through omareel_qt.
class Recorder
{
public:
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
};

} // namespace Omareel
