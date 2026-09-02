#pragma once

#include <QColor>
#include <QJsonObject>
#include <QPointF>
#include <QRectF>
#include <QString>
#include <QStringList>
#include <QVector>

namespace OmaRecord {

struct Spring { double mass = 1.0; double stiffness = 100.0; double damping = 20.0; };
struct GradientStop { QColor color; double position = 0.0; };
struct Gradient { double angle = 135.0; QVector<GradientStop> stops; };
struct Background {
    QString type = QStringLiteral("wallpaper");
    QString wallpaper = QStringLiteral("omarchy:current");
    Gradient gradient;
    QColor color = QColor(QStringLiteral("#1a1b26"));
    QString image;
    double blur = 0.0;
};
struct Shadow {
    bool enabled = true;
    double intensity = 0.75;
    double blur = 20.0;
    double distance = 25.0;
    double angle = 90.0;
};
struct Inset {
    bool enabled = false;
    double width = 0.0;
    QColor color = QColor(Qt::black);
    double alpha = 0.5;
};
struct Frame { double padding = 0.10; double radius = 12.0; Shadow shadow; Inset inset; };
struct Cursor {
    bool visible = true;
    double size = 1.5;
    bool smoothing = true;
    Spring spring{3.0, 470.0, 70.0};
    QString clickEffect = QStringLiteral("ripple");
    double clickShrink = 0.8;
    double rotateOnXMovementRatio = 0.5;
    int hideWhenIdleMs = -1;
    QString style = QStringLiteral("macos");
    QColor ringColor = QColor(QStringLiteral("#7aa2f7"));
};
struct ZoomStyle {
    Spring spring{2.25, 200.0, 40.0};
    double snapToEdgesRatio = 0.25;
    bool instantAnimation = false;
};
struct Clip { QString id; double in = 0.0; double out = 0.0; double speed = 1.0; };
struct ZoomSegment {
    QString id;
    double start = 0.0;
    double end = 0.0;
    double level = 2.0;
    bool automaticTarget = true;
    QPointF target{0.5, 0.5};
};
struct Audio { bool desktop = true; bool mic = true; double volume = 1.0; };
struct Camera {
    bool enabled = false;
    QString position = QStringLiteral("bottom-right");
    double size = 0.25;
    QString shape = QStringLiteral("round");
    double radius = 16.0;
    bool mirror = true;
    QPointF offset{0.02, 0.02};
};
struct GifExport { int fps = 15; int height = 480; QString quality = QStringLiteral("studio"); bool loop = true; };
struct ExportSettings {
    QString format = QStringLiteral("mp4");
    int fps = 60;
    int height = 1080;
    QString quality = QStringLiteral("social");
    GifExport gif;
};

class Project
{
public:
    static Project defaults(const QString &name = QStringLiteral("Untitled Recording"),
                            double duration = 0.0);
    static Project fromJson(const QJsonObject &json);
    static Project load(const QString &path, QString *error = nullptr);

    bool save(const QString &path, QString *error = nullptr) const;
    QJsonObject toJson() const;
    QJsonObject json() const { return toJson(); }
    void setJson(const QJsonObject &json) { *this = fromJson(json); }

    int version = 1;
    QString name = QStringLiteral("Untitled Recording");
    QString aspect = QStringLiteral("auto");
    QRectF crop{0.0, 0.0, 1.0, 1.0};
    QVector<Clip> clips;
    QVector<ZoomSegment> zooms;
    ZoomStyle zoomStyle;
    Background background;
    Frame frame;
    Cursor cursor;
    Audio audio;
    Camera camera;
    ExportSettings exportSettings;
};

QJsonObject springToJson(const Spring &spring);
Spring springFromJson(const QJsonObject &json, const Spring &defaults);
QVector<double> allowedClipSpeeds();
QStringList allowedAspects();

} // namespace OmaRecord
