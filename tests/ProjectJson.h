#pragma once

// project.json for the C++ tests that set up a bundle by hand. The project model is Rust
// (omareel_core::project), so these build the JSON a new recording's project saves, which the
// tests then edit, as they once edited a C++ Project before saving it.

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QString>
#include <QStringList>

namespace ProjectJson {

// What Project::defaults(name, duration) saves: one clip, "c1", over the whole recording, and
// the default gradient. It is at the current defaultsVersion, so loading it neither migrates
// nor rewrites it.
inline QJsonObject defaults(const QString &name, double duration)
{
    const auto spring = [](double mass, double stiffness, double damping) {
        return QJsonObject{{"mass", mass}, {"stiffness", stiffness}, {"damping", damping}};
    };
    const auto shadow = [](bool enabled, double intensity, double blur, double distance, double angle) {
        return QJsonObject{{"enabled", enabled}, {"intensity", intensity}, {"blur", blur},
                           {"distance", distance}, {"angle", angle}};
    };
    const auto border = [](bool enabled, double width, const char *color, double alpha) {
        return QJsonObject{{"enabled", enabled}, {"width", width}, {"color", color}, {"alpha", alpha}};
    };
    return {{"version", 1}, {"defaultsVersion", 4}, {"name", name},
        {"aspect", QJsonValue(QJsonValue::Null)},
        {"crop", QJsonObject{{"x", 0.0}, {"y", 0.0}, {"w", 1.0}, {"h", 1.0}}},
        {"clips", QJsonArray{QJsonObject{{"id", "c1"}, {"in", 0.0}, {"out", duration}, {"speed", 1.0}}}},
        {"zooms", QJsonArray{}},
        {"zoomStyle", QJsonObject{{"spring", spring(2.25, 200.0, 40.0)}, {"snapToEdgesRatio", 0.25},
            {"instantAnimation", false}, {"motionBlur", 0.0}}},
        {"background", QJsonObject{{"type", "wallpaper"}, {"wallpaper", "omarchy:current"},
            {"gradient", QJsonObject{{"angle", 135.0},
                {"stops", QJsonArray{QJsonArray{"#ff8a00", 0.0}, QJsonArray{"#e52e71", 1.0}}}}},
            {"color", "#1a1b26"}, {"image", QJsonValue(QJsonValue::Null)}, {"blur", 0.0}}},
        {"frame", QJsonObject{{"padding", 0.10}, {"radius", 12.0},
            {"shadow", shadow(true, 0.25, 40.0, 10.0, 90.0)},
            {"border", border(false, 7.0, "#000000", 1.0)}}},
        {"cursor", QJsonObject{{"visible", true}, {"size", 1.5}, {"smoothing", true},
            {"spring", spring(3.0, 470.0, 70.0)}, {"clickEffect", "ripple"}, {"clickShrink", 0.8},
            {"rotateOnXMovementRatio", 0.5}, {"hideWhenIdleMs", QJsonValue(QJsonValue::Null)},
            {"style", "light-arrow"}, {"ringColor", "#7aa2f7"}, {"clickSound", "none"}}},
        {"keystrokes", QJsonObject{{"enabled", false}, {"position", "bottom-center"}, {"size", 1.0},
            {"holdMs", 900}}},
        {"audio", QJsonObject{{"desktop", true}, {"mic", true}, {"volume", 1.0}}},
        {"camera", QJsonObject{{"enabled", false}, {"position", "bottom-right"}, {"size", 0.25},
            {"shape", "round"}, {"radius", 16.0}, {"flipHorizontal", false}, {"rotation", 0},
            {"shadow", shadow(true, 0.28, 23.0, 20.0, 45.0)},
            {"border", border(true, 6.0, "#ffffff", 0.7)},
            {"scaleDuringZoom", 0.7}, {"offset", QJsonObject{{"x", 0.035}, {"y", 0.035}}},
            {"fullFrame", "fill"}}},
        {"export", QJsonObject{{"format", "mp4"}, {"fps", 60}, {"height", 1080}, {"quality", "social"},
            {"gif", QJsonObject{{"fps", 15}, {"height", 480}, {"quality", "studio"}, {"loop", true}}}}}};
}

// A piece of the recording, written as the project writes it: the layout only when it is not
// "default".
inline QJsonObject clip(const QString &id, double in, double out, double speed = 1.0,
                        const QString &layout = QStringLiteral("default"))
{
    QJsonObject result{{"id", id}, {"in", in}, {"out", out}, {"speed", speed}};
    if (layout != QLatin1String("default")) result.insert("layout", layout);
    return result;
}

// An inserted video: media is its path inside the bundle, fit how it meets the canvas.
inline QJsonObject insertedClip(const QString &id, double in, double out, const QString &media,
                                const QString &fit = QStringLiteral("fit"))
{
    QJsonObject result = clip(id, in, out);
    result.insert("media", media);
    result.insert("fit", fit);
    return result;
}

// Sets the member at path, such as {"camera", "shadow", "enabled"}.
inline void set(QJsonObject &object, QStringList path, const QJsonValue &value)
{
    const QString key = path.takeFirst();
    if (path.isEmpty()) {
        object.insert(key, value);
        return;
    }
    QJsonObject child = object.value(key).toObject();
    set(child, path, value);
    object.insert(key, child);
}

// Writes project.json as the editor saves it: indented, replacing the file in one step.
inline bool save(const QJsonObject &project, const QString &path, QString *error)
{
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        *error = file.errorString();
        return false;
    }
    if (file.write(QJsonDocument(project).toJson(QJsonDocument::Indented)) < 0 || !file.commit()) {
        *error = file.errorString();
        return false;
    }
    error->clear();
    return true;
}

} // namespace ProjectJson
