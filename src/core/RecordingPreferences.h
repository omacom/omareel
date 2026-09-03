#pragma once

#include <QJsonObject>
#include <QString>

namespace OmaRecord {

struct RecordingPreferences {
    bool systemAudio = true;
    bool microphone = true;
    QString microphoneDevice = QStringLiteral("default_input");
    bool webcamEnabled = false;
    QString webcamDevice = QStringLiteral("/dev/video2");
    QJsonObject webcam{
        {QStringLiteral("enabled"), false},
        {QStringLiteral("position"), QStringLiteral("bottom-right")},
        {QStringLiteral("size"), 0.25},
        {QStringLiteral("shape"), QStringLiteral("round")},
        {QStringLiteral("radius"), 16},
        {QStringLiteral("crop"), QStringLiteral("original")},
        {QStringLiteral("mirror"), true},
        {QStringLiteral("shadow"), true},
        {QStringLiteral("scaleDuringZoom"), 0.7},
        {QStringLiteral("offset"), QJsonObject{{QStringLiteral("x"), 0.02}, {QStringLiteral("y"), 0.02}}}
    };

    static QString path();
    static RecordingPreferences load();
    bool save(QString *error = nullptr) const;
};

} // namespace OmaRecord
