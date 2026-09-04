#include "core/InputLog.h"
#include "core/ZoomTimeline.h"
#include "record/Recorder.h"
#include "render/Exporter.h"
#include "core/Theme.h"
#include "core/OmarchyPaths.h"
#include "core/RecordingPreferences.h"
#include "ui/Editor.h"
#include "ui/Launcher.h"
#include "ui/RecordingBar.h"

#include <LayerShellQt/shell.h>
#include <LayerShellQt/window.h>

#include <QGuiApplication>
#include <QFont>
#include <QFontDatabase>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QScreen>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQmlError>
#include <QStandardPaths>
#include <QTextStream>
#include <QTimer>
#include <QRegularExpression>
#include <QRegion>
#include <cerrno>
#include <cstdio>
#include <cstring>
#include <unistd.h>
#include <vector>

#ifndef OMAREEL_VERSION
#define OMAREEL_VERSION "unknown"
#endif

using namespace Omareel;

struct ResolvedFonts {
    QString ui;
    QString mono;
};

static QString availableFamily(const QStringList &families, const QStringList &candidates)
{
    for (const QString &candidate : candidates) {
        for (const QString &family : families)
            if (family.compare(candidate, Qt::CaseInsensitive) == 0) return family;
    }
    return {};
}

static ResolvedFonts resolveFonts()
{
    QProcess match;
    match.start(QStringLiteral("fc-match"),
                {QStringLiteral("-f"), QStringLiteral("%{family[0]}"), QStringLiteral("monospace")});
    QString family;
    if (match.waitForFinished(1500) && match.exitCode() == 0)
        family = QString::fromUtf8(match.readAllStandardOutput()).trimmed();
    if (family.isEmpty()) family = QFontDatabase::systemFont(QFontDatabase::FixedFont).family();
    qInfo().noquote() << "omareel: fontconfig monospace ->" << family;
    return {family, family};
}

static int shellFontBaseSize()
{
    QFile file(QDir(OmarchyPaths::stateRoot()).filePath(QStringLiteral("theme/shell.toml")));
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) return 12;
    bool inFont = false;
    const QRegularExpression section(QStringLiteral(R"(^\s*\[([^]]+)\])"));
    const QRegularExpression base(QStringLiteral(R"(^\s*base-size\s*=\s*(\d+))"));
    while (!file.atEnd()) {
        const QString line = QString::fromUtf8(file.readLine());
        const auto sectionMatch = section.match(line);
        if (sectionMatch.hasMatch()) { inFont = sectionMatch.captured(1) == QLatin1String("font"); continue; }
        const auto baseMatch = base.match(line);
        if (inFont && baseMatch.hasMatch()) return qMax(1, baseMatch.captured(1).toInt());
    }
    return 12;
}

static int usage(const QString &error = {})
{
    QTextStream stream(error.isEmpty() ? stdout : stderr);
    if (!error.isEmpty()) stream << "omareel: " << error << '\n';
    stream << "usage: omareel [command]\n\n"
              "commands:\n"
              "  record [--region|--fullscreen|--window] [options]\n"
              "      Toggle recording (smart gesture is the default: drag an area, click a\n"
              "      window, or click the desktop for the whole screen). Options: --fps N,\n"
              "      --dir PATH,\n"
              "      --with-desktop-audio, --with-microphone-audio, --no-audio,\n"
              "      --with-webcam, --webcam-device PATH, --webcam-height 720|1080,\n"
              "      --no-webcam, --no-selfview, --no-open, --no-bar,\n"
              "      --stop, --cancel.\n"
              "  edit <bundle.omareel>\n"
              "      Open a recording bundle in the editor.\n"
              "  export <bundle> -o <file.mp4|file.gif> [options]\n"
              "      Export with --fps N, --width W, --quality LEVEL, --gif-fps N,\n"
              "      --gif-width W, or --timing.\n"
              "  probe <bundle>\n"
              "      Print a JSON summary of a recording bundle.\n"
              "  help\n"
              "      Show this help.\n\n"
              "Running omareel without a command opens the launcher.\n"
              "The legacy omarecord command remains available as a compatibility alias.\n";
    return error.isEmpty() ? 0 : 2;
}

static void configureDebugScreenshot(QQmlApplicationEngine &engine, QGuiApplication &app)
{
    QString path = qEnvironmentVariable("OMAREEL_SCREENSHOT_LIVE");
    if (path.isEmpty()) path = qEnvironmentVariable("OMAREEL_SCREENSHOT");
    if (path.isEmpty() || engine.rootObjects().isEmpty()) return;
    const int captureDelay = qEnvironmentVariableIsEmpty("OMAREEL_SCREENSHOT_LIVE") ? 3000 : 30000;

    auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
    if (!window) return;

    const QString size = qEnvironmentVariable("OMAREEL_SCREENSHOT_SIZE");
    const QStringList dimensions = size.toLower().split(QLatin1Char('x'));
    bool widthOk = false;
    bool heightOk = false;
    const int width = dimensions.value(0).toInt(&widthOk);
    const int height = dimensions.value(1).toInt(&heightOk);
    if (dimensions.size() == 2 && widthOk && heightOk && width > 0 && height > 0) {
        if (qEnvironmentVariableIsEmpty("OMAREEL_SCREENSHOT_LIVE"))
            window->setMinimumSize(QSize(1, 1));
        window->resize(width, height);
    }

    static const QHash<QString, int> panels{
        {QStringLiteral("background"), 0}, {QStringLiteral("shape"), 1},
        {QStringLiteral("cursor"), 2}, {QStringLiteral("zoom"), 3},
        {QStringLiteral("clip"), 4}, {QStringLiteral("camera"), 5},
        {QStringLiteral("keystrokes"), 6}, {QStringLiteral("audio"), 7}
    };
    const QString panel = qEnvironmentVariable("OMAREEL_SCREENSHOT_PANEL").toLower();
    if (panels.contains(panel)) {
        if (QObject *sidePanel = window->findChild<QObject *>(QStringLiteral("sidePanel")))
            sidePanel->setProperty("section", panels.value(panel));
        if (QObject *editorObject = engine.rootContext()->contextProperty(QStringLiteral("editor")).value<QObject *>()) {
            if (panel == QLatin1String("zoom")) {
                const QVariantList zooms = editorObject->property("zooms").toList();
                if (!zooms.isEmpty())
                    editorObject->setProperty("selectedZoomId", zooms.first().toMap().value(QStringLiteral("id")));
            } else if (panel == QLatin1String("clip")) {
                const QVariantList clips = editorObject->property("clips").toList();
                if (!clips.isEmpty())
                    editorObject->setProperty("selectedClipId", clips.first().toMap().value(QStringLiteral("id")));
            }
        }
    }
    if (qEnvironmentVariable("OMAREEL_SCREENSHOT_PICK_ZOOM") == QLatin1String("1"))
        if (QObject *editor = engine.rootContext()->contextProperty(QStringLiteral("editor")).value<QObject *>())
            editor->setProperty("pickingZoomTarget", true);

    bool screenshotTimeOk = false;
    QString screenshotSeek = qEnvironmentVariable("OMAREEL_SCREENSHOT_SEEK");
    if (screenshotSeek.isEmpty()) screenshotSeek = qEnvironmentVariable("OMAREEL_SCREENSHOT_TIME");
    const double screenshotTime = screenshotSeek.toDouble(&screenshotTimeOk);
    if (screenshotTimeOk) {
        if (QObject *editor = engine.rootContext()->contextProperty(QStringLiteral("editor")).value<QObject *>()) {
            QTimer::singleShot(captureDelay - 400, editor, [editor, screenshotTime] {
                QMetaObject::invokeMethod(editor, "pause");
                QMetaObject::invokeMethod(editor, "seek", Q_ARG(double, screenshotTime));
            });
        }
    }

    const QJsonObject projectValues = QJsonDocument::fromJson(
        qEnvironmentVariable("OMAREEL_SCREENSHOT_PROJECT_VALUES").toUtf8()).object();
    if (!projectValues.isEmpty()) {
        if (QObject *editor = engine.rootContext()->contextProperty(QStringLiteral("editor")).value<QObject *>()) {
            for (auto it = projectValues.constBegin(); it != projectValues.constEnd(); ++it) {
                QMetaObject::invokeMethod(editor, "setProjectValue",
                                          Q_ARG(QString, it.key()),
                                          Q_ARG(QVariant, it.value().toVariant()),
                                          Q_ARG(bool, false));
            }
        }
    }

    const QString screenshotView = qEnvironmentVariable("OMAREEL_SCREENSHOT_VIEW");
    if (!screenshotView.isEmpty()) {
        QTimer::singleShot(250, window, [window, screenshotView] {
            QMetaObject::invokeMethod(window, "prepareScreenshot",
                                      Q_ARG(QVariant, QVariant(screenshotView)));
        });
    }

    QTimer::singleShot(captureDelay, &app, [&app, window, path] {
        QDir().mkpath(QFileInfo(path).absolutePath());
        const QImage image = window->grabWindow();
        const bool saved = !image.isNull() && image.save(path);
        if (!saved) QTextStream(stderr) << "omareel: could not save UI screenshot to " << path << '\n';
        app.exit(saved ? 0 : 2);
    });
}

static void reportQmlWarnings(QQmlApplicationEngine &engine)
{
    QObject::connect(&engine, &QQmlApplicationEngine::warnings, [](const QList<QQmlError> &warnings) {
        for (const QQmlError &warning : warnings)
            QTextStream(stderr) << warning.toString() << '\n';
    });
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
        } else if (arg == QLatin1String("--timing")) options.timing = true;
        else return usage(QStringLiteral("unknown export option %1").arg(arg));
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
        if (frame == total || frame == 1 || frame % 10 == 0)
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
        QTextStream(stderr) << '\n' << "omareel: " << failure << '\n';
        return 1;
    }
    return 0;
}

static int recordCommand(const QStringList &arguments)
{
    if (Recorder::isRecording()) {
        if (arguments.contains(QStringLiteral("--stop")) && arguments.contains(QStringLiteral("--cancel")))
            return usage(QStringLiteral("choose either --stop or --cancel"));
        const bool cancel = arguments.contains(QStringLiteral("--cancel"));
        QString bundle, error;
        const int result = Recorder::stopExisting(cancel, &bundle, &error);
        if (result == 0)
            QTextStream(stdout) << (cancel ? QStringLiteral("Recording discarded") : bundle) << '\n';
        else QTextStream(stderr) << "omareel: " << error << '\n';
        return result;
    }
    if (arguments.contains(QStringLiteral("--stop")) || arguments.contains(QStringLiteral("--cancel"))) {
        QTextStream(stderr) << "omareel: no recording is active\n";
        return 1;
    }
    if (arguments.contains(QStringLiteral("--with-webcam"))
        && arguments.contains(QStringLiteral("--no-webcam")))
        return usage(QStringLiteral("--with-webcam cannot be combined with --no-webcam"));
    RecordOptions options;
    options.outputDirectory = OmarchyPaths::recordingsDirectory();
    const RecordingPreferences preferences = RecordingPreferences::load();
    const bool explicitAudio = arguments.contains(QStringLiteral("--with-desktop-audio"))
        || arguments.contains(QStringLiteral("--with-microphone-audio"))
        || arguments.contains(QStringLiteral("--no-audio"));
    if (!explicitAudio) {
        options.desktopAudio = preferences.systemAudio;
        options.microphoneAudio = preferences.microphone;
    }
    options.microphoneDevice = preferences.microphoneDevice;
    const bool explicitWebcam = arguments.contains(QStringLiteral("--with-webcam"))
        || arguments.contains(QStringLiteral("--no-webcam"));
    if (!explicitWebcam) options.webcam = preferences.webcamEnabled;
    options.webcamDevice = preferences.webcamDevice;
    options.webcamHeight = preferences.webcamHeight;
    options.webcamRotation = preferences.webcamRotation;
    options.webcamFlipHorizontal = preferences.webcamFlipHorizontal;
    options.selfView = preferences.selfViewEnabled;
    options.selfViewSize = preferences.selfViewSize;
    options.captureBackend = preferences.captureBackend;
    int modeCount = 0;
    for (int i = 0; i < arguments.size(); ++i) {
        const QString arg = arguments[i];
        if (arg == QLatin1String("--fullscreen")) { options.mode = CaptureMode::Fullscreen; ++modeCount; }
        else if (arg == QLatin1String("--region")) { options.mode = CaptureMode::Region; ++modeCount; }
        else if (arg == QLatin1String("--window")) { options.mode = CaptureMode::Window; ++modeCount; }
        else if (arg == QLatin1String("--with-desktop-audio")) options.desktopAudio = true;
        else if (arg == QLatin1String("--with-microphone-audio")) options.microphoneAudio = true;
        else if (arg == QLatin1String("--with-webcam")) options.webcam = true;
        else if (arg == QLatin1String("--no-webcam")) options.webcam = false;
        else if (arg == QLatin1String("--no-selfview")) options.selfView = false;
        else if (arg.startsWith(QLatin1String("--webcam-device="))) {
            options.webcamDevice = arg.section(QLatin1Char('='), 1);
            options.webcam = true;
        }
        else if (arg == QLatin1String("--webcam-device")) {
            if (++i >= arguments.size()) return usage(QStringLiteral("--webcam-device requires a value"));
            options.webcamDevice = arguments[i];
            options.webcam = true;
        }
        else if (arg == QLatin1String("--webcam-height")) {
            if (++i >= arguments.size()) return usage(QStringLiteral("--webcam-height requires a value"));
            options.webcamHeight = arguments[i].toInt();
            options.webcam = true;
        }
        else if (arg == QLatin1String("--no-audio")) {
            options.desktopAudio = false;
            options.microphoneAudio = false;
        }
        else if (arg == QLatin1String("--microphone-device")) {
            if (++i >= arguments.size()) return usage(QStringLiteral("--microphone-device requires a value"));
            options.microphoneDevice = arguments[i];
        }
        else if (arg == QLatin1String("--no-open")) options.noOpen = true;
        else if (arg == QLatin1String("--no-bar")) options.noBar = true;
        else if (arg == QLatin1String("--fps") || arg == QLatin1String("--dir")) {
            if (++i >= arguments.size()) return usage(QStringLiteral("%1 requires a value").arg(arg));
            if (arg == QLatin1String("--fps")) options.fps = arguments[i].toInt();
            else options.outputDirectory = QDir(arguments[i]).absolutePath();
        } else {
            return usage(QStringLiteral("unknown record option %1").arg(arg));
        }
    }
    if (modeCount > 1) return usage(QStringLiteral("choose only one capture mode"));
    if (arguments.contains(QStringLiteral("--no-audio"))
        && (arguments.contains(QStringLiteral("--with-desktop-audio"))
            || arguments.contains(QStringLiteral("--with-microphone-audio"))))
        return usage(QStringLiteral("--no-audio cannot be combined with audio enable flags"));
    if (options.fps <= 0 || options.fps > 240) return usage(QStringLiteral("fps must be between 1 and 240"));
    if (options.webcam && !options.webcamDevice.startsWith(QLatin1String("/dev/video")))
        return usage(QStringLiteral("webcam device must be a /dev/video device"));
    if (options.webcamHeight != 720 && options.webcamHeight != 1080)
        return usage(QStringLiteral("webcam height must be 720 or 1080"));
    if (qEnvironmentVariable("OMAREEL_NO_BAR") == QLatin1String("1")) options.noBar = true;
    QString message;
    const int result = Recorder::startDetached(options, &message);
    QTextStream(result == 0 ? stdout : stderr) << message << '\n';
    return result;
}

static QScreen *recordBarScreen(const QString &recordedMonitor)
{
    for (QScreen *screen : QGuiApplication::screens()) {
        if (screen->name() == recordedMonitor) return screen;
    }
    return QGuiApplication::primaryScreen();
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
    // QProcess resolves executables to absolute paths, but Omarchy's REC indicator intentionally
    // matches an argv[0] beginning with "gpu-screen-recorder". Preserve that public contract.
    if (argc > 1 && std::strcmp(argv[1], "__record-gsr") == 0) {
        std::vector<char *> gsrArguments;
        gsrArguments.reserve(size_t(argc));
        gsrArguments.push_back(const_cast<char *>("gpu-screen-recorder"));
        for (int i = 2; i < argc; ++i) gsrArguments.push_back(argv[i]);
        gsrArguments.push_back(nullptr);
        ::execvp("gpu-screen-recorder", gsrArguments.data());
        std::fprintf(stderr, "omareel: could not exec gpu-screen-recorder: %s\n", std::strerror(errno));
        return 127;
    }
    const bool exporting = argc > 1 && QByteArray(argv[1]) == "export";
    const bool recordBar = argc > 1 && QByteArray(argv[1]) == "__record-bar";
    const bool hiddenRecordBar = recordBar && argc > 2 && QByteArray(argv[2]) == "--hidden";
    const bool graphical = argc == 1 || recordBar || (argc > 1 && QByteArray(argv[1]) == "edit");
    if (recordBar) LayerShellQt::Shell::useLayerShell();
    if (graphical) {
        QQuickStyle::setStyle(QStringLiteral("Basic"));
    }
    // Screenshot mode must be independent of compositor capture and GPU backend quirks.
    // It still exercises the real QML window and QQuickWindow::grabWindow().
    if (!qEnvironmentVariableIsEmpty("OMAREEL_SCREENSHOT")) {
        qputenv("QT_QPA_PLATFORM", "offscreen");
        qputenv("QT_QPA_PLATFORMTHEME", QByteArray());
    }
    if (qEnvironmentVariableIsEmpty("WAYLAND_DISPLAY")
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
    if (graphical) {
        if (qEnvironmentVariable("QT_QPA_PLATFORM") == QLatin1String("offscreen"))
            QQuickWindow::setSceneGraphBackend(QStringLiteral("software"));
        else QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    }
    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("omareel"));
    app.setOrganizationName(QStringLiteral("omareel"));
    app.setApplicationVersion(QStringLiteral(OMAREEL_VERSION));
    const ResolvedFonts resolvedFonts = resolveFonts();
    QFont applicationFont(resolvedFonts.ui);
    applicationFont.setPixelSize(shellFontBaseSize());
    QGuiApplication::setFont(applicationFont);
    Theme::setFontFamilies(resolvedFonts.ui, resolvedFonts.mono);
    if (graphical)
        QTextStream(stderr) << "omareel: application font " << resolvedFonts.ui
                            << " at " << applicationFont.pixelSize() << " px\n";
    const QStringList args = app.arguments();
    if (args.size() < 2) {
        app.setDesktopFileName(QStringLiteral("omareel"));
        Theme theme;
        Launcher launcher;
        QQmlApplicationEngine engine;
        reportQmlWarnings(engine);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        engine.rootContext()->setContextProperty(QStringLiteral("launcher"), &launcher);
        QObject::connect(&launcher, &Launcher::quitRequested, &app, &QCoreApplication::quit);
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/Omareel/Launcher.qml")));
        if (engine.rootObjects().isEmpty()) return 2;
        configureDebugScreenshot(engine, app);
        return app.exec();
    }
    const QString command = args[1];
    if (command == QLatin1String("__record-daemon")) return Recorder::daemonMain(args.mid(2));
    if (command == QLatin1String("__record-bar-ipc")) {
        if (args.size() != 4 || args[2] != QLatin1String("selfview")
            || (args[3] != QLatin1String("hide") && args[3] != QLatin1String("show")))
            return usage(QStringLiteral("__record-bar-ipc requires selfview hide|show"));
        QString error;
        if (!Recorder::updateRecordingState(
                QJsonObject{{QStringLiteral("selfview"), args[3] == QLatin1String("show")}}, &error)) {
            QTextStream(stderr) << "omareel: " << error << '\n';
            return 2;
        }
        return 0;
    }
    if (command == QLatin1String("__record-bar")) {
        Theme theme;
        RecordingBar recordingBar(hiddenRecordBar);
        QQmlApplicationEngine engine;
        reportQmlWarnings(engine);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        engine.rootContext()->setContextProperty(QStringLiteral("recordingBar"), &recordingBar);
        QObject::connect(&recordingBar, &RecordingBar::finished, &app, &QCoreApplication::quit);
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/Omareel/RecordingBar.qml")));
        if (engine.rootObjects().isEmpty()) return 2;
        auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        if (!window) return 2;
        QScreen *screen = recordBarScreen(recordingBar.recordedMonitor());
        if (screen) window->setScreen(screen);
        QProcess barRule;
        barRule.start(QStringLiteral("hyprctl"), {QStringLiteral("eval"),
            QStringLiteral("hl.layer_rule({ name = 'omareel-record-bar-private', match = { namespace = 'omareel-record-bar' }, no_screen_share = true })")});
        if (!barRule.waitForFinished(3000) || barRule.exitCode() != 0)
            qWarning().noquote() << "omareel: could not apply the recording bar privacy rule";
        auto *layerWindow = LayerShellQt::Window::get(window);
        layerWindow->setLayer(LayerShellQt::Window::LayerOverlay);
        layerWindow->setAnchors(LayerShellQt::Window::AnchorTop);
        layerWindow->setExclusiveZone(0);
        layerWindow->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        layerWindow->setMargins(QMargins(0, 12, 0, 0));
        layerWindow->setScope(QStringLiteral("omareel-record-bar"));
        layerWindow->setActivateOnShow(false);
        if (screen) layerWindow->setScreen(screen);
        if (!hiddenRecordBar) window->show();

        QQuickWindow *selfViewWindow = window->findChild<QQuickWindow *>(QStringLiteral("selfViewWindow"));
        if (!selfViewWindow) {
            for (QWindow *candidate : QGuiApplication::allWindows()) {
                if (candidate->objectName() == QLatin1String("selfViewWindow")) {
                    selfViewWindow = qobject_cast<QQuickWindow *>(candidate);
                    break;
                }
            }
        }
        if (selfViewWindow && recordingBar.webcam()) {
            QScreen *selfViewScreen = nullptr;
            for (QScreen *candidate : QGuiApplication::screens())
                if (candidate->name() == recordingBar.selfViewMonitor()) selfViewScreen = candidate;
            if (!selfViewScreen) selfViewScreen = screen;
            if (selfViewScreen) {
                selfViewWindow->setScreen(selfViewScreen);
                recordingBar.setSelfViewScreenSize(selfViewScreen->geometry().size());
            }
            QProcess layerRule;
            layerRule.start(QStringLiteral("hyprctl"), {QStringLiteral("eval"),
                QStringLiteral("hl.layer_rule({ name = 'omareel-selfview-private', match = { namespace = 'omareel-selfview' }, no_screen_share = true })")});
            if (!layerRule.waitForFinished(3000) || layerRule.exitCode() != 0)
                qWarning().noquote() << "omareel: could not apply the self-view privacy rule";
            auto *selfViewLayer = LayerShellQt::Window::get(selfViewWindow);
            selfViewLayer->setLayer(LayerShellQt::Window::LayerOverlay);
            selfViewLayer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorTop)
                                      | LayerShellQt::Window::AnchorLeft);
            selfViewLayer->setExclusiveZone(0);
            selfViewLayer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
            selfViewLayer->setScope(QStringLiteral("omareel-selfview"));
            selfViewLayer->setActivateOnShow(false);
            if (selfViewScreen) selfViewLayer->setScreen(selfViewScreen);
            const auto updateSelfViewPlacement = [&recordingBar, selfViewLayer] {
                selfViewLayer->setMargins(QMargins(recordingBar.selfViewX(),
                                                   recordingBar.selfViewY(), 0, 0));
            };
            updateSelfViewPlacement();
            QObject::connect(&recordingBar, &RecordingBar::selfViewPlacementChanged,
                             selfViewWindow, updateSelfViewPlacement);
            const auto updateSelfViewInput = [&recordingBar, selfViewWindow] {
                selfViewWindow->setMask(recordingBar.selfViewVisible()
                    ? QRegion(0, 0, selfViewWindow->width(), selfViewWindow->height())
                    : QRegion());
            };
            QObject::connect(&recordingBar, &RecordingBar::selfViewVisibilityChanged,
                             selfViewWindow, updateSelfViewInput);
            updateSelfViewInput();
            selfViewWindow->show();
        }
        configureDebugScreenshot(engine, app);
        return app.exec();
    }
    if (command == QLatin1String("record")) return recordCommand(args.mid(2));
    if (command == QLatin1String("probe")) {
        if (args.size() != 3) return usage(QStringLiteral("probe requires a bundle"));
        return probeCommand(QDir(args[2]).absolutePath());
    }
    if (command == QLatin1String("export")) return exportCommand(args.mid(2));
    if (command == QLatin1String("edit")) {
        if (args.size() != 3) return usage(QStringLiteral("edit requires a bundle"));
        Editor editor(QDir(args[2]).absolutePath());
        if (!editor.isValid()) {
            QTextStream(stderr) << "omareel: " << editor.errorString() << '\n';
            return 1;
        }
        Theme theme;
        QQmlApplicationEngine engine;
        reportQmlWarnings(engine);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("comp"), &editor);
        QObject::connect(&theme, &Theme::sourceChanged, &editor, &Editor::refreshOmarchyTheme);
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/Omareel/Main.qml")));
        if (engine.rootObjects().isEmpty()) return 2;
        if (qEnvironmentVariableIntValue("OMAREEL_PREVIEW_STATS") == 1)
            QTimer::singleShot(1500, &editor, &Editor::play);
        configureDebugScreenshot(engine, app);
        return app.exec();
    }
    if (command == QLatin1String("--version") || command == QLatin1String("-V")) {
        QTextStream(stdout) << "omareel " << OMAREEL_VERSION << '\n';
        return 0;
    }
    if (command == QLatin1String("--help") || command == QLatin1String("-h")
        || command == QLatin1String("help")) return usage();
    return usage(QStringLiteral("unknown command %1").arg(command));
}
