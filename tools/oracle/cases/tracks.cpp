// keystroketrack and motiontrack: KeystrokeTrack::build / sample over key logs and every
// Keystrokes setting, and MotionTrack::build / sample / ripples over short recordings with the
// project's cursor and zoom settings varied.

#include "timeline.h"

#include "core/InputLog.h"
#include "core/KeystrokeTrack.h"
#include "core/MotionTrack.h"
#include "core/ZoomTimeline.h"

#include <QDir>
#include <QFileInfo>

#include <algorithm>

namespace Oracle {

namespace {

constexpr double Infinity = std::numeric_limits<double>::infinity();

Omareel::Keystrokes keystrokesFromJson(const QJsonObject &o)
{
    Omareel::Keystrokes k;
    if (o.contains(QStringLiteral("enabled"))) k.enabled = o.value(QStringLiteral("enabled")).toBool();
    if (o.contains(QStringLiteral("position"))) k.position = o.value(QStringLiteral("position")).toString();
    if (o.contains(QStringLiteral("size"))) k.size = o.value(QStringLiteral("size")).toDouble();
    if (o.contains(QStringLiteral("showOnlyShortcuts")))
        k.showOnlyShortcuts = o.value(QStringLiteral("showOnlyShortcuts")).toBool();
    if (o.contains(QStringLiteral("holdMs"))) k.holdMs = o.value(QStringLiteral("holdMs")).toInt();
    return k;
}

QVector<double> uniform(int denominator, int from, int to)
{
    QVector<double> values;
    for (int i = from; i <= to; ++i) values << i / double(denominator);
    return values;
}

Omareel::Spring springFromArray(const QJsonValue &value)
{
    const QJsonArray a = value.toArray();
    return {a.at(0).toDouble(), a.at(1).toDouble(), a.at(2).toDouble()};
}

// The "set" overrides of motiontrack/cases.json, written straight into the Project's fields
// (no clamping, unlike Project::fromJson).
void applyOverride(Omareel::Project &p, const QString &key, const QJsonValue &v)
{
    if (key == QLatin1String("cursor.visible")) p.cursor.visible = v.toBool();
    else if (key == QLatin1String("cursor.smoothing")) p.cursor.smoothing = v.toBool();
    else if (key == QLatin1String("cursor.spring")) p.cursor.spring = springFromArray(v);
    else if (key == QLatin1String("cursor.clickEffect")) p.cursor.clickEffect = v.toString();
    else if (key == QLatin1String("cursor.clickShrink")) p.cursor.clickShrink = v.toDouble();
    else if (key == QLatin1String("cursor.rotateOnXMovementRatio")) p.cursor.rotateOnXMovementRatio = v.toDouble();
    else if (key == QLatin1String("cursor.hideWhenIdleMs")) p.cursor.hideWhenIdleMs = v.toInt();
    else if (key == QLatin1String("zoomStyle.spring")) p.zoomStyle.spring = springFromArray(v);
    else if (key == QLatin1String("zoomStyle.instantAnimation")) p.zoomStyle.instantAnimation = v.toBool();
    else if (key == QLatin1String("zoomStyle.motionBlur")) p.zoomStyle.motionBlur = v.toDouble();
    else if (key == QLatin1String("zoomStyle.snapToEdgesRatio")) p.zoomStyle.snapToEdgesRatio = v.toDouble();
    else fail(QStringLiteral("motiontrack/cases.json: unknown setting %1").arg(key));
}

QVector<Omareel::ZoomSegment> zoomsFromJson(const QJsonArray &array)
{
    QVector<Omareel::ZoomSegment> segments;
    for (const QJsonValue &value : array) {
        const QJsonObject o = value.toObject();
        segments << Omareel::ZoomSegment{o.value(QStringLiteral("id")).toString(),
                                         o.value(QStringLiteral("start")).toDouble(),
                                         o.value(QStringLiteral("end")).toDouble(),
                                         o.value(QStringLiteral("level")).toDouble(),
                                         o.value(QStringLiteral("auto")).toBool(),
                                         QPointF(o.value(QStringLiteral("x")).toDouble(),
                                                 o.value(QStringLiteral("y")).toDouble())};
    }
    return segments;
}

QByteArray motionCells(const Omareel::MotionTrack &track, double t)
{
    const Omareel::MotionSample s = track.sample(t);
    QList<QByteArray> cells{num(t), num(s.zoomScale), num(s.zoomCx), num(s.zoomCy),
                            num(s.zoomCenterVelocity), num(s.cursorX), num(s.cursorY),
                            num(s.cursorScale), num(s.cursorOpacity), num(s.cursorRotation)};
    const auto ripples = track.ripples(t);
    cells << QByteArray::number(ripples.size());
    for (const auto &r : ripples) cells << num(r.x) << num(r.y) << num(r.progress);
    return cells.join('\t');
}

} // namespace

void runKeystrokeTrack(const Context &context)
{
    const QJsonObject cases = readJson(QDir(context.corpus).filePath(QStringLiteral("keystroketrack/cases.json"))).toObject();
    const QJsonObject settings = cases.value(QStringLiteral("settings")).toObject();
    Text out;
    for (const QJsonValue &runValue : cases.value(QStringLiteral("runs")).toArray()) {
        const QString log = runValue.toArray().at(0).toString();
        QString error = QStringLiteral("<untouched>");
        const Omareel::InputLog input = Omareel::InputLog::load(QDir(context.corpus).filePath(log), 1000000, -1.0, &error);
        if (!error.isEmpty()) fail(QStringLiteral("%1: %2").arg(log, error));
        QVector<double> keyTimes;
        for (const auto &event : input.events())
            if (event.kind == Omareel::InputKind::KeyDown) keyTimes << event.time;
        const double last = keyTimes.isEmpty() ? 0.0 : *std::max_element(keyTimes.begin(), keyTimes.end());
        for (const QJsonValue &nameValue : runValue.toArray().at(1).toArray()) {
            const QString name = nameValue.toString();
            if (!settings.contains(name)) fail(QStringLiteral("keystroketrack: no settings %1").arg(name));
            const Omareel::Keystrokes keystrokes = keystrokesFromJson(settings.value(name).toObject());
            const Omareel::KeystrokeTrack track = Omareel::KeystrokeTrack::build(input.events(), keystrokes);
            out << "== " << log.toUtf8() << " settings=" << name.toUtf8() << "\n";
            const double hold = keystrokes.holdMs / 1000.0;
            const double end = last + std::min(std::max(hold, 0.0), 2.0) + 0.5;
            QVector<double> times = uniform(10, -5, int(std::ceil(10 * end)));
            for (const double e : keyTimes) {
                times << before(e) << e << after(e);
                const double fadeEnd = e + hold + 0.25;
                if (fadeEnd >= -0.5 && fadeEnd <= end)
                    times << e + hold << before(fadeEnd) << fadeEnd << after(fadeEnd);
            }
            times = sortedUnique(times);
            for (const double t : times) {
                const auto pills = track.sample(t);
                QList<QByteArray> cells{num(t), QByteArray::number(pills.size())};
                for (const auto &pill : pills) cells << quoteList(pill.keys) << quote(pill.text) << num(pill.opacity);
                out.row(cells);
            }
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("sample.txt")), out.data());
}

void runMotionTrack(const Context &context)
{
    const QJsonObject cases = readJson(QDir(context.corpus).filePath(QStringLiteral("motiontrack/cases.json"))).toObject();
    Text out;
    for (auto it = cases.begin(); it != cases.end(); ++it) {
        const QJsonObject c = it.value().toObject();
        const QString input = QDir(context.corpus).filePath(c.value(QStringLiteral("input")).toString());
        const double duration = c.value(QStringLiteral("duration")).toDouble();
        QString error = QStringLiteral("<untouched>");
        // A bundle is read the way the editor reads it (with the recording's duration); a log
        // file with first frame 1000000 us and no duration cut.
        const Omareel::InputLog log = QFileInfo(input).isDir()
            ? Omareel::InputLog::loadBundle(input, duration, &error)
            : Omareel::InputLog::load(input, 1000000, -1.0, &error);
        if (!error.isEmpty()) fail(QStringLiteral("motiontrack %1: %2").arg(it.key(), error));

        Omareel::Project project = Omareel::Project::defaults(QStringLiteral("Motion"), duration);
        const QJsonValue zooms = c.value(QStringLiteral("zooms"));
        if (zooms.toString() == QLatin1String("generate")) {
            QVector<double> clicks;
            for (const auto &click : log.clickDowns(true)) clicks << click.time;
            project.zooms = Omareel::ZoomTimeline::generate(clicks, duration);
        } else {
            project.zooms = zoomsFromJson(zooms.toArray());
        }
        const QJsonObject overrides = c.value(QStringLiteral("set")).toObject();
        for (auto o = overrides.begin(); o != overrides.end(); ++o) applyOverride(project, o.key(), o.value());

        const Omareel::MotionTrack track = Omareel::MotionTrack::build(
            duration, c.value(QStringLiteral("width")).toInt(), c.value(QStringLiteral("height")).toInt(),
            log.events(), project, c.value(QStringLiteral("captureFps")).toDouble());
        out << "== " << it.key().toUtf8() << "\n";
        out.row({"duration", num(track.duration())});
        out.row({"size", QByteArray::number(track.size())});
        const double end = track.duration();
        QVector<double> times = uniform(50, -5, int(std::ceil(50 * end)) + 5);
        times << before(end) << end << after(end);
        for (const auto &click : log.clickDowns(false))
            for (const double edge : {click.time, click.time + 0.45})
                times << before(edge) << edge << after(edge);
        times = sortedUnique(times) << -Infinity << Infinity;
        for (const double t : times) out << motionCells(track, t) << "\n";
    }
    out << "== default-constructed\n";
    const Omareel::MotionTrack empty{};
    out.row({"duration", num(empty.duration())});
    out.row({"size", QByteArray::number(empty.size())});
    for (const double t : {-1.0, 0.0, 0.5, Infinity}) out << motionCells(empty, t) << "\n";
    writeFile(QDir(context.output).filePath(QStringLiteral("sample.txt")), out.data());
}

} // namespace Oracle
