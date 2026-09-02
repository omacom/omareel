#include "Project.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QSaveFile>

using namespace OmaRecord;

Project Project::defaults(const QString &name, double duration)
{
    QJsonObject root;
    root["version"] = 1;
    root["name"] = name;
    root["aspect"] = "auto";
    root["crop"] = QJsonObject{{"x", 0}, {"y", 0}, {"w", 1}, {"h", 1}};
    root["clips"] = QJsonArray{QJsonObject{{"id", "c1"}, {"in", 0.0},
                                            {"out", duration}, {"speed", 1.0}}};
    root["zooms"] = QJsonArray{};
    root["zoomStyle"] = QJsonObject{{"transitionIn", 0.7}, {"transitionOut", 0.7},
                                      {"easing", "easeInOutCubic"},
                                      {"followSmoothing", 0.85},
                                      {"followDeadZone", 0.12}, {"lookahead", 0.0}};
    root["background"] = QJsonObject{
        {"type", "wallpaper"}, {"wallpaper", "omarchy:current"},
        {"gradient", QJsonObject{{"angle", 135},
          {"stops", QJsonArray{QJsonArray{"#ff8a00", 0}, QJsonArray{"#e52e71", 1}}}}},
        {"color", "#1a1b26"}, {"image", QJsonValue::Null}, {"blur", 0}};
    root["frame"] = QJsonObject{
        {"padding", 0.08}, {"radius", 12},
        {"shadow", QJsonObject{{"enabled", true}, {"opacity", 0.5},
                                {"blur", 40}, {"offsetY", 20}}},
        {"inset", QJsonObject{{"enabled", false}, {"width", 0},
                               {"color", "#000000"}, {"alpha", 0.5}}}};
    root["cursor"] = QJsonObject{{"visible", true}, {"size", 1.5}, {"smoothing", 0.8},
                                   {"clickEffect", "ripple"}, {"clickShrink", 0.85},
                                   {"hideWhenIdleMs", QJsonValue::Null}, {"style", "macos"}};
    root["audio"] = QJsonObject{{"desktop", true}, {"mic", true}, {"volume", 1.0}};
    root["export"] = QJsonObject{{"format", "mp4"}, {"fps", 60}, {"width", 1920},
                                   {"quality", "high"},
                                   {"gif", QJsonObject{{"fps", 20}, {"width", 960}}}};
    return Project(root);
}

Project Project::load(const QString &path, QString *error)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        if (error) *error = file.errorString();
        return Project();
    }
    QJsonParseError parseError;
    const auto doc = QJsonDocument::fromJson(file.readAll(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
        if (error) *error = parseError.errorString();
        return Project();
    }
    if (error) error->clear();
    return Project(doc.object());
}

bool Project::save(const QString &path, QString *error) const
{
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        if (error) *error = file.errorString();
        return false;
    }
    if (file.write(QJsonDocument(m_json).toJson(QJsonDocument::Indented)) < 0 || !file.commit()) {
        if (error) *error = file.errorString();
        return false;
    }
    if (error) error->clear();
    return true;
}
