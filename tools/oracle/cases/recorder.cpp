// recorder: what src/record/Recorder.cpp decides without a compositor. The tag's file is included
// whole (below), so its file-static helpers (writeInputLog, writeJson, drainRecorderOutput,
// lastNonEmptyLine, firstFrameTimestamp, writeStartTimestamp, monitorLayouts, selfViewPlacement,
// startAudioCapture, valueAfter...) run exactly as the tag has them; nothing of it is copied except
// the pactl parsing inside daemonMain (marked below). The programs it starts (hyprctl, pactl,
// ffmpeg) are the replay shims. tests/golden/README.md, recorder.

#include "record/Recorder.cpp"

#include "common.h"
#include "recording.h"
#include "replay.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLockFile>
#include <QProcess>
#include <QSysInfo>

#include <cmath>
#include <limits>
#include <sys/stat.h>
#include <fcntl.h>

namespace Oracle {

namespace {

using Omareel::CaptureMode;
using Omareel::CaptureRegion;
using Omareel::DeviceEvent;
using Omareel::DeviceEventKind;
using Omareel::RawCursorSample;
using Omareel::Recorder;
using Omareel::RecordOptions;
using Omareel::RecordingPreferences;

constexpr double inf = std::numeric_limits<double>::infinity();

QByteArray modeText(CaptureMode mode)
{
    switch (mode) {
    case CaptureMode::Fullscreen: return "Fullscreen";
    case CaptureMode::Region: return "Region";
    case CaptureMode::Window: return "Window";
    }
    return "Mode" + QByteArray::number(int(mode));
}

QList<QByteArray> regionCells(const CaptureRegion &region)
{
    return {quote(region.monitorName), num(region.x), num(region.y), num(region.width), num(region.height),
            num(region.scale), QByteArray::number(region.physicalWidth),
            QByteArray::number(region.physicalHeight), modeText(region.mode)};
}

QByteArray classificationText(Recorder::GsrExitClassification value)
{
    switch (value) {
    case Recorder::GsrExitClassification::UserStop: return "UserStop";
    case Recorder::GsrExitClassification::ExternalStop: return "ExternalStop";
    case Recorder::GsrExitClassification::Failure: return "Failure";
    }
    return "Classification" + QByteArray::number(int(value));
}

QByteArray compact(const QJsonObject &object)
{
    return QJsonDocument(object).toJson(QJsonDocument::Compact);
}

QStringList strings(const QJsonValue &value)
{
    QStringList result;
    for (const QJsonValue &item : value.toArray()) result << item.toString();
    return result;
}

void recordClassify(const Context &context)
{
    Text out;
    for (const int exitCode : {-1, 0, 1, 2, 130, 255})
        for (const bool requested : {false, true})
            for (const qint64 size : {qint64(-1), qint64(0), qint64(1), qint64(1048576)})
                for (const double duration : {qQNaN(), -inf, -1.0, -0.0, 0.0, 5e-324, 0.001, 2.5, inf})
                    out.row({QByteArray::number(exitCode), boolText(requested), QByteArray::number(size),
                             num(duration),
                             classificationText(Recorder::classifyGsrExit(exitCode, requested, size, duration))});
    writeFile(QDir(context.output).filePath(QStringLiteral("classify-gsr-exit.txt")), out.data());
}

void recordEvenRegion(const Context &context)
{
    Text out;
    for (const CaptureMode mode : {CaptureMode::Region, CaptureMode::Fullscreen, CaptureMode::Window})
        for (const double scale : {1.0, 2.0, 1.5, 1.25, 0.0, -1.0, qQNaN()})
            for (const int width : {-3, -2, -1, 0, 1, 2, 3, 1520, 1521})
                for (const int height : {-1, 0, 1, 2, 835, 836}) {
                    if (mode != CaptureMode::Region && (scale != 2.0 || (width != 1521 && width != 1520)))
                        continue;
                    CaptureRegion region{QStringLiteral("DP-1"), 10.0, 20.0, 760.5, 417.5, scale, width, height,
                                         mode};
                    QList<QByteArray> cells = regionCells(region);
                    cells << "->" << regionCells(Recorder::evenCaptureRegion(region));
                    out.row(cells);
                }
    writeFile(QDir(context.output).filePath(QStringLiteral("even-capture-region.txt")), out.data());
}

void recordCameraBlock(const Context &context)
{
    struct Call {
        QString device = QStringLiteral("/dev/v4l/by-id/usb-Insta360_Link_2-video-index0");
        int requestedHeight = 1080;
        int width = 1920;
        int height = 1080;
        double fps = 30.0;
        qint64 firstFrameUs = 1234567;
        QString backend = QStringLiteral("ffmpeg-pipe");
        int rotation = 90;
        bool flip = true;
        QString shape = QStringLiteral("round");
        double radius = 16;
    };
    Text out;
    const auto row = [&out](const QByteArray &label, const Call &call, bool defaults = false) {
        const QJsonObject block = defaults
            ? Recorder::cameraCaptureBlock(call.device, call.requestedHeight, call.width, call.height, call.fps,
                                           call.firstFrameUs, call.backend, call.rotation, call.flip)
            : Recorder::cameraCaptureBlock(call.device, call.requestedHeight, call.width, call.height, call.fps,
                                           call.firstFrameUs, call.backend, call.rotation, call.flip,
                                           call.shape, call.radius);
        out.row({label, quote(call.device), QByteArray::number(call.requestedHeight),
                 QByteArray::number(call.width), QByteArray::number(call.height), num(call.fps),
                 QByteArray::number(call.firstFrameUs), quote(call.backend), QByteArray::number(call.rotation),
                 boolText(call.flip), defaults ? QByteArray("<default>") : quote(call.shape),
                 defaults ? QByteArray("<default>") : num(call.radius), compact(block)});
    };
    row("base", Call{});
    row("default shape and radius", Call{}, true);
    for (const QString &device : {QString(), QStringLiteral("/dev/video0"), QStringLiteral("Cámara \"1\"")}) {
        Call call; call.device = device; row("device", call);
    }
    for (const int height : {720, 0, 480, 1440, -720, 721}) { Call call; call.requestedHeight = height; row("requestedHeight", call); }
    for (const auto &[width, height] : QList<QPair<int, int>>{{0, 0}, {-1, 7}, {1280, 720}, {2147483647, -2147483647 - 1}}) {
        Call call; call.width = width; call.height = height; row("size", call);
    }
    for (const double fps : {29.97002997002997, 0.0, -30.0, 1e-300, 60.000000000000007, qQNaN(), inf, -inf}) {
        Call call; call.fps = fps; row("fps", call);
    }
    for (const qint64 first : {qint64(0), qint64(-1), (qint64(1) << 53) + 1, std::numeric_limits<qint64>::max(),
                               std::numeric_limits<qint64>::min()}) {
        Call call; call.firstFrameUs = first; row("firstFrameUs", call);
    }
    for (const QString &backend : {QString(), QStringLiteral("v4l2")}) { Call call; call.backend = backend; row("backend", call); }
    for (const int rotation : {0, 45, 180, 270, -90, 360}) { Call call; call.rotation = rotation; row("rotation", call); }
    { Call call; call.flip = false; row("flipHorizontal", call); }
    for (const QString &shape : {QStringLiteral("square"), QStringLiteral("rectangle"), QStringLiteral("Square"),
                                 QStringLiteral("circle"), QString(), QStringLiteral("square "),
                                 QStringLiteral("rect")}) {
        Call call; call.shape = shape; row("shape", call);
    }
    for (const double radius : {0.0, -1.0, 0.5, 80.0, 1e300, qQNaN(), inf}) { Call call; call.radius = radius; row("radius", call); }
    writeFile(QDir(context.output).filePath(QStringLiteral("camera-capture-block.txt")), out.data());
}

void recordMicrophone(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject microphone = cases.value(QStringLiteral("microphone")).toObject();
    const QJsonObject sources = microphone.value(QStringLiteral("sources")).toObject();
    for (const QString &device : strings(microphone.value(QStringLiteral("devices"))))
        for (auto it = sources.begin(); it != sources.end(); ++it)
            out.row({quote(device), it.key().toUtf8(),
                     boolText(Recorder::microphoneMissing(device, strings(it.value())))});
    writeFile(QDir(context.output).filePath(QStringLiteral("microphone-missing.txt")), out.data());

    Text peaks;
    const QString logs = QDir(context.corpus).filePath(QStringLiteral("recorder/volumedetect"));
    for (const QString &name : sortedEntries(logs, false)) {
        if (!name.endsWith(QLatin1String(".log"))) continue;
        // As daemonMain reads audio.mka.log.
        const double peak = Recorder::microphonePeakDb(QString::fromUtf8(readFile(QDir(logs).filePath(name))));
        peaks.row({name.toUtf8(), num(peak), hexBits(peak)});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("microphone-peak-db.txt")), peaks.data());
}

// The parsing of `pactl list short sources` inside Recorder::daemonMain.
QStringList pactlSources()
{
    // src/record/Recorder.cpp:803-807 at rust-migration-base, verbatim.
    QStringList sources;
    const QStringList lines = commandOutput(QStringLiteral("pactl"),
        {QStringLiteral("list"), QStringLiteral("short"), QStringLiteral("sources")})
            .split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    for (const QString &line : lines) sources << line.section(QLatin1Char('\t'), 1, 1);
    return sources;
}

void recordPactl(const Context &context)
{
    Text out;
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("pactl/list-short-sources"));
    const auto run = [&](const QByteArray &label, const Replay::Answer &answer) {
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-pactl")));
        replay.answer(QStringLiteral("pactl"), {QStringLiteral("list"), QStringLiteral("short"), QStringLiteral("sources")},
                      answer);
        replay.activate();
        out.row({label, quoteList(pactlSources())});
    };
    for (const QString &name : sortedEntries(directory, false)) {
        Replay::Answer answer;
        answer.out = readFile(QDir(directory).filePath(name));
        run(name.toUtf8(), answer);
    }
    Replay::Answer failing;
    failing.out = readFile(QDir(directory).filePath(QStringLiteral("crafted-dock.txt")));
    failing.status = 1;
    run("<crafted-dock.txt, exit status 1>", failing);
    writeFile(QDir(context.output).filePath(QStringLiteral("pactl-sources.txt")), out.data());
}

void recordDiscard(const Context &context)
{
    Text out;
    const QList<QByteArray> names{"abortCapture", "killAudio", "requestCameraStop", "waitForCameraStop",
                                  "removeBundle", "removeState"};
    for (int mask = 0; mask < 64; ++mask) {
        for (const bool removes : {true, false}) {
            if (!(mask & 16) && !removes) continue;
            QStringList calls;
            Recorder::DiscardActions actions;
            if (mask & 1) actions.abortCapture = [&] { calls << QStringLiteral("abortCapture"); };
            if (mask & 2) actions.killAudio = [&] { calls << QStringLiteral("killAudio"); };
            if (mask & 4) actions.requestCameraStop = [&] { calls << QStringLiteral("requestCameraStop"); };
            if (mask & 8) actions.waitForCameraStop = [&](int timeout) {
                calls << QStringLiteral("waitForCameraStop(%1)").arg(timeout);
            };
            if (mask & 16) actions.removeBundle = [&, removes] { calls << QStringLiteral("removeBundle"); return removes; };
            if (mask & 32) actions.removeState = [&] { calls << QStringLiteral("removeState"); };
            QList<QByteArray> present;
            for (int bit = 0; bit < 6; ++bit)
                if (mask & (1 << bit)) present << names[bit];
            const bool result = Recorder::discardTimeline(actions);
            out.row({present.isEmpty() ? QByteArray("-") : present.join(','),
                     (mask & 16) ? boolText(removes) : QByteArray("-"), quoteList(calls), boolText(result)});
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("discard-timeline.txt")), out.data());
}

void recordTimestamps(const Context &context)
{
    Text out;
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("recorder/timestamps"));
    for (const QString &name : sortedEntries(directory, false))
        out.row({"firstFrameTimestamp", name.toUtf8(), QByteArray::number(firstFrameTimestamp(QDir(directory).filePath(name)))});
    out.row({"firstFrameTimestamp", "<missing file>",
             QByteArray::number(firstFrameTimestamp(QDir(directory).filePath(QStringLiteral("missing.ts"))))});
    out.row({"firstFrameTimestamp", "<directory>", QByteArray::number(firstFrameTimestamp(directory))});
    out.row({"firstFrameTimestamp", "<empty path>", QByteArray::number(firstFrameTimestamp(QString()))});

    const QString files = QDir(context.scratch).filePath(QStringLiteral("timestamps"));
    QDir().mkpath(files);
    QDir().mkpath(QDir(files).filePath(QStringLiteral("directory.ts")));
    const auto write = [&](const QByteArray &label, const QString &path, qint64 monotonic) {
        const qint64 before = realtimeNowUs();
        const bool ok = writeStartTimestamp(path, monotonic);
        const qint64 after = realtimeNowUs();
        bool readable = false;
        const QByteArray bytes = readFile(path, &readable);
        out.row({"writeStartTimestamp", label, QByteArray::number(monotonic), boolText(ok),
                 readable && QFileInfo(path).isFile() ? quoteBytes(maskRealtime(bytes, before, after))
                                                      : QByteArray("-"),
                 octalMode(path), QByteArray::number(firstFrameTimestamp(path))});
    };
    write("new file", QDir(files).filePath(QStringLiteral("audio.mka.ts")), 123456789);
    write("replaced", QDir(files).filePath(QStringLiteral("audio.mka.ts")), 5);
    write("zero", QDir(files).filePath(QStringLiteral("zero.ts")), 0);
    write("negative", QDir(files).filePath(QStringLiteral("negative.ts")), -5);
    write("maximum", QDir(files).filePath(QStringLiteral("maximum.ts")), std::numeric_limits<qint64>::max());
    write("missing directory", QDir(files).filePath(QStringLiteral("missing/audio.mka.ts")), 1);
    write("path is a directory", QDir(files).filePath(QStringLiteral("directory.ts")), 1);
    writeFile(QDir(context.output).filePath(QStringLiteral("timestamps.txt")), out.data());
}

DeviceEventKind deviceKind(const QString &name)
{
    if (name == QLatin1String("ButtonDown")) return DeviceEventKind::ButtonDown;
    if (name == QLatin1String("ButtonUp")) return DeviceEventKind::ButtonUp;
    if (name == QLatin1String("Scroll")) return DeviceEventKind::Scroll;
    if (name == QLatin1String("KeyDown")) return DeviceEventKind::KeyDown;
    if (name == QLatin1String("KeyUp")) return DeviceEventKind::KeyUp;
    fail(QStringLiteral("unknown device event kind %1").arg(name));
    return DeviceEventKind::KeyDown;
}

CaptureRegion regionFromJson(const QJsonObject &object)
{
    CaptureRegion region;
    region.x = object.value(QStringLiteral("x")).toDouble();
    region.y = object.value(QStringLiteral("y")).toDouble();
    region.scale = object.value(QStringLiteral("scale")).toDouble();
    return region;
}

QVector<RawCursorSample> samplesFromJson(const QJsonValue &value)
{
    QVector<RawCursorSample> samples;
    for (const QJsonValue &item : value.toArray()) {
        const QJsonArray triple = item.toArray();
        samples << RawCursorSample{triple.at(0).toInteger(), QPointF(triple.at(1).toDouble(), triple.at(2).toDouble())};
    }
    return samples;
}

void recordInputLog(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject inputs = cases.value(QStringLiteral("inputLog")).toObject();
    const QString files = QDir(context.scratch).filePath(QStringLiteral("input-log"));
    QDir().mkpath(files);
    const auto dump = [&](const QString &path, bool ok, const QString &error) {
        out.row({"ok", boolText(ok), quote(sanitize(error))});
        bool readable = false;
        const QByteArray bytes = readFile(path, &readable);
        if (!readable) { out.row({"file", "-"}); return; }
        out.row({"file", QByteArray::number(bytes.size()), octalMode(path)});
        const QList<QByteArray> lines = bytes.split('\n');
        for (qsizetype i = 0; i < lines.size(); ++i) {
            if (i + 1 == lines.size() && lines[i].isEmpty()) break;
            out.row({"line", lines[i]});
        }
    };
    for (auto it = inputs.begin(); it != inputs.end(); ++it) {
        const QJsonObject input = it.value().toObject();
        QVector<DeviceEvent> devices;
        for (const QJsonValue &item : input.value(QStringLiteral("devices")).toArray()) {
            const QJsonObject object = item.toObject();
            DeviceEvent event;
            event.monotonicUs = object.value(QStringLiteral("t")).toInteger();
            event.kind = deviceKind(object.value(QStringLiteral("kind")).toString());
            event.button = object.value(QStringLiteral("button")).toString();
            event.dx = object.value(QStringLiteral("dx")).toInt();
            event.dy = object.value(QStringLiteral("dy")).toInt();
            event.keyCode = object.value(QStringLiteral("code")).toInt();
            event.keyName = object.value(QStringLiteral("name")).toString();
            event.modifiers = strings(object.value(QStringLiteral("mods")));
            devices << event;
        }
        const QString path = QDir(files).filePath(it.key() + QStringLiteral(".jsonl"));
        QString error = QStringLiteral("<untouched>");
        const bool ok = writeInputLog(path, samplesFromJson(input.value(QStringLiteral("cursor"))), devices,
                                      regionFromJson(input.value(QStringLiteral("region")).toObject()), &error);
        out << "== " << it.key().toUtf8() << "\n";
        dump(path, ok, error);
    }
    const QVector<RawCursorSample> one{{1, QPointF(1, 1)}};
    for (const auto &[label, path] : QList<QPair<QByteArray, QString>>{
             {"<missing directory>", QDir(files).filePath(QStringLiteral("missing/input.jsonl"))},
             {"<path is a directory>", files}}) {
        QString error = QStringLiteral("<untouched>");
        const bool ok = writeInputLog(path, one, {}, CaptureRegion{}, &error);
        out << "== " << label << "\n";
        dump(QFileInfo(path).isDir() ? QString() : path, ok, error);
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("input-log.txt")), out.data());
}

void recordWriteJson(const Context &context)
{
    Text out;
    const QString files = QDir(context.scratch).filePath(QStringLiteral("write-json"));
    QDir().mkpath(files);
    const QJsonObject recording = readJson(QDir(context.corpus).filePath(QStringLiteral("recorder/state/recording.json"))).toObject();
    const QList<QPair<QString, QJsonObject>> objects{
        {QStringLiteral("empty"), QJsonObject{}},
        {QStringLiteral("recording-json"), QJsonObject{{"overlay_exclusion", "plugin"}}},
        {QStringLiteral("state"), recording},
        {QStringLiteral("special-values"), QJsonObject{{"nan", qQNaN()}, {"inf", inf}, {"negative-zero", -0.0},
            {"big", qint64(9007199254740993)}, {"tiny", 5e-324}, {"text", "line\nbreak \"q\" \\ é\U0001F3AC"},
            {"nested", QJsonObject{{"a", QJsonArray{1, 2.5, QJsonValue(), true}}}}}},
    };
    for (const auto &[name, object] : objects) {
        const QString path = QDir(files).filePath(name + QStringLiteral(".json"));
        QString error = QStringLiteral("<untouched>");
        const bool ok = writeJson(path, object, &error);
        out.row({name.toUtf8(), boolText(ok), quote(sanitize(error)), octalMode(path)});
        writeFile(QDir(context.output).filePath(QStringLiteral("write-json/%1.json").arg(name)), readFile(path));
    }
    QFile existing(QDir(files).filePath(QStringLiteral("existing-0600.json")));
    writeFile(existing.fileName(), "old");
    ::chmod(QFile::encodeName(existing.fileName()).constData(), 0600);
    for (const auto &[label, path] : QList<QPair<QByteArray, QString>>{
             {"<existing file, mode 0600>", existing.fileName()},
             {"<missing directory>", QDir(files).filePath(QStringLiteral("missing/capture.json"))},
             {"<path is a directory>", files}, {"<empty path>", QString()}}) {
        QString error = QStringLiteral("<untouched>");
        const bool ok = writeJson(path, QJsonObject{{"version", 1}}, &error);
        bool readable = false;
        const QByteArray bytes = QFileInfo(path).isFile() ? readFile(path, &readable) : QByteArray();
        out.row({label, boolText(ok), quote(sanitize(error)), path.isEmpty() ? QByteArray("-") : octalMode(path),
                 readable ? quoteBytes(bytes) : QByteArray("-")});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("write-json.txt")), out.data());
}

void recordRecorderOutput(const Context &context)
{
    Text out;
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("recorder/output"));
    for (const QString &name : sortedEntries(directory, false))
        out.row({"lastNonEmptyLine", name.toUtf8(), quote(lastNonEmptyLine(readFile(QDir(directory).filePath(name))))});
    out.row({"lastNonEmptyLine", "<empty>", quote(lastNonEmptyLine(QByteArray()))});

    // Each chunk is what one `cat <file>` printed, read as daemonMain reads gpu-screen-recorder:
    // merged channels, drained into one tail.
    const QList<QPair<QByteArray, QStringList>> sequences{
        {"one chunk", {QStringLiteral("gsr-error.txt")}},
        {"big chunk", {QStringLiteral("big.txt")}},
        {"three chunks", {QStringLiteral("gsr-error.txt"), QStringLiteral("big.txt"), QStringLiteral("no-newline.txt")}},
        {"growing past the limit", {QStringLiteral("big.txt"), QStringLiteral("big.txt"), QStringLiteral("unicode.txt")}},
        {"empty chunks", {QStringLiteral("empty.txt"), QStringLiteral("crlf.txt"), QStringLiteral("empty.txt")}},
        {"bytes", {QStringLiteral("invalid-utf8.bin"), QStringLiteral("nul.bin"), QStringLiteral("carriage-return.txt")}},
    };
    const QString debugPath = QDir(context.scratch).filePath(QStringLiteral("omareel.log"));
    for (const auto &[label, chunks] : sequences) {
        for (const QByteArray &debug : {QByteArray("open"), QByteArray("closed"), QByteArray("none")}) {
            out << "== " << label << " debugLog=" << debug << "\n";
            QFile::remove(debugPath);
            QFile debugLog(debugPath);
            if (debug == "open" && !debugLog.open(QIODevice::WriteOnly | QIODevice::Append))
                fail(QStringLiteral("cannot open %1").arg(debugPath));
            QByteArray tail;
            for (const QString &chunk : chunks) {
                QProcess process;
                process.setProcessChannelMode(QProcess::MergedChannels);
                process.start(QStringLiteral("cat"), {QDir(directory).filePath(chunk)});
                if (!process.waitForFinished(10000)) fail(QStringLiteral("cat did not finish"));
                drainRecorderOutput(&process, &tail, debug == "none" ? nullptr : &debugLog);
                out.row({"drain", chunk.toUtf8(), QByteArray::number(tail.size()), quoteBytes(tail),
                         quote(lastNonEmptyLine(tail))});
            }
            debugLog.close();
            bool readable = false;
            const QByteArray logged = readFile(debugPath, &readable);
            out.row({"debugLog", readable ? quoteBytes(logged) : QByteArray("-")});
        }
    }
    QFile::remove(debugPath);
    writeFile(QDir(context.output).filePath(QStringLiteral("recorder-output.txt")), out.data());
}

void setMtime(const QString &path, qint64 seconds)
{
    const timespec times[2] = {{seconds, 0}, {seconds, 0}};
    ::utimensat(AT_FDCWD, QFile::encodeName(path).constData(), times, AT_SYMLINK_NOFOLLOW);
}

void recordStateFiles(const Context &context)
{
    Text out;
    const QString runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    {
        ScopedEnvironment environment;
        const auto paths = [&](const QByteArray &label) {
            out.row({"paths", label, quote(sanitize(Recorder::stateFilePath())),
                     quote(sanitize(Recorder::selfViewHostPath())), quote(sanitize(lastErrorFilePath()))});
        };
        paths("XDG_RUNTIME_DIR hermetic");
        environment.set("XDG_RUNTIME_DIR", QFile::encodeName(runtime) + "/");
        paths("XDG_RUNTIME_DIR with a trailing slash");
        environment.set("XDG_RUNTIME_DIR", "relative/run");
        paths("XDG_RUNTIME_DIR relative");
        environment.set("XDG_RUNTIME_DIR", "");
        paths("XDG_RUNTIME_DIR empty");
        environment.unset("XDG_RUNTIME_DIR");
        paths("XDG_RUNTIME_DIR unset");
    }

    const QString states = QDir(context.corpus).filePath(QStringLiteral("recorder/state"));
    for (const QString &name : sortedEntries(states, false))
        out.row({"readStateFile", name.toUtf8(), compact(Recorder::readStateFile(QDir(states).filePath(name)))});
    out.row({"readStateFile", "<missing>", compact(Recorder::readStateFile(QDir(states).filePath(QStringLiteral("missing.json"))))});
    out.row({"readStateFile", "<directory>", compact(Recorder::readStateFile(states))});

    // The running-recording queries over each state file as recording.json. Only states whose pid
    // is at most 1 are asked: a larger pid would be looked up in the machine's /proc.
    const QString stateFile = Recorder::stateFilePath();
    const QString stateDirectory = QFileInfo(stateFile).absolutePath();
    for (const QString &name : sortedEntries(states, false)) {
        const QJsonObject state = Recorder::readStateFile(QDir(states).filePath(name));
        if (state.value(QStringLiteral("pid")).toVariant().toLongLong() > 1) {
            out.row({"queries", name.toUtf8(), "skipped (pid > 1)"});
            continue;
        }
        const auto install = [&] {
            QDir(stateDirectory).removeRecursively();
            writeFile(stateFile, readFile(QDir(states).filePath(name)));
        };
        install();
        QList<QByteArray> cells{"queries", name.toUtf8(),
                                QByteArray::number(Recorder::recordingStartedUs()),
                                quote(Recorder::recordedMonitor()), boolText(Recorder::recordingHasWebcam()),
                                compact(Recorder::recordingState())};
        cells << boolText(Recorder::isRecording()) << boolText(QFileInfo::exists(stateFile));
        install();
        QString error = QStringLiteral("<untouched>");
        cells << boolText(Recorder::signalExisting(false, &error)) << quote(error) << boolText(QFileInfo::exists(stateFile));
        install();
        error = QStringLiteral("<untouched>");
        QString bundle = QStringLiteral("<untouched>");
        cells << QByteArray::number(Recorder::stopExisting(true, &bundle, &error)) << quote(bundle) << quote(error)
              << boolText(QFileInfo::exists(stateFile));
        out.row(cells);
    }
    QDir(stateDirectory).removeRecursively();
    out.row({"queries", "<no state file>", QByteArray::number(Recorder::recordingStartedUs()),
             quote(Recorder::recordedMonitor()), boolText(Recorder::recordingHasWebcam()),
             compact(Recorder::recordingState()), boolText(Recorder::isRecording()),
             boolText(QFileInfo::exists(stateFile))});

    // updateStateFile, one step after another on the same files.
    const auto show = [&](const QString &path) {
        bool readable = false;
        const QByteArray bytes = QFileInfo(path).isFile() ? readFile(path, &readable) : QByteArray();
        out.row({"  file", readable ? quoteBytes(bytes) : QByteArray("-"), octalMode(path)});
        out.row({"  directory", directoryListing(QFileInfo(path).absolutePath())});
    };
    const auto update = [&](const QByteArray &label, const QString &path, const QJsonObject &values) {
        QString error = QStringLiteral("<untouched>");
        const bool ok = Recorder::updateStateFile(path, values, &error);
        out.row({"update", label, quote(sanitize(path)), compact(values), boolText(ok), quote(sanitize(error))});
        show(path);
    };
    const QString scratch = QDir(context.scratch).filePath(QStringLiteral("state"));
    const QString file = QDir(scratch).filePath(QStringLiteral("deep/omareel/recording.json"));
    update("creates the directories", file, {{"pid", 0}, {"monitor", "DP-1"}});
    update("first command", file, {{"command", "rotate"}});
    update("second command", file, {{"command", "rotate"}});
    update("explicit sequence", file, {{"command", "flip"}, {"command_sequence", 7}});
    update("after an explicit sequence", file, {{"command", "corner"}});
    update("no command", file, {{"camera_status", "ready"}, {"selfview_x", 16.5}});
    update("empty command", file, {{"command", ""}});
    update("null command", file, {{"command", QJsonValue()}});
    update("sequence only", file, {{"command_sequence", -3}});
    update("command after a negative sequence", file, {{"command", "show"}});
    update("nested values", file, {{"extra", QJsonObject{{"list", QJsonArray{1, "two", QJsonValue()}}}},
                                   {"monitor", QJsonValue()}});
    update("nothing", file, {});
    for (const QString &name : {QStringLiteral("strings.json"), QStringLiteral("doubles.json"), QStringLiteral("huge.json"),
                                QStringLiteral("nulls.json"), QStringLiteral("nested.json"), QStringLiteral("array.json"),
                                QStringLiteral("invalid.json"), QStringLiteral("empty.json"), QStringLiteral("bom.json")}) {
        writeFile(file, readFile(QDir(states).filePath(name)));
        update("command over " + name.toUtf8(), file, {{"command", "x"}});
    }
    writeFile(file, "{\"command_sequence\": 1}");
    ::chmod(QFile::encodeName(file).constData(), 0600);
    update("keeps mode 0600", file, {{"command", "y"}});
    {
        QLockFile held(file + QStringLiteral(".lock"));
        if (!held.tryLock(0)) fail(QStringLiteral("could not take the lock"));
        update("lock held by this process (waits a second)", file, {{"command", "busy"}});
    }
    out.row({"after unlock", directoryListing(QFileInfo(file).absolutePath())});
    writeFile(file + QStringLiteral(".lock"), "999999999\nomareel\n\n");
    update("stale lock of a dead process", file, {{"command", "stale"}});
    writeFile(file + QStringLiteral(".lock"), "garbage");
    update("garbage lock file (waits a second)", file, {{"command", "garbage"}});
    setMtime(file + QStringLiteral(".lock"), 1000000000);
    update("garbage lock file, old", file, {{"command", "old garbage"}});
    QDir().mkpath(file + QStringLiteral(".lock"));
    update("lock path is a directory (waits a second)", file, {{"command", "directory"}});
    QDir(file + QStringLiteral(".lock")).removeRecursively();
    QFile::remove(file);
    QDir().mkpath(file);
    update("state path is a directory", file, {{"command", "x"}});
    QDir(file).removeRecursively();
    const QString readOnly = QDir(scratch).filePath(QStringLiteral("read-only"));
    QDir().mkpath(readOnly);
    ::chmod(QFile::encodeName(readOnly).constData(), 0500);
    update("read-only directory", QDir(readOnly).filePath(QStringLiteral("recording.json")), {{"command", "x"}});
    ::chmod(QFile::encodeName(readOnly).constData(), 0700);
    {
        QString error = QStringLiteral("<untouched>");
        const bool ok = Recorder::updateStateFile(QDir(scratch).filePath(QStringLiteral("no-error.json")),
                                                  {{"command", "x"}});
        out.row({"update", "without an error pointer", boolText(ok), quote(error)});
    }

    // The fixed paths: sendHostCommand and updateRecordingState under XDG_RUNTIME_DIR.
    QDir(stateDirectory).removeRecursively();
    for (const QString &command : {QStringLiteral("quit"), QStringLiteral("configure"), QStringLiteral("")}) {
        const bool ok = Recorder::sendHostCommand(command);
        out.row({"sendHostCommand", quote(command), boolText(ok)});
        show(Recorder::selfViewHostPath());
    }
    QString error = QStringLiteral("<untouched>");
    const bool ok = Recorder::updateRecordingState({{"capture_started", true}, {"command", "rotate"}}, &error);
    out.row({"updateRecordingState", boolText(ok), quote(error)});
    show(stateFile);

    // last-error.txt.
    out.row({"readLastError", "<missing>", quote(readLastError())});
    for (const QString &detail : {QStringLiteral("Overlays could not leave the recorded output"),
                                  QStringLiteral("two\nlines\n"), QStringLiteral("  padded  "), QString()}) {
        writeLastError(detail);
        out.row({"writeLastError", quote(detail), quoteBytes(readFile(lastErrorFilePath())), quote(readLastError())});
    }
    QDir(stateDirectory).removeRecursively();

    // What a QLockFile leaves while it is held: Qt's format, which the Rust side has to match
    // for the C++ processes that still lock these files. The pid, the application name, the host
    // name and the machine and boot ids are facts of the run and shown as placeholders.
    const QString lockPath = QDir(scratch).filePath(QStringLiteral("format.lock"));
    {
        QLockFile lock(lockPath);
        lock.setStaleLockTime(10000);
        if (!lock.tryLock(1000)) fail(QStringLiteral("could not lock %1").arg(lockPath));
        QByteArray bytes = readFile(lockPath);
        QList<QByteArray> lines = bytes.split('\n');
        if (lines.size() >= 3) {
            if (lines[0] == QByteArray::number(QCoreApplication::applicationPid())) lines[0] = "{pid}";
            if (lines[1] == QCoreApplication::applicationName().toUtf8()) lines[1] = "{applicationName}";
            if (lines[2] == QSysInfo::machineHostName().toUtf8()) lines[2] = "{hostName}";
        }
        if (lines.size() >= 5) {
            if (lines[3] == QSysInfo::machineUniqueId()) lines[3] = "{machineUniqueId}";
            if (lines[4] == QSysInfo::bootUniqueId()) lines[4] = "{bootUniqueId}";
        }
        out.row({"QLockFile", quoteBytes(lines.join('\n')), octalMode(lockPath)});
    }
    out.row({"QLockFile after unlock", boolText(QFileInfo::exists(lockPath))});
    writeFile(QDir(context.output).filePath(QStringLiteral("state-files.txt")), out.data());
}

void recordMonitorLayouts(const Context &context)
{
    Text out;
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("hyprctl/monitors"));
    const auto dump = [&](const QByteArray &label, const Replay::Answer &answer, bool hyprctl = true) {
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-monitors")));
        replay.answer(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("monitors")}, answer);
        replay.activate();
        ScopedEnvironment environment;
        if (!hyprctl) environment.set("PATH", QFile::encodeName(shimDirectory(context.scratch, QStringLiteral("empty"), {})));
        out << "== " << label << "\n";
        for (const MonitorLayout &layout : monitorLayouts())
            out.row({quote(layout.name), num(layout.geometry.x()), num(layout.geometry.y()),
                     num(layout.geometry.width()), num(layout.geometry.height()), num(layout.reserved.left()),
                     num(layout.reserved.top()), num(layout.reserved.right()), num(layout.reserved.bottom())});
    };
    for (const QString &name : sortedEntries(directory, false)) {
        Replay::Answer answer;
        answer.out = readFile(QDir(directory).filePath(name));
        dump(name.toUtf8(), answer);
    }
    Replay::Answer failing;
    failing.out = readFile(QDir(directory).filePath(QStringLiteral("crafted-two.json")));
    failing.status = 1;
    dump("<crafted-two.json, exit status 1>", failing);
    dump("<hyprctl not on PATH>", Replay::Answer{}, false);
    writeFile(QDir(context.output).filePath(QStringLiteral("monitor-layouts.txt")), out.data());
}

void recordSelfViewPlacement(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject placements = cases.value(QStringLiteral("selfViewPlacement")).toObject();
    const auto place = [&](const QByteArray &label, const QJsonObject &input, double x, double y) {
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-placement")));
        Replay::Answer answer;
        if (input.value(QStringLiteral("monitors")).isString())
            answer.out = readFile(QDir(context.corpus).filePath(input.value(QStringLiteral("monitors")).toString()));
        else
            answer.status = 1;
        replay.answer(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("monitors")}, answer);
        replay.activate();
        RecordOptions options;
        options.webcam = input.value(QStringLiteral("webcam")).toBool();
        options.selfView = input.value(QStringLiteral("selfView")).toBool();
        options.selfViewSize = input.value(QStringLiteral("size")).toString();
        CaptureRegion capture;
        capture.monitorName = input.value(QStringLiteral("monitor")).toString();
        RecordingPreferences preferences;
        preferences.selfViewX = x;
        preferences.selfViewY = y;
        const SelfViewPlacement placement = selfViewPlacement(options, capture, preferences);
        out.row({label, boolText(options.webcam), boolText(options.selfView), quote(options.selfViewSize),
                 quote(capture.monitorName), num(x), num(y), boolText(placement.visible), boolText(placement.safe),
                 quote(placement.monitor), quote(placement.sizeName), QByteArray::number(placement.pixels),
                 QByteArray::number(placement.position.x()), QByteArray::number(placement.position.y()),
                 QByteArray::number(replay.calls().size())});
    };
    for (auto it = placements.begin(); it != placements.end(); ++it) {
        const QJsonObject input = it.value().toObject();
        place(it.key().toUtf8(), input, input.value(QStringLiteral("x")).toDouble(), input.value(QStringLiteral("y")).toDouble());
    }
    for (const QString &size : {QStringLiteral("S"), QStringLiteral("M"), QStringLiteral("L"), QStringLiteral("s"),
                                QStringLiteral("XL"), QString()})
        out.row({"selfViewPixels", quote(size), QByteArray::number(selfViewPixels(size))});
    writeFile(QDir(context.output).filePath(QStringLiteral("self-view-placement.txt")), out.data());
}

void recordHelpers(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject valueAfterCases = cases.value(QStringLiteral("valueAfter")).toObject();
    for (const QJsonValue &arguments : valueAfterCases.value(QStringLiteral("arguments")).toArray())
        for (const QString &name : strings(valueAfterCases.value(QStringLiteral("names"))))
            out.row({"valueAfter", quoteList(strings(arguments)), quote(name), quote(valueAfter(strings(arguments), name))});
    for (const CaptureMode mode : {CaptureMode::Fullscreen, CaptureMode::Region, CaptureMode::Window})
        out.row({"modeName", modeText(mode), quote(modeName(mode))});
    out.row({"modeName", "Mode7", quote(modeName(CaptureMode(7)))});
    const QJsonObject position = cases.value(QStringLiteral("positionObject")).toObject();
    for (const QJsonValue &region : position.value(QStringLiteral("regions")).toArray())
        for (const QJsonValue &point : position.value(QStringLiteral("points")).toArray()) {
            const CaptureRegion capture = regionFromJson(region.toObject());
            const QPointF logical(point.toArray().at(0).toDouble(), point.toArray().at(1).toDouble());
            out.row({"positionObject", num(logical.x()), num(logical.y()), num(capture.x), num(capture.y),
                     num(capture.scale), compact(positionObject(logical, capture))});
        }
    const QJsonObject at = cases.value(QStringLiteral("cursorAt")).toObject();
    const QJsonObject sets = at.value(QStringLiteral("samples")).toObject();
    for (auto it = sets.begin(); it != sets.end(); ++it) {
        const QVector<RawCursorSample> samples = samplesFromJson(it.value());
        for (const QJsonValue &time : at.value(QStringLiteral("times")).toArray()) {
            const QPointF point = cursorAt(samples, time.toInteger());
            out.row({"cursorAt", it.key().toUtf8(), QByteArray::number(time.toInteger()), num(point.x()), num(point.y())});
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("helpers.txt")), out.data());
}

void recordAudioCapture(const Context &context)
{
    Text out;
    const QString files = QDir(context.scratch).filePath(QStringLiteral("audio"));
    struct Start {
        QByteArray label;
        bool desktop = false;
        bool microphone = false;
        QString device = QStringLiteral("default_input");
        int sinkStatus = -1; // -1: no answer
        QByteArray sink;
        int sourceStatus = -1;
        QByteArray source;
        bool ffmpeg = true;
        QString path;
    };
    const QString audioPath = QDir(files).filePath(QStringLiteral("bundle/audio.mka"));
    const QList<Start> starts{
        {"desktop", true, false, QStringLiteral("default_input"), 0, "alsa_output.pci-0000_00_1f.3.analog-stereo\n"},
        {"microphone", false, true, QStringLiteral("alsa_input.usb-Blue_Yeti-00.analog-stereo")},
        {"default microphone", false, true, QStringLiteral("default_input"), -1, {}, 0, "  alsa_input.default \n"},
        {"both", true, true, QStringLiteral("alsa_input.usb-Blue_Yeti-00.analog-stereo"), 0, "sink"},
        {"both defaults", true, true, QStringLiteral("default_input"), 0, "sink", 0, "source"},
        {"no default sink", true, false, QStringLiteral("default_input"), 1, "sink"},
        {"empty default source", false, true, QStringLiteral("default_input"), -1, {}, 0, "\n"},
        {"sink fails, microphone left", true, true, QStringLiteral("mic"), 1, {}},
        {"source fails, desktop left", true, true, QStringLiteral("default_input"), 0, "sink", 1, "source"},
        {"nothing requested", false, false},
        {"empty device", false, true, QString()},
        {"ffmpeg not on PATH", true, false, QStringLiteral("default_input"), 0, "sink", -1, {}, false},
        {"log file cannot be created", true, false, QStringLiteral("default_input"), 0, "sink", -1, {}, true,
         QDir(files).filePath(QStringLiteral("missing/audio.mka"))},
    };
    for (const Start &start : starts) {
        QDir(files).removeRecursively();
        QDir().mkpath(QDir(files).filePath(QStringLiteral("bundle")));
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-audio")));
        if (start.sinkStatus >= 0) {
            Replay::Answer answer; answer.status = start.sinkStatus; answer.out = start.sink;
            replay.answer(QStringLiteral("pactl"), {QStringLiteral("get-default-sink")}, answer);
        }
        if (start.sourceStatus >= 0) {
            Replay::Answer answer; answer.status = start.sourceStatus; answer.out = start.source;
            replay.answer(QStringLiteral("pactl"), {QStringLiteral("get-default-source")}, answer);
        }
        Replay::Answer ffmpeg; ffmpeg.out = "ffmpeg shim log\n"; ffmpeg.err = "on stderr\n";
        replay.answerAny(QStringLiteral("ffmpeg"), ffmpeg);
        replay.activate();
        ScopedEnvironment environment;
        if (!start.ffmpeg)
            environment.set("PATH", QFile::encodeName(shimDirectory(context.scratch, QStringLiteral("pactl"), {QStringLiteral("pactl")})));
        RecordOptions options;
        options.desktopAudio = start.desktop;
        options.microphoneAudio = start.microphone;
        options.microphoneDevice = start.device;
        const QString path = start.path.isEmpty() ? audioPath : start.path;
        QProcess process;
        qint64 startedUs = -7;
        const qint64 before = realtimeNowUs();
        const bool started = startAudioCapture(&process, options, path, &startedUs);
        process.waitForFinished(10000);
        const qint64 after = realtimeNowUs();
        out << "== start " << start.label << "\n";
        out.row({"result", boolText(started), startedUs == -7 ? QByteArray("untouched")
                                              : startedUs > 0 ? QByteArray("positive") : QByteArray::number(startedUs)});
        replay.dumpCalls(out);
        bool readable = false;
        const QByteArray log = readFile(path + QStringLiteral(".log"), &readable);
        out.row({"log", readable ? quoteBytes(log) : QByteArray("-")});
        // The monotonic column is startedUs, a reading of the machine's clock.
        QByteArray ts = readFile(path + QStringLiteral(".ts"), &readable);
        ts.replace("\n" + QByteArray::number(startedUs) + '\t', "\n{startedUs}\t");
        out.row({"ts", readable ? quoteBytes(maskRealtime(ts, before, after)) : QByteArray("-")});
        out.row({"bundle", directoryListing(QFileInfo(path).absolutePath())});
    }

    struct Finish {
        QByteArray label;
        QByteArray audio; // written to the audio path unless null
        bool audioExists = true;
        int muxStatus = 0;
        bool creates = true;
        qint64 screenStartedUs = 1000000;
        qint64 audioStartedUs = 1250000;
        qint64 screenDurationUs = 2000000;
        bool running = false;
    };
    const QList<Finish> finishes{
        {"muxes", "audio bytes"},
        {"audio empty", ""},
        {"audio missing", {}, false},
        {"mux fails", "audio", true, 1},
        {"mux writes nothing", "audio", true, 0, false},
        {"no screen duration", "audio", true, 0, true, 1000000, 1250000, 0},
        {"audio started first", "audio", true, 0, true, 2000000, 1999999, 1},
        {"long recording", "audio", true, 0, true, 0, 3600000000LL, 7200000000LL},
        {"negative duration", "audio", true, 0, true, 5, 3, -1},
        {"stops a running recorder", "audio", true, 0, true, 1000000, 1000000, 500000, true},
    };
    for (const Finish &finish : finishes) {
        QDir(files).removeRecursively();
        QDir().mkpath(files);
        const QString screen = QDir(files).filePath(QStringLiteral("screen-video.mp4"));
        const QString audio = QDir(files).filePath(QStringLiteral("audio.mka"));
        const QString output = QDir(files).filePath(QStringLiteral("screen.mp4"));
        writeFile(screen, "screen");
        if (finish.audioExists) writeFile(audio, finish.audio);
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay-mux")));
        Replay::Answer mux;
        mux.status = finish.muxStatus;
        mux.hasCreates = finish.creates;
        mux.creates = "muxed";
        replay.answerAny(QStringLiteral("ffmpeg"), mux);
        replay.activate();
        QProcess recorder;
        if (finish.running) {
            recorder.start(QStringLiteral("sleep"), {QStringLiteral("30")});
            if (!recorder.waitForStarted(5000)) fail(QStringLiteral("sleep did not start"));
        }
        const bool ok = Recorder::finishAudioCapture(&recorder, screen, audio, output, finish.screenStartedUs,
                                                     finish.audioStartedUs, finish.screenDurationUs);
        out << "== finish " << finish.label << "\n";
        out.row({"arguments", QByteArray::number(finish.screenStartedUs), QByteArray::number(finish.audioStartedUs),
                 QByteArray::number(finish.screenDurationUs)});
        out.row({"result", boolText(ok), finish.running ? (recorder.state() == QProcess::NotRunning ? "recorder stopped" : "recorder running")
                                                        : "recorder not started"});
        replay.dumpCalls(out);
        bool readable = false;
        const QByteArray bytes = readFile(output, &readable);
        out.row({"output", readable ? quoteBytes(bytes) : QByteArray("-")});
        if (recorder.state() != QProcess::NotRunning) { recorder.kill(); recorder.waitForFinished(1000); }
    }
    QDir(files).removeRecursively();
    writeFile(QDir(context.output).filePath(QStringLiteral("audio-capture.txt")), out.data());
}

} // namespace

void runRecorder(const Context &context)
{
    const QJsonObject cases = corpusObject(context, QStringLiteral("recorder/cases.json"));
    recordClassify(context);
    recordEvenRegion(context);
    recordCameraBlock(context);
    recordMicrophone(context, cases);
    recordPactl(context);
    recordDiscard(context);
    recordTimestamps(context);
    recordInputLog(context, cases);
    recordWriteJson(context);
    recordRecorderOutput(context);
    recordStateFiles(context);
    recordMonitorLayouts(context);
    recordSelfViewPlacement(context, cases);
    recordHelpers(context, cases);
    recordAudioCapture(context);
}

} // namespace Oracle
