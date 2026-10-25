#include "Recorder.h"

#include "omareel_qt/src/record.cxxqt.h"

#include <QJsonDocument>

using namespace Omareel;

QString Recorder::stateFilePath()
{
    return omareel::record::stateFilePath();
}

QString Recorder::selfViewHostPath()
{
    return omareel::record::selfViewHostPath();
}

QJsonObject Recorder::readStateFile(const QString &path)
{
    return QJsonDocument::fromJson(omareel::record::readStateFile(path)).object();
}

bool Recorder::updateStateFile(const QString &path, const QJsonObject &values, QString *error)
{
    QString message;
    const bool updated = omareel::record::updateStateFile(
        path, QJsonDocument(values).toJson(QJsonDocument::Compact), message);
    if (error) *error = message;
    return updated;
}

bool Recorder::sendHostCommand(const QString &command)
{
    return omareel::record::sendHostCommand(command);
}

bool Recorder::isRecording()
{
    return omareel::record::isRecording();
}

qint64 Recorder::recordingStartedUs()
{
    return omareel::record::recordingStartedUs();
}

QString Recorder::recordedMonitor()
{
    return omareel::record::recordedMonitor();
}

bool Recorder::recordingHasWebcam()
{
    return omareel::record::recordingHasWebcam();
}

QJsonObject Recorder::recordingState()
{
    return readStateFile(stateFilePath());
}

bool Recorder::updateRecordingState(const QJsonObject &values, QString *error)
{
    return updateStateFile(stateFilePath(), values, error);
}

bool Recorder::signalExisting(bool cancel, QString *error)
{
    QString message;
    if (omareel::record::signalExisting(cancel, message)) return true;
    if (error) *error = message;
    return false;
}
