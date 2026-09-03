#include "RecordingPreferences.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QSaveFile>
#include <QStandardPaths>

using namespace OmaRecord;

QString RecordingPreferences::path()
{
    return QDir(QStandardPaths::writableLocation(QStandardPaths::ConfigLocation))
        .filePath(QStringLiteral("omarecord/settings.json"));
}

RecordingPreferences RecordingPreferences::load()
{
    RecordingPreferences preferences;
    QFile file(path());
    if (!file.open(QIODevice::ReadOnly)) return preferences;
    const QJsonObject root = QJsonDocument::fromJson(file.readAll()).object();
    preferences.systemAudio = root.value(QStringLiteral("systemAudio")).toBool(true);
    preferences.microphone = root.value(QStringLiteral("microphone")).toBool(true);
    preferences.microphoneDevice = root.value(QStringLiteral("microphoneDevice"))
                                       .toString(QStringLiteral("default_input"));
    if (root.value(QStringLiteral("webcam")).isObject()) {
        preferences.webcam = root.value(QStringLiteral("webcam")).toObject();
        preferences.webcamEnabled = preferences.webcam.value(QStringLiteral("enabled")).toBool(false);
        preferences.webcamDevice = preferences.webcam.value(QStringLiteral("device"))
                                       .toString(QStringLiteral("/dev/video2"));
        preferences.webcamHeight = preferences.webcam.value(QStringLiteral("captureHeight")).toInt(1080);
        if (preferences.webcamHeight != 720) preferences.webcamHeight = 1080;
    }
    return preferences;
}

bool RecordingPreferences::save(QString *error) const
{
    const QString filePath = path();
    if (!QDir().mkpath(QFileInfo(filePath).absolutePath())) {
        if (error) *error = QStringLiteral("Could not create the settings directory");
        return false;
    }
    QSaveFile file(filePath);
    if (!file.open(QIODevice::WriteOnly)) {
        if (error) *error = file.errorString();
        return false;
    }
    QJsonObject savedWebcam = webcam;
    savedWebcam.insert(QStringLiteral("enabled"), webcamEnabled);
    savedWebcam.insert(QStringLiteral("device"), webcamDevice);
    savedWebcam.insert(QStringLiteral("captureHeight"), webcamHeight == 720 ? 720 : 1080);
    const QJsonObject root{
        {QStringLiteral("systemAudio"), systemAudio},
        {QStringLiteral("microphone"), microphone},
        {QStringLiteral("microphoneDevice"), microphoneDevice},
        {QStringLiteral("webcam"), savedWebcam}
    };
    if (file.write(QJsonDocument(root).toJson(QJsonDocument::Indented)) < 0 || !file.commit()) {
        if (error) *error = file.errorString();
        return false;
    }
    if (error) error->clear();
    return true;
}
