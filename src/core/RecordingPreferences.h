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
    int webcamHeight = 1080;
    int webcamRotation = 0;
    bool webcamFlipHorizontal = false;
    bool selfViewEnabled = true;
    QString selfViewSize = QStringLiteral("M");
    double selfViewX = 1.0;
    double selfViewY = 1.0;
    bool hideSelfViewViaPortal = false;
    bool selfViewCaptureWarningShown = false;
    QJsonObject webcam{
        {QStringLiteral("enabled"), false},
        {QStringLiteral("position"), QStringLiteral("bottom-right")},
        {QStringLiteral("size"), 0.25},
        {QStringLiteral("shape"), QStringLiteral("round")},
        {QStringLiteral("radius"), 16},
        {QStringLiteral("crop"), QStringLiteral("original")},
        {QStringLiteral("flipHorizontal"), false},
        {QStringLiteral("rotation"), 0},
        {QStringLiteral("shadow"), QJsonObject{{QStringLiteral("enabled"), false},
            {QStringLiteral("intensity"), 0.55}, {QStringLiteral("blur"), 18},
            {QStringLiteral("distance"), 18}}},
        {QStringLiteral("inset"), QJsonObject{{QStringLiteral("enabled"), false},
            {QStringLiteral("width"), 2}, {QStringLiteral("color"), QStringLiteral("#ffffff")},
            {QStringLiteral("alpha"), 0.7}}},
        {QStringLiteral("scaleDuringZoom"), 0.7},
        {QStringLiteral("offset"), QJsonObject{{QStringLiteral("x"), 0.02}, {QStringLiteral("y"), 0.02}}}
    };

    static QString path();
    static RecordingPreferences load();
    bool save(QString *error = nullptr) const;
};

} // namespace OmaRecord
