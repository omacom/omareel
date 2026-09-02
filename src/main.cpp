#include "core/InputLog.h"
#include "core/ZoomTimeline.h"
#include "record/Recorder.h"
#include "render/Exporter.h"

#include <QGuiApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QQuickWindow>
#include <QStandardPaths>
#include <QTextStream>

using namespace OmaRecord;

static int usage(const QString &error = {})
{
    QTextStream stream(error.isEmpty() ? stdout : stderr);
    if (!error.isEmpty()) stream << "omarecord: " << error << '\n';
    stream << "usage: omarecord record [--fullscreen|--region|--window] [options]\n"
              "       omarecord edit <bundle>\n"
              "       omarecord export <bundle> -o <file> [--fps N] [--width W]\n"
              "       omarecord probe <bundle>\n";
    return error.isEmpty() ? 0 : 2;
}

static int exportCommand(const QStringList &arguments)
{
    if (arguments.isEmpty()) return usage(QStringLiteral("export requires a bundle"));
    ExportOptions options;
    options.bundlePath = QDir(arguments.first()).absolutePath();
    for (int i = 1; i < arguments.size(); ++i) {
        const QString arg = arguments[i];
        if (arg == QLatin1String("-o") || arg == QLatin1String("--fps")
            || arg == QLatin1String("--width") || arg == QLatin1String("--quality")
            || arg == QLatin1String("--gif-fps") || arg == QLatin1String("--gif-width")) {
            if (++i >= arguments.size()) return usage(QStringLiteral("%1 requires a value").arg(arg));
            const QString value = arguments[i];
            if (arg == QLatin1String("-o")) options.outputPath = QFileInfo(value).absoluteFilePath();
            else if (arg == QLatin1String("--fps")) options.fps = value.toInt();
            else if (arg == QLatin1String("--width")) options.width = value.toInt();
            else if (arg == QLatin1String("--quality")) options.quality = value;
            else if (arg == QLatin1String("--gif-fps")) options.gifFps = value.toInt();
            else options.gifWidth = value.toInt();
        } else return usage(QStringLiteral("unknown export option %1").arg(arg));
    }
    if (options.outputPath.isEmpty()) return usage(QStringLiteral("export requires -o <file>"));
    if (!options.outputPath.endsWith(QStringLiteral(".mp4"), Qt::CaseInsensitive)
        && !options.outputPath.endsWith(QStringLiteral(".gif"), Qt::CaseInsensitive))
        return usage(QStringLiteral("export output must end in .mp4 or .gif"));
    if (options.fps < 0 || options.width < 0 || options.gifFps < 0 || options.gifWidth < 0)
        return usage(QStringLiteral("frame rate and size options must be positive"));
    const QStringList qualities{QStringLiteral("low"), QStringLiteral("medium"), QStringLiteral("high"),
        QStringLiteral("best"), QStringLiteral("web-low"), QStringLiteral("web-high"),
        QStringLiteral("social"), QStringLiteral("studio")};
    if (!options.quality.isEmpty() && !qualities.contains(options.quality))
        return usage(QStringLiteral("invalid quality %1").arg(options.quality));

    Exporter exporter;
    bool success = false;
    QString failure;
    QObject::connect(&exporter, &Exporter::progress, [](int frame, int total) {
        QTextStream(stderr) << '\r' << "Exporting " << frame << '/' << total << Qt::flush;
    });
    QObject::connect(&exporter, &Exporter::finished, [&](const QString &path) {
        success = true;
        QTextStream(stderr) << '\n';
        QTextStream(stdout) << path << '\n';
    });
    QObject::connect(&exporter, &Exporter::failed, [&](const QString &message) { failure = message; });
    exporter.exportBundle(options);
    if (!success) {
        QTextStream(stderr) << '\n' << "omarecord: " << failure << '\n';
        return 1;
    }
    return 0;
}

static int recordCommand(const QStringList &arguments)
{
    if (Recorder::isRecording()) {
        QString bundle, error;
        const int result = Recorder::stopExisting(arguments.contains(QStringLiteral("--stop")),
                                                   &bundle, &error);
        if (result == 0) QTextStream(stdout) << bundle << '\n';
        else QTextStream(stderr) << "omarecord: " << error << '\n';
        return result;
    }
    if (arguments.contains(QStringLiteral("--stop"))) {
        QTextStream(stderr) << "omarecord: no recording is active\n";
        return 1;
    }
    RecordOptions options;
    options.outputDirectory = QDir(QStandardPaths::writableLocation(QStandardPaths::MoviesLocation))
                                  .filePath(QStringLiteral("omarecord"));
    int modeCount = 0;
    for (int i = 0; i < arguments.size(); ++i) {
        const QString arg = arguments[i];
        if (arg == QLatin1String("--fullscreen")) { options.mode = CaptureMode::Fullscreen; ++modeCount; }
        else if (arg == QLatin1String("--region")) { options.mode = CaptureMode::Region; ++modeCount; }
        else if (arg == QLatin1String("--window")) { options.mode = CaptureMode::Window; ++modeCount; }
        else if (arg == QLatin1String("--with-desktop-audio")) options.desktopAudio = true;
        else if (arg == QLatin1String("--with-microphone-audio")) options.microphoneAudio = true;
        else if (arg == QLatin1String("--no-open")) options.noOpen = true;
        else if (arg == QLatin1String("--fps") || arg == QLatin1String("--dir")) {
            if (++i >= arguments.size()) return usage(QStringLiteral("%1 requires a value").arg(arg));
            if (arg == QLatin1String("--fps")) options.fps = arguments[i].toInt();
            else options.outputDirectory = QDir(arguments[i]).absolutePath();
        } else {
            return usage(QStringLiteral("unknown record option %1").arg(arg));
        }
    }
    if (modeCount > 1) return usage(QStringLiteral("choose only one capture mode"));
    if (options.fps <= 0 || options.fps > 240) return usage(QStringLiteral("fps must be between 1 and 240"));
    QString message;
    const int result = Recorder::startDetached(options, &message);
    QTextStream(result == 0 ? stdout : stderr) << message << '\n';
    return result;
}

static QJsonObject videoInfo(const QString &path)
{
    QProcess process;
    process.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-select_streams"), QStringLiteral("v:0"),
        QStringLiteral("-show_entries"), QStringLiteral("stream=width,height,r_frame_rate:format=duration"),
        QStringLiteral("-of"), QStringLiteral("json"), path});
    if (!process.waitForFinished(10000) || process.exitCode() != 0) return {};
    return QJsonDocument::fromJson(process.readAllStandardOutput()).object();
}

static double fraction(const QString &text)
{
    const auto parts = text.split('/');
    if (parts.size() == 2 && parts[1].toDouble() != 0.0) return parts[0].toDouble() / parts[1].toDouble();
    return text.toDouble();
}

static int probeCommand(const QString &bundle)
{
    const QFileInfo bundleInfo(bundle);
    if (!bundleInfo.isDir()) return usage(QStringLiteral("bundle does not exist: %1").arg(bundle));
    const auto info = videoInfo(QDir(bundle).filePath(QStringLiteral("screen.mp4")));
    const auto streams = info.value("streams").toArray();
    if (streams.isEmpty()) return usage(QStringLiteral("screen.mp4 could not be probed"));
    const auto stream = streams.first().toObject();
    const double duration = info.value("format").toObject().value("duration").toString().toDouble();
    QString error;
    const auto input = InputLog::loadBundle(bundle, duration, &error);
    if (!error.isEmpty()) return usage(error);
    QVector<double> clickTimes;
    for (const auto &click : input.clickDowns(true)) clickTimes << click.time;
    const auto zooms = ZoomTimeline::generate(clickTimes, duration);
    QJsonArray zoomJson;
    for (const auto &zoom : zooms) {
        zoomJson << QJsonObject{{"id", zoom.id}, {"start", zoom.start}, {"end", zoom.end},
                                {"level", zoom.level}, {"target", ZoomTimeline::targetToJson(zoom)}};
    }
    QJsonObject output{{"duration", duration},
                       {"fps", fraction(stream.value("r_frame_rate").toString())},
                       {"size", QJsonObject{{"width", stream.value("width").toInt()},
                                             {"height", stream.value("height").toInt()}}},
                       {"sample_count", input.moves().size()},
                       {"click_count", input.clickDowns().size()},
                       {"key_count", input.keyCount()},
                       {"generated_zoom_count", zooms.size()}, {"zooms", zoomJson}};
    QTextStream(stdout) << QJsonDocument(output).toJson(QJsonDocument::Indented);
    return 0;
}

int main(int argc, char **argv)
{
    const bool exporting = argc > 1 && QByteArray(argv[1]) == "export";
    if (exporting && qEnvironmentVariableIsEmpty("WAYLAND_DISPLAY")
        && qEnvironmentVariableIsEmpty("DISPLAY")) {
        if (qEnvironmentVariableIsEmpty("QT_QPA_PLATFORM")) qputenv("QT_QPA_PLATFORM", "offscreen");
        qputenv("QT_QPA_PLATFORMTHEME", QByteArray());
    }
    if (exporting) {
        const bool headless = qEnvironmentVariableIsEmpty("WAYLAND_DISPLAY")
                           && qEnvironmentVariableIsEmpty("DISPLAY");
        if (headless) QQuickWindow::setSceneGraphBackend(QStringLiteral("software"));
        else QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    }
    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("omarecord"));
    const QStringList args = app.arguments();
    if (args.size() < 2) {
        QTextStream(stderr) << "omarecord: launcher not implemented yet\n";
        return 2;
    }
    const QString command = args[1];
    if (command == QLatin1String("__record-daemon")) return Recorder::daemonMain(args.mid(2));
    if (command == QLatin1String("record")) return recordCommand(args.mid(2));
    if (command == QLatin1String("probe")) {
        if (args.size() != 3) return usage(QStringLiteral("probe requires a bundle"));
        return probeCommand(QDir(args[2]).absolutePath());
    }
    if (command == QLatin1String("export")) return exportCommand(args.mid(2));
    if (command == QLatin1String("edit")) {
        QTextStream(stderr) << "not implemented yet\n";
        return 2;
    }
    if (command == QLatin1String("--help") || command == QLatin1String("help")) return usage();
    return usage(QStringLiteral("unknown command %1").arg(command));
}
