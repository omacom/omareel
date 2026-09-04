#include "Project.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QSaveFile>
#include <algorithm>

using namespace Omareel;

static QColor parseColor(const QJsonValue &value, const QColor &fallback)
{
    const QColor parsed(value.toString());
    return parsed.isValid() ? parsed : fallback;
}

QJsonObject Omareel::springToJson(const Spring &s)
{
    return {{"mass", s.mass}, {"stiffness", s.stiffness}, {"damping", s.damping}};
}

Spring Omareel::springFromJson(const QJsonObject &json, const Spring &fallback)
{
    return {json.value("mass").toDouble(fallback.mass),
            json.value("stiffness").toDouble(fallback.stiffness),
            json.value("damping").toDouble(fallback.damping)};
}

QVector<double> Omareel::allowedClipSpeeds()
{
    return {0.5, 0.75, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 3.0, 4.0, 8.0, 16.0, 24.0};
}

QStringList Omareel::allowedAspects()
{
    return {QStringLiteral("auto"), QStringLiteral("16:9"), QStringLiteral("1:1"),
            QStringLiteral("4:3"), QStringLiteral("9:16"), QStringLiteral("3:4"),
            QStringLiteral("4:5")};
}

Project Project::defaults(const QString &projectName, double duration)
{
    Project result;
    result.name = projectName;
    result.clips = {Clip{QStringLiteral("c1"), 0.0, duration, 1.0}};
    result.background.gradient.stops = {
        {QColor(QStringLiteral("#ff8a00")), 0.0},
        {QColor(QStringLiteral("#e52e71")), 1.0}};
    return result;
}

Project Project::fromJson(const QJsonObject &root)
{
    Project p = defaults(root.value("name").toString(QStringLiteral("Untitled Recording")), 0.0);
    p.version = root.value("version").toInt(1);
    const QString aspect = root.value("aspect").isNull() ? QStringLiteral("auto")
                                                          : root.value("aspect").toString("auto");
    if (allowedAspects().contains(aspect)) p.aspect = aspect;
    const auto crop = root.value("crop").toObject();
    p.crop = QRectF(crop.value("x").toDouble(0.0), crop.value("y").toDouble(0.0),
                    crop.value("w").toDouble(1.0), crop.value("h").toDouble(1.0));

    p.clips.clear();
    for (const auto &value : root.value("clips").toArray()) {
        const auto o = value.toObject();
        p.clips << Clip{o.value("id").toString(), o.value("in").toDouble(),
                       o.value("out").toDouble(), o.value("speed").toDouble(1.0)};
    }
    p.zooms.clear();
    for (const auto &value : root.value("zooms").toArray()) {
        const auto o = value.toObject();
        ZoomSegment z;
        z.id = o.value("id").toString();
        z.start = o.value("start").toDouble();
        z.end = o.value("end").toDouble();
        z.level = o.value("level").toDouble(2.0);
        const auto target = o.value("target");
        z.automaticTarget = !target.isObject();
        if (target.isObject()) {
            const auto t = target.toObject();
            z.target = QPointF(t.value("x").toDouble(0.5), t.value("y").toDouble(0.5));
        }
        p.zooms << z;
    }
    const auto zs = root.value("zoomStyle").toObject();
    p.zoomStyle.spring = springFromJson(zs.value("spring").toObject(), p.zoomStyle.spring);
    p.zoomStyle.snapToEdgesRatio = zs.value("snapToEdgesRatio").toDouble(0.25);
    p.zoomStyle.instantAnimation = zs.value("instantAnimation").toBool(false);
    p.zoomStyle.motionBlur = std::clamp(zs.value("motionBlur").toDouble(0.0), 0.0, 1.0);

    const auto bg = root.value("background").toObject();
    p.background.type = bg.value("type").toString(p.background.type);
    p.background.wallpaper = bg.value("wallpaper").toString(p.background.wallpaper);
    p.background.color = parseColor(bg.value("color"), p.background.color);
    p.background.image = bg.value("image").toString();
    p.background.blur = bg.value("blur").toDouble();
    const auto gradient = bg.value("gradient").toObject();
    p.background.gradient.angle = gradient.value("angle").toDouble(135.0);
    if (gradient.contains("stops")) {
        p.background.gradient.stops.clear();
        for (const auto &v : gradient.value("stops").toArray()) {
            const auto a = v.toArray();
            if (a.size() >= 2) p.background.gradient.stops << GradientStop{parseColor(a[0], Qt::black), a[1].toDouble()};
        }
    }

    const auto f = root.value("frame").toObject();
    p.frame.padding = f.value("padding").toDouble(0.10);
    p.frame.radius = f.value("radius").toDouble(12.0);
    const auto sh = f.value("shadow").toObject();
    p.frame.shadow.enabled = sh.value("enabled").toBool(true);
    p.frame.shadow.intensity = sh.value("intensity").toDouble(sh.value("opacity").toDouble(0.25));
    p.frame.shadow.blur = sh.value("blur").toDouble(40.0);
    p.frame.shadow.distance = sh.value("distance").toDouble(sh.value("offsetY").toDouble(10.0));
    p.frame.shadow.angle = sh.value("angle").toDouble(90.0);
    const auto border = (f.contains("border") ? f.value("border") : f.value("inset")).toObject();
    p.frame.border.enabled = border.value("enabled").toBool(false);
    p.frame.border.width = border.value("width").toDouble(7.0);
    p.frame.border.color = parseColor(border.value("color"), Qt::black);
    p.frame.border.alpha = border.value("alpha").toDouble(1.0);

    const auto c = root.value("cursor").toObject();
    p.cursor.visible = c.value("visible").toBool(true);
    p.cursor.size = c.value("size").toDouble(1.5);
    p.cursor.smoothing = c.value("smoothing").isBool() ? c.value("smoothing").toBool()
                                                        : c.value("smoothing").toDouble(0.8) > 0.0;
    p.cursor.spring = springFromJson(c.value("spring").toObject(), p.cursor.spring);
    p.cursor.clickEffect = c.value("clickEffect").toString("ripple");
    p.cursor.clickShrink = c.value("clickShrink").toDouble(0.8);
    p.cursor.rotateOnXMovementRatio = c.value("rotateOnXMovementRatio").toDouble(0.5);
    p.cursor.hideWhenIdleMs = c.value("hideWhenIdleMs").isNull() ? -1 : c.value("hideWhenIdleMs").toInt(-1);
    p.cursor.style = c.value("style").toString("light-arrow");
    const QStringList cursorStyles{QStringLiteral("light-arrow"), QStringLiteral("dark-arrow"),
                                   QStringLiteral("dot"), QStringLiteral("hand")};
    if (!cursorStyles.contains(p.cursor.style)) p.cursor.style = QStringLiteral("light-arrow");
    p.cursor.ringColor = parseColor(c.value("ringColor"), p.cursor.ringColor);
    p.cursor.clickSound = c.value("clickSound").toString("none");
    if (p.cursor.clickSound != QLatin1String("soft")) p.cursor.clickSound = QStringLiteral("none");

    const auto keystrokes = root.value("keystrokes").toObject();
    p.keystrokes.enabled = keystrokes.value("enabled").toBool(false);
    p.keystrokes.position = keystrokes.value("position").toString("bottom-center");
    const QStringList keystrokePositions{QStringLiteral("top-left"), QStringLiteral("top-center"),
        QStringLiteral("top-right"), QStringLiteral("bottom-left"),
        QStringLiteral("bottom-center"), QStringLiteral("bottom-right")};
    if (!keystrokePositions.contains(p.keystrokes.position))
        p.keystrokes.position = QStringLiteral("bottom-center");
    p.keystrokes.size = std::clamp(keystrokes.value("size").toDouble(1.0), 0.5, 2.0);
    p.keystrokes.showOnlyShortcuts = keystrokes.value("showOnlyShortcuts").toBool(true);
    p.keystrokes.holdMs = std::clamp(keystrokes.value("holdMs").toInt(900), 100, 5000);

    const auto a = root.value("audio").toObject();
    p.audio.desktop = a.value("desktop").toBool(true);
    p.audio.mic = a.value("mic").toBool(true);
    p.audio.volume = a.value("volume").toDouble(1.0);
    const auto camera = root.value("camera").toObject();
    p.camera.enabled = camera.value("enabled").toBool(false);
    p.camera.position = camera.value("position").toString("bottom-right");
    p.camera.size = camera.value("size").toDouble(0.25);
    const QString cameraShape = camera.value("shape").toString("round");
    if (cameraShape == QLatin1String("round") || cameraShape == QLatin1String("rounded")
        || cameraShape == QLatin1String("square"))
        p.camera.shape = cameraShape;
    p.camera.radius = camera.value("radius").toDouble(16.0);
    const QString cameraCrop = camera.value("crop").toString("original");
    if (cameraCrop == QLatin1String("square") || cameraCrop == QLatin1String("original"))
        p.camera.crop = cameraCrop;
    p.camera.flipHorizontal = camera.contains("flipHorizontal")
        ? camera.value("flipHorizontal").toBool(false)
        : camera.value("mirror").toBool(false);
    const int cameraRotation = camera.value("rotation").toInt(0);
    if (cameraRotation == 0 || cameraRotation == 90 || cameraRotation == 180
        || cameraRotation == 270)
        p.camera.rotation = cameraRotation;
    const QJsonValue cameraShadowValue = camera.value("shadow");
    if (cameraShadowValue.isObject()) {
        const QJsonObject cameraShadow = cameraShadowValue.toObject();
        p.camera.shadow.enabled = cameraShadow.value("enabled").toBool(false);
        p.camera.shadow.intensity = cameraShadow.value("intensity").toDouble(0.55);
        p.camera.shadow.blur = cameraShadow.value("blur").toDouble(18.0);
        p.camera.shadow.distance = cameraShadow.value("distance").toDouble(18.0);
    } else {
        p.camera.shadow.enabled = cameraShadowValue.toBool(false);
    }
    const QJsonObject cameraBorder = (camera.contains("border")
        ? camera.value("border") : camera.value("inset")).toObject();
    p.camera.border.enabled = cameraBorder.value("enabled").toBool(false);
    p.camera.border.width = cameraBorder.value("width").toDouble(2.0);
    p.camera.border.color = parseColor(cameraBorder.value("color"), Qt::white);
    p.camera.border.alpha = cameraBorder.value("alpha").toDouble(0.7);
    p.camera.scaleDuringZoom = camera.value("scaleDuringZoom").toDouble(0.7);
    const auto cameraOffset = camera.value("offset").toObject();
    p.camera.offset = QPointF(cameraOffset.value("x").toDouble(0.02),
                              cameraOffset.value("y").toDouble(0.02));
    const auto e = root.value("export").toObject();
    p.exportSettings.format = e.value("format").toString("mp4");
    p.exportSettings.fps = e.value("fps").toInt(60);
    p.exportSettings.height = e.value("height").toInt(1080);
    p.exportSettings.quality = e.value("quality").toString("social");
    const auto g = e.value("gif").toObject();
    p.exportSettings.gif.fps = g.value("fps").toInt(15);
    p.exportSettings.gif.height = g.value("height").toInt(480);
    p.exportSettings.gif.quality = g.value("quality").toString("studio");
    p.exportSettings.gif.loop = g.value("loop").toBool(true);
    return p;
}

QJsonObject Project::toJson() const
{
    QJsonArray clipArray;
    for (const auto &c : clips) clipArray << QJsonObject{{"id", c.id}, {"in", c.in}, {"out", c.out}, {"speed", c.speed}};
    QJsonArray zoomArray;
    for (const auto &z : zooms) {
        QJsonValue target = z.automaticTarget ? QJsonValue(QStringLiteral("auto"))
            : QJsonValue(QJsonObject{{"x", z.target.x()}, {"y", z.target.y()}});
        zoomArray << QJsonObject{{"id", z.id}, {"start", z.start}, {"end", z.end}, {"level", z.level}, {"target", target}};
    }
    QJsonArray stops;
    for (const auto &s : background.gradient.stops)
        stops << QJsonArray{s.color.name(QColor::HexRgb), s.position};
    const QJsonObject bg{{"type", background.type}, {"wallpaper", background.wallpaper},
        {"gradient", QJsonObject{{"angle", background.gradient.angle}, {"stops", stops}}},
        {"color", background.color.name(QColor::HexRgb)},
        {"image", background.image.isEmpty() ? QJsonValue(QJsonValue::Null) : QJsonValue(background.image)},
        {"blur", background.blur}};
    const QJsonObject frameJson{{"padding", frame.padding}, {"radius", frame.radius},
        {"shadow", QJsonObject{{"enabled", frame.shadow.enabled}, {"intensity", frame.shadow.intensity},
            {"blur", frame.shadow.blur}, {"distance", frame.shadow.distance}, {"angle", frame.shadow.angle}}},
        {"border", QJsonObject{{"enabled", frame.border.enabled}, {"width", frame.border.width},
            {"color", frame.border.color.name(QColor::HexRgb)}, {"alpha", frame.border.alpha}}}};
    const QJsonObject cursorJson{{"visible", cursor.visible}, {"size", cursor.size}, {"smoothing", cursor.smoothing},
        {"spring", springToJson(cursor.spring)}, {"clickEffect", cursor.clickEffect}, {"clickShrink", cursor.clickShrink},
        {"rotateOnXMovementRatio", cursor.rotateOnXMovementRatio},
        {"hideWhenIdleMs", cursor.hideWhenIdleMs < 0 ? QJsonValue(QJsonValue::Null) : QJsonValue(cursor.hideWhenIdleMs)},
        {"style", cursor.style}, {"ringColor", cursor.ringColor.name(QColor::HexRgb)},
        {"clickSound", cursor.clickSound}};
    return {{"version", version}, {"name", name},
        {"aspect", aspect == QLatin1String("auto") ? QJsonValue(QJsonValue::Null) : QJsonValue(aspect)},
        {"crop", QJsonObject{{"x", crop.x()}, {"y", crop.y()}, {"w", crop.width()}, {"h", crop.height()}}},
        {"clips", clipArray}, {"zooms", zoomArray},
        {"zoomStyle", QJsonObject{{"spring", springToJson(zoomStyle.spring)}, {"snapToEdgesRatio", zoomStyle.snapToEdgesRatio}, {"instantAnimation", zoomStyle.instantAnimation}, {"motionBlur", zoomStyle.motionBlur}}},
        {"background", bg}, {"frame", frameJson}, {"cursor", cursorJson},
        {"keystrokes", QJsonObject{{"enabled", keystrokes.enabled},
            {"position", keystrokes.position}, {"size", keystrokes.size},
            {"showOnlyShortcuts", keystrokes.showOnlyShortcuts}, {"holdMs", keystrokes.holdMs}}},
        {"audio", QJsonObject{{"desktop", audio.desktop}, {"mic", audio.mic}, {"volume", audio.volume}}},
        {"camera", QJsonObject{{"enabled", camera.enabled}, {"position", camera.position},
            {"size", camera.size}, {"shape", camera.shape}, {"radius", camera.radius},
            {"crop", camera.crop}, {"flipHorizontal", camera.flipHorizontal},
            {"rotation", camera.rotation},
            {"shadow", QJsonObject{{"enabled", camera.shadow.enabled},
                {"intensity", camera.shadow.intensity}, {"blur", camera.shadow.blur},
                {"distance", camera.shadow.distance}}},
            {"border", QJsonObject{{"enabled", camera.border.enabled},
                {"width", camera.border.width}, {"color", camera.border.color.name(QColor::HexRgb)},
                {"alpha", camera.border.alpha}}},
            {"scaleDuringZoom", camera.scaleDuringZoom}, {"offset", QJsonObject{{"x", camera.offset.x()},
                {"y", camera.offset.y()}}}}},
        {"export", QJsonObject{{"format", exportSettings.format}, {"fps", exportSettings.fps}, {"height", exportSettings.height},
            {"quality", exportSettings.quality}, {"gif", QJsonObject{{"fps", exportSettings.gif.fps}, {"height", exportSettings.gif.height},
                {"quality", exportSettings.gif.quality}, {"loop", exportSettings.gif.loop}}}}}};
}

Project Project::load(const QString &path, QString *error)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) { if (error) *error = file.errorString(); return {}; }
    QJsonParseError parseError;
    const auto doc = QJsonDocument::fromJson(file.readAll(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
        if (error) *error = parseError.errorString(); return {};
    }
    if (error) error->clear();
    return fromJson(doc.object());
}

bool Project::save(const QString &path, QString *error) const
{
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) { if (error) *error = file.errorString(); return false; }
    if (file.write(QJsonDocument(toJson()).toJson(QJsonDocument::Indented)) < 0 || !file.commit()) {
        if (error) *error = file.errorString(); return false;
    }
    if (error) error->clear();
    return true;
}
