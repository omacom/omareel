// captureexclusion: src/record/CaptureExclusion.cpp (compiled as it is; everything it decides is
// public): compatibleHash, overlayMonitor, status, ensureLoaded with OMAREEL_PLUGIN_PATH,
// overlaysOffMonitor and HardwareCursorHold, with hyprctl answered by the replay shim.
// tests/golden/README.md, captureexclusion.

#include "common.h"
#include "recording.h"
#include "replay.h"

#include "record/CaptureExclusion.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

namespace Oracle {

namespace {

using Omareel::CaptureExclusion;
using Omareel::HardwareCursorHold;

QStringList exclusionStrings(const QJsonValue &value)
{
    QStringList result;
    for (const QJsonValue &item : value.toArray()) result << item.toString();
    return result;
}

void dumpStatus(Text &out, const CaptureExclusion::Status &status)
{
    out.row({"loaded", boolText(status.loaded)});
    out.row({"builtHash", quote(status.builtHash)});
    out.row({"reason", quote(sanitize(status.reason))});
    out.row({"details", QJsonDocument(status.details).toJson(QJsonDocument::Compact)});
}

// Replaces @PLUGIN@ in the answers' argv by the plugin's absolute path.
QJsonArray withPlugin(const QJsonArray &answers, const QString &plugin)
{
    QJsonArray result;
    for (const QJsonValue &value : answers) {
        QJsonObject object = value.toObject();
        QJsonArray argv;
        for (const QJsonValue &argument : object.value(QStringLiteral("argv")).toArray())
            argv << (argument.toString() == QLatin1String("@PLUGIN@") ? QJsonValue(plugin) : argument);
        object.insert(QStringLiteral("argv"), argv);
        result << object;
    }
    return result;
}

void exclusionHashes(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QStringList hashes = exclusionStrings(cases.value(QStringLiteral("hashes")));
    for (const QString &built : hashes)
        for (const QString &running : hashes)
            out.row({quote(built), quote(running), boolText(CaptureExclusion::compatibleHash(built, running))});
    writeFile(QDir(context.output).filePath(QStringLiteral("compatible-hash.txt")), out.data());
}

void exclusionOverlayMonitor(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject input = cases.value(QStringLiteral("overlayMonitor")).toObject();
    for (const bool plugin : {false, true})
        for (const QString &recorded : exclusionStrings(input.value(QStringLiteral("recorded"))))
            for (const QJsonValue &monitors : input.value(QStringLiteral("monitors")).toArray())
                out.row({boolText(plugin), quote(recorded), quoteList(exclusionStrings(monitors)),
                         quote(CaptureExclusion::overlayMonitor(plugin, recorded, exclusionStrings(monitors)))});
    writeFile(QDir(context.output).filePath(QStringLiteral("overlay-monitor.txt")), out.data());
}

void exclusionStatus(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject scenarios = cases.value(QStringLiteral("status")).toObject();
    for (auto it = scenarios.begin(); it != scenarios.end(); ++it) {
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay")));
        replay.answer(it.value().toArray(), context.corpus);
        replay.activate();
        ScopedEnvironment environment;
        if (it.value().isNull())
            environment.set("PATH", QFile::encodeName(shimDirectory(context.scratch, QStringLiteral("empty"), {})));
        out << "== " << it.key().toUtf8() << "\n";
        dumpStatus(out, CaptureExclusion::status());
        replay.dumpCalls(out);
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("status.txt")), out.data());
}

void exclusionEnsureLoaded(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject scenarios = cases.value(QStringLiteral("ensureLoaded")).toObject();
    const QString plugins = QDir(context.scratch).filePath(QStringLiteral("plugins"));
    for (auto it = scenarios.begin(); it != scenarios.end(); ++it) {
        const QJsonObject scenario = it.value().toObject();
        QDir(plugins).removeRecursively();
        QDir().mkpath(plugins);
        const QString file = QDir(plugins).filePath(QStringLiteral("omareel-capture-exclude.so"));
        ScopedEnvironment environment;
        const QJsonValue kind = scenario.value(QStringLiteral("plugin"));
        QString variable;
        if (kind.isNull()) {
            environment.unset("OMAREEL_PLUGIN_PATH");
        } else if (kind.toString() == QLatin1String("missing")) {
            variable = QDir(plugins).filePath(QStringLiteral("missing.so"));
        } else if (kind.toString() == QLatin1String("directory")) {
            variable = QDir(plugins).filePath(QStringLiteral("directory.so"));
            QDir().mkpath(variable);
        } else if (kind.toString() == QLatin1String("empty-variable")) {
            environment.set("OMAREEL_PLUGIN_PATH", QByteArray());
        } else {
            writeFile(file, "fixture");
            if (scenario.value(QStringLiteral("hash")).isString())
                writeFile(file + QStringLiteral(".hash"), scenario.value(QStringLiteral("hash")).toString().toUtf8());
            // Relative to the working directory, $HOME.
            variable = kind.toString() == QLatin1String("relative")
                ? QDir(QDir::homePath()).relativeFilePath(file) : file;
        }
        if (!variable.isEmpty()) environment.set("OMAREEL_PLUGIN_PATH", QFile::encodeName(variable));
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay")));
        replay.answer(withPlugin(scenario.value(QStringLiteral("answers")).toArray(), file), context.corpus);
        replay.activate();
        out << "== " << it.key().toUtf8() << "\n";
        out.row({"OMAREEL_PLUGIN_PATH", kind.isNull() ? QByteArray("<unset>") : quote(sanitize(variable))});
        dumpStatus(out, CaptureExclusion::ensureLoaded());
        replay.dumpCalls(out);
    }
    QDir(plugins).removeRecursively();
    writeFile(QDir(context.output).filePath(QStringLiteral("ensure-loaded.txt")), out.data());
}

void exclusionOverlaysOff(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject input = cases.value(QStringLiteral("overlaysOffMonitor")).toObject();
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("hyprctl/layers"));
    const auto row = [&](const QByteArray &label, const Replay::Answer &answer) {
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay")));
        replay.answer(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("layers")}, answer);
        replay.activate();
        QList<QByteArray> cells{label};
        for (const QString &monitor : exclusionStrings(input.value(QStringLiteral("monitors"))))
            cells << quote(monitor) + '=' + boolText(CaptureExclusion::overlaysOffMonitor(monitor));
        out.row(cells);
    };
    for (const QString &name : exclusionStrings(input.value(QStringLiteral("files")))) {
        Replay::Answer answer;
        answer.out = readFile(QDir(directory).filePath(name));
        row(name.toUtf8(), answer);
    }
    Replay::Answer failing;
    failing.out = readFile(QDir(directory).filePath(QStringLiteral("crafted-clean-two.json")));
    failing.status = 1;
    row("<crafted-clean-two.json, exit status 1>", failing);
    writeFile(QDir(context.output).filePath(QStringLiteral("overlays-off-monitor.txt")), out.data());
}

void exclusionHardwareCursor(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject scenarios = cases.value(QStringLiteral("hardwareCursor")).toObject();
    for (auto it = scenarios.begin(); it != scenarios.end(); ++it) {
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay")));
        replay.answer(it.value().toArray(), context.corpus);
        replay.activate();
        out << "== " << it.key().toUtf8() << "\n";
        qsizetype held = 0;
        {
            const HardwareCursorHold hold;
            held = replay.calls().size();
        }
        const QList<QStringList> calls = replay.calls();
        for (qsizetype index = 0; index < calls.size(); ++index)
            out.row({index < held ? QByteArray("hold") : QByteArray("release"), quoteList(calls.at(index))});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("hardware-cursor.txt")), out.data());
}

} // namespace

void runCaptureExclusion(const Context &context)
{
    const QJsonObject cases = corpusObject(context, QStringLiteral("captureexclusion/cases.json"));
    exclusionHashes(context, cases);
    exclusionOverlayMonitor(context, cases);
    exclusionStatus(context, cases);
    exclusionEnsureLoaded(context, cases);
    exclusionOverlaysOff(context, cases);
    exclusionHardwareCursor(context, cases);
}

} // namespace Oracle
