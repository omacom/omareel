// regionpicker: src/record/RegionPicker.cpp, included whole so its file-static helpers (monitors,
// captureOptions, capturableMonitor, parseRect, monitorFor, windowRectangles, snapToWindow) run as
// the tag has them, and RegionPicker::pick end to end with hyprctl, gpu-screen-recorder, slurp and
// omarchy-capture-region answered by the replay shims. tests/golden/README.md, regionpicker.

#include "record/RegionPicker.cpp"

#include "common.h"
#include "recording.h"
#include "replay.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonObject>

#include <limits>

namespace Oracle {

namespace {

using Omareel::CaptureMode;
using Omareel::CaptureRegion;
using Omareel::RegionPicker;

QByteArray pickerModeText(CaptureMode mode)
{
    switch (mode) {
    case CaptureMode::Fullscreen: return "Fullscreen";
    case CaptureMode::Region: return "Region";
    case CaptureMode::Window: return "Window";
    }
    return "Mode" + QByteArray::number(int(mode));
}

CaptureMode pickerMode(const QString &name)
{
    if (name == QLatin1String("Fullscreen")) return CaptureMode::Fullscreen;
    if (name == QLatin1String("Window")) return CaptureMode::Window;
    return CaptureMode::Region;
}

QList<QByteArray> monitorCells(const Monitor &monitor)
{
    return {quote(monitor.name), num(monitor.x), num(monitor.y), num(monitor.logicalWidth),
            num(monitor.logicalHeight), num(monitor.scale), QByteArray::number(monitor.physicalWidth),
            QByteArray::number(monitor.physicalHeight), boolText(monitor.focused)};
}

QStringList stringList(const QJsonValue &value)
{
    QStringList result;
    for (const QJsonValue &item : value.toArray()) result << item.toString();
    return result;
}

// One scenario's world: replayed answers, the PATH it names, OMAREEL_CAPTURE and a settings.json
// with its captureBackend in a config directory of its own.
struct World {
    Replay replay;
    ScopedEnvironment environment;

    World(const Context &context, const QJsonObject &scenario)
        : replay(QDir(context.scratch).filePath(QStringLiteral("replay")))
    {
        replay.answer(scenario.value(QStringLiteral("answers")).toArray(), context.corpus);
        if (scenario.contains(QStringLiteral("programs")))
            environment.set("PATH", QFile::encodeName(shimDirectory(context.scratch, QStringLiteral("scenario"),
                                                                    stringList(scenario.value(QStringLiteral("programs"))))));
        const QJsonObject env = scenario.value(QStringLiteral("env")).toObject();
        environment.unset("OMAREEL_CAPTURE");
        for (auto it = env.begin(); it != env.end(); ++it)
            environment.set(it.key().toUtf8().constData(), it.value().toString().toUtf8());
        const QString config = QDir(context.scratch).filePath(QStringLiteral("config"));
        QDir(config).removeRecursively();
        QDir().mkpath(config);
        if (scenario.contains(QStringLiteral("captureBackend")))
            writeFile(QDir(config).filePath(QStringLiteral("omareel/settings.json")),
                      "{\"captureBackend\": \"" + scenario.value(QStringLiteral("captureBackend")).toString().toUtf8() + "\"}\n");
        environment.set("XDG_CONFIG_HOME", QFile::encodeName(config));
        replay.activate();
    }
};

QVector<Monitor> monitorsFrom(const Context &context, const QString &file, QString *error = nullptr)
{
    Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-monitors")));
    Replay::Answer answer;
    answer.out = readFile(QDir(context.corpus).filePath(file));
    replay.answer(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("monitors")}, answer);
    replay.activate();
    return monitors(error);
}

void pickerCaptureOptions(const Context &context)
{
    Text out;
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("gpu-screen-recorder/list-capture-options"));
    for (const QString &name : sortedEntries(directory, false))
        out.row({name.toUtf8(), quoteList(RegionPicker::parseCaptureOptions(QString::fromUtf8(readFile(QDir(directory).filePath(name)))))});
    writeFile(QDir(context.output).filePath(QStringLiteral("parse-capture-options.txt")), out.data());
}

void pickerClicks(const Context &context)
{
    Text out;
    const double inf = std::numeric_limits<double>::infinity();
    const QList<double> values{-1.0, -0.0, 0.0, 1e-9, 1.0, 2.0, 4.0, 4.4, 4.4721359549995796, 4.4721359549995805,
                               5.0, 19.0, 19.999, 20.0, 21.0, 1e300, inf, qQNaN()};
    for (const double width : values)
        for (const double height : values)
            out.row({num(width), num(height), boolText(RegionPicker::isClickSelection(width, height))});
    writeFile(QDir(context.output).filePath(QStringLiteral("is-click-selection.txt")), out.data());
}

void pickerRects(const Context &context, const QJsonObject &cases)
{
    Text out;
    for (const QString &text : stringList(cases.value(QStringLiteral("rects")))) {
        double x = -7, y = -7, w = -7, h = -7;
        const bool ok = parseRect(text, &x, &y, &w, &h);
        out.row({quote(text), boolText(ok), num(x), num(y), num(w), num(h)});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("parse-rect.txt")), out.data());
}

void pickerMonitors(const Context &context)
{
    Text out;
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("hyprctl/monitors"));
    const auto dump = [&](const QByteArray &label, const Replay::Answer &answer, bool onPath = true) {
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-monitors")));
        replay.answer(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("monitors")}, answer);
        replay.activate();
        ScopedEnvironment environment;
        if (!onPath) environment.set("PATH", QFile::encodeName(shimDirectory(context.scratch, QStringLiteral("empty"), {})));
        QString error = QStringLiteral("<untouched>");
        const QVector<Monitor> values = monitors(&error);
        out << "== " << label << "\n";
        out.row({"error", quote(error)});
        for (const Monitor &monitor : values) out.row(monitorCells(monitor));
    };
    for (const QString &name : sortedEntries(directory, false)) {
        Replay::Answer answer;
        answer.out = readFile(QDir(directory).filePath(name));
        dump(name.toUtf8(), answer);
    }
    Replay::Answer failing;
    failing.out = readFile(QDir(directory).filePath(QStringLiteral("crafted-two.json")));
    failing.err = "Couldn't connect to /run/user/1000/hypr/x/.socket.sock. (3)\n";
    failing.status = 1;
    dump("<crafted-two.json, exit status 1, stderr>", failing);
    Replay::Answer quiet;
    quiet.status = 3;
    dump("<exit status 3, no output>", quiet);
    dump("<hyprctl not on PATH>", Replay::Answer{}, false);
    writeFile(QDir(context.output).filePath(QStringLiteral("monitors.txt")), out.data());
}

void pickerMonitorFor(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject input = cases.value(QStringLiteral("monitorFor")).toObject();
    for (const QString &set : stringList(input.value(QStringLiteral("sets")))) {
        const QVector<Monitor> values = monitorsFrom(context, set);
        for (const QJsonValue &rect : input.value(QStringLiteral("rects")).toArray()) {
            const QJsonArray r = rect.toArray();
            const double x = r.at(0).toDouble(), y = r.at(1).toDouble(), w = r.at(2).toDouble(), h = r.at(3).toDouble();
            const Monitor *found = monitorFor(values, x, y, w, h);
            QByteArray index = "-";
            if (found) index = QByteArray::number(found - values.constData());
            out.row({set.toUtf8(), num(x), num(y), num(w), num(h), index,
                     found ? quote(found->name) : QByteArray("-")});
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("monitor-for.txt")), out.data());
}

void pickerWindowRectangles(const Context &context, const QJsonObject &cases)
{
    Text out;
    for (const QJsonValue &value : cases.value(QStringLiteral("rectangles")).toArray()) {
        const QJsonObject pair = value.toObject();
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-windows")));
        const auto add = [&](const QString &request, const QJsonValue &file) {
            Replay::Answer answer;
            if (file.isString()) answer.out = readFile(QDir(context.corpus).filePath(file.toString()));
            else answer.status = 1;
            replay.answer(QStringLiteral("hyprctl"), {QStringLiteral("-j"), request}, answer);
        };
        add(QStringLiteral("activeworkspace"), pair.value(QStringLiteral("workspace")));
        add(QStringLiteral("clients"), pair.value(QStringLiteral("clients")));
        replay.activate();
        QString error = QStringLiteral("<untouched>");
        const QString rectangles = windowRectangles(&error);
        out << "== workspace=" << (pair.value(QStringLiteral("workspace")).isString()
                                       ? pair.value(QStringLiteral("workspace")).toString().toUtf8() : QByteArray("<exit 1>"))
            << " clients=" << (pair.value(QStringLiteral("clients")).isString()
                                   ? pair.value(QStringLiteral("clients")).toString().toUtf8() : QByteArray("<exit 1>"))
            << "\n";
        out.row({"result", quote(rectangles)});
        out.row({"error", quote(error)});
        replay.dumpCalls(out);
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("window-rectangles.txt")), out.data());
}

void pickerSnap(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject snap = cases.value(QStringLiteral("snap")).toObject();
    for (const QString &rectangles : stringList(snap.value(QStringLiteral("rectangles"))))
        for (const QJsonValue &point : snap.value(QStringLiteral("points")).toArray()) {
            const double px = point.toArray().at(0).toDouble(), py = point.toArray().at(1).toDouble();
            double x = -7, y = -7, w = -7, h = -7;
            const bool found = snapToWindow(rectangles, px, py, &x, &y, &w, &h);
            out.row({quote(rectangles), num(px), num(py), boolText(found), num(x), num(y), num(w), num(h)});
        }
    writeFile(QDir(context.output).filePath(QStringLiteral("snap-to-window.txt")), out.data());
}

void pickerCapturable(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject scenarios = cases.value(QStringLiteral("capturable")).toObject();
    for (auto it = scenarios.begin(); it != scenarios.end(); ++it) {
        const QJsonObject scenario = it.value().toObject();
        const QVector<Monitor> values = monitorsFrom(context, scenario.value(QStringLiteral("monitors")).toString());
        const Monitor *requested = nullptr;
        for (const Monitor &monitor : values)
            if (monitor.name == scenario.value(QStringLiteral("requested")).toString()) requested = &monitor;
        if (!requested) fail(QStringLiteral("capturable scenario %1 names no monitor").arg(it.key()));
        World world(context, scenario);
        QString note = QStringLiteral("<untouched>");
        const Monitor *result = capturableMonitor(values, requested, &note);
        out << "== " << it.key().toUtf8() << "\n";
        out.row({"result", quote(requested->name), quote(result->name),
                 result == requested ? QByteArray("requested") : QByteArray::number(result - values.constData())});
        out.row({"note", quote(note)});
        world.replay.dumpCalls(out);
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("capturable-monitor.txt")), out.data());
}

void pickerPick(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject scenarios = cases.value(QStringLiteral("pick")).toObject();
    for (auto it = scenarios.begin(); it != scenarios.end(); ++it) {
        const QJsonObject scenario = it.value().toObject();
        World world(context, scenario);
        CaptureRegion region;
        region.monitorName = QStringLiteral("<untouched>");
        QString error = QStringLiteral("<untouched>");
        const CaptureMode mode = pickerMode(scenario.value(QStringLiteral("mode")).toString());
        const bool ok = RegionPicker::pick(mode, &region, &error);
        out << "== " << it.key().toUtf8() << "\n";
        out.row({"pick", pickerModeText(mode), boolText(ok)});
        out.row({"error", quote(sanitize(error))});
        QList<QByteArray> cells{"region"};
        cells << quote(region.monitorName) << num(region.x) << num(region.y) << num(region.width) << num(region.height)
              << num(region.scale) << QByteArray::number(region.physicalWidth)
              << QByteArray::number(region.physicalHeight) << pickerModeText(region.mode);
        out.row(cells);
        world.replay.dumpCalls(out);
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("pick.txt")), out.data());
}

} // namespace

void runRegionPicker(const Context &context)
{
    const QJsonObject cases = corpusObject(context, QStringLiteral("regionpicker/cases.json"));
    pickerCaptureOptions(context);
    pickerClicks(context);
    pickerRects(context, cases);
    pickerMonitors(context);
    pickerMonitorFor(context, cases);
    pickerWindowRectangles(context, cases);
    pickerSnap(context, cases);
    pickerCapturable(context, cases);
    pickerPick(context, cases);
}

} // namespace Oracle
