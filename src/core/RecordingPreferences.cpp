#include "RecordingPreferences.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QSaveFile>
#include <QStandardPaths>

using namespace Omareel;

QString RecordingPreferences::path()
{
    return QDir(QStandardPaths::writableLocation(QStandardPaths::ConfigLocation))
        .filePath(QStringLiteral("omareel/settings.json"));
}

RecordingPreferences RecordingPreferences::load()
{
    RecordingPreferences preferences;
    const QString currentPath = path();
    const QString legacyPath = QDir(QStandardPaths::writableLocation(QStandardPaths::ConfigLocation))
                                   .filePath(QStringLiteral("omarecord/settings.json"));
    QString readPath = currentPath;
    if (!QFileInfo::exists(currentPath) && QFileInfo::exists(legacyPath)) {
        QDir().mkpath(QFileInfo(currentPath).absolutePath());
        if (!QFile::copy(legacyPath, currentPath)) readPath = legacyPath;
    }
    QFile file(readPath);
    if (!file.open(QIODevice::ReadOnly)) return preferences;
    const QJsonObject root = QJsonDocument::fromJson(file.readAll()).object();
    preferences.systemAudio = root.value(QStringLiteral("systemAudio")).toBool(true);
    preferences.microphone = root.value(QStringLiteral("microphone")).toBool(true);
    preferences.microphoneDevice = root.value(QStringLiteral("microphoneDevice"))
                                       .toString(QStringLiteral("default_input"));
    preferences.selfViewEnabled = root.value(QStringLiteral("selfViewEnabled")).toBool(true);
    const QString selfViewSize = root.value(QStringLiteral("selfViewSize")).toString(QStringLiteral("M")).toUpper();
    preferences.selfViewSize = selfViewSize == QLatin1String("S") || selfViewSize == QLatin1String("L")
        ? selfViewSize : QStringLiteral("M");
    const QJsonObject selfViewPosition = root.value(QStringLiteral("selfViewPosition")).toObject();
    preferences.selfViewX = qBound(0.0, selfViewPosition.value(QStringLiteral("x")).toDouble(1.0), 1.0);
    preferences.selfViewY = qBound(0.0, selfViewPosition.value(QStringLiteral("y")).toDouble(1.0), 1.0);
    preferences.captureBackend = root.value(QStringLiteral("captureBackend")).toString(QStringLiteral("auto"));
    if (root.value(QStringLiteral("webcam")).isObject()) {
        preferences.webcam = root.value(QStringLiteral("webcam")).toObject();
        preferences.webcamEnabled = preferences.webcam.value(QStringLiteral("enabled")).toBool(false);
        preferences.webcamDevice = preferences.webcam.value(QStringLiteral("device"))
                                       .toString(QStringLiteral("/dev/video2"));
        preferences.webcamHeight = preferences.webcam.value(QStringLiteral("captureHeight")).toInt(1080);
        if (preferences.webcamHeight != 720) preferences.webcamHeight = 1080;
        const int rotation = preferences.webcam.value(QStringLiteral("rotation")).toInt(0);
        preferences.webcamRotation = rotation == 90 || rotation == 180 || rotation == 270 ? rotation : 0;
        preferences.webcamFlipHorizontal = preferences.webcam.contains(QStringLiteral("flipHorizontal"))
            ? preferences.webcam.value(QStringLiteral("flipHorizontal")).toBool(false)
            : preferences.webcam.value(QStringLiteral("mirror")).toBool(false);
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
    savedWebcam.insert(QStringLiteral("rotation"), webcamRotation);
    savedWebcam.insert(QStringLiteral("flipHorizontal"), webcamFlipHorizontal);
    const QJsonObject root{
        {QStringLiteral("systemAudio"), systemAudio},
        {QStringLiteral("microphone"), microphone},
        {QStringLiteral("microphoneDevice"), microphoneDevice},
        {QStringLiteral("selfViewEnabled"), selfViewEnabled},
        {QStringLiteral("selfViewSize"), selfViewSize},
        {QStringLiteral("selfViewPosition"), QJsonObject{
            {QStringLiteral("x"), qBound(0.0, selfViewX, 1.0)},
            {QStringLiteral("y"), qBound(0.0, selfViewY, 1.0)}}},
        {QStringLiteral("captureBackend"), captureBackend},
        {QStringLiteral("webcam"), savedWebcam}
    };
    if (file.write(QJsonDocument(root).toJson(QJsonDocument::Indented)) < 0 || !file.commit()) {
        if (error) *error = file.errorString();
        return false;
    }
    if (error) error->clear();
    return true;
}
