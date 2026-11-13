#include "ui/CaptureTestClient.h"
#include <qpa/qplatformnativeinterface.h>
#include <wayland-client.h>
#include "Exporter.h"
#include "render/VideoDecoding.h"
#include "omareel_qt/src/agent_runner.cxxqt.h"
#include "omareel_qt/src/cli.cxxqt.h"
extern "C" int omr_ff_preload(void);
#include "omareel_qt/src/editor.cxxqt.h"
#include "omareel_qt/src/launcher.cxxqt.h"
#include "omareel_qt/src/record.cxxqt.h"
#include "omareel_qt/src/recording_bar.cxxqt.h"
#include "omareel_qt/src/theme.cxxqt.h"

#include <LayerShellQt/shell.h>
#include <LayerShellQt/window.h>

#include <QGuiApplication>
#include <QLoggingCategory>
#include <algorithm>
#include <chrono>
#include <QFont>
#include <QFontDatabase>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QImage>
#include <QImageWriter>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QIcon>
#include <QProcess>
#include <QQuickStyle>
#include <QQuickItem>
#include <QQuickWindow>
#include <QScreen>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QEventLoop>
#include <QMediaPlayer>
#include <QOffscreenSurface>
#include <QOpenGLContext>
#include <QSocketNotifier>
#include <QQmlContext>
#include <QQmlError>
#include <QStandardPaths>
#include <QTextLayout>
#include <QTextStream>
#include <QTimer>
#include <QRegularExpression>
#include <QRegion>
#include <cerrno>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <unistd.h>
#include <csignal>
#include <fcntl.h>
#include <sys/syscall.h>
#include <memory>
#include <optional>
#include <thread>
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

// The interface face. The monospace one is the theme's (Theme asks fontconfig for it).
static ResolvedFonts resolveFonts()
{
    QString ui = ThemeFonts::bundledUiFamily();
    if (ui.isEmpty()) ui = QFontDatabase::systemFont(QFontDatabase::GeneralFont).family();
    return {ui, {}};
}

static int usage(const QString &error = {})
{
    return omareel::cli::usage(error.toStdString());
}

static void configureDebugScreenshot(QQmlApplicationEngine &engine, QGuiApplication &app)
{
    QString path = qEnvironmentVariable("OMAREEL_SCREENSHOT_LIVE");
    if (path.isEmpty()) path = qEnvironmentVariable("OMAREEL_SCREENSHOT");
    if (path.isEmpty() || engine.rootObjects().isEmpty()) return;
    const int captureDelay = qEnvironmentVariableIsSet("OMAREEL_SCREENSHOT_DELAY_MS")
        ? qEnvironmentVariableIntValue("OMAREEL_SCREENSHOT_DELAY_MS")
        : qEnvironmentVariableIsEmpty("OMAREEL_SCREENSHOT_LIVE") ? 3000 : 30000;

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
        {QStringLiteral("keystrokes"), 6}, {QStringLiteral("audio"), 7}, {QStringLiteral("speech"), 8}
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
    const QString forcedHoverHandle = qEnvironmentVariable("OMAREEL_SCREENSHOT_HOVER_HANDLE").toLower();
    const QString forcedDragHandle = qEnvironmentVariable("OMAREEL_SCREENSHOT_DRAG_HANDLE").toLower();
    const QString forcedSelectHandle = qEnvironmentVariable("OMAREEL_SCREENSHOT_SELECT_HANDLE").toLower();
    const QString forcedHandle = !forcedDragHandle.isEmpty() ? forcedDragHandle
        : !forcedHoverHandle.isEmpty() ? forcedHoverHandle : forcedSelectHandle;
    if (forcedHandle == QLatin1String("clip-right") || forcedHandle == QLatin1String("zoom-left")) {
        const bool clipHandle = forcedHandle.startsWith(QLatin1String("clip"));
        const QString prefix = clipHandle ? QStringLiteral("clipTrimHandle-")
                                          : QStringLiteral("zoomTrimHandle-");
        const QString suffix = clipHandle ? QStringLiteral("-right")
                                          : QStringLiteral("-left");
        QTimer::singleShot(std::min(1500, captureDelay / 2), window,
                           [window, prefix, suffix, forcedHoverHandle, forcedDragHandle] {
            std::function<QQuickItem *(QQuickItem *)> findItem =
                [&](QQuickItem *item) -> QQuickItem * {
                if (item->objectName().startsWith(prefix) && item->objectName().endsWith(suffix))
                    return item;
                for (QQuickItem *child : item->childItems())
                    if (QQuickItem *match = findItem(child)) return match;
                return nullptr;
            };
            if (QObject *handle = findItem(window->contentItem())) {
                handle->setProperty("debugHovered", !forcedHoverHandle.isEmpty()
                                                           || !forcedDragHandle.isEmpty());
                handle->setProperty("debugDragging", !forcedDragHandle.isEmpty());
            }
        });
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
            // OMAREEL_SCREENSHOT_PLAY_TO_END=1 first plays the clip to its end (letting the player
            // reach EndOfMedia) before seeking, so scrub-after-end rendering can be verified.
            const bool playToEnd = qEnvironmentVariableIntValue("OMAREEL_SCREENSHOT_PLAY_TO_END") == 1;
            if (playToEnd) {
                QTimer::singleShot(400, editor, [editor] {
                    const double duration = editor->property("duration").toDouble();
                    QMetaObject::invokeMethod(editor, "seek", Q_ARG(double, std::max(0.0, duration - 1.5)));
                    QMetaObject::invokeMethod(editor, "play");
                });
            }
            // OMAREEL_SCREENSHOT_NO_PAUSE=1 seeks the way a playhead drag does (no pause first).
            const bool noPause = qEnvironmentVariableIntValue("OMAREEL_SCREENSHOT_NO_PAUSE") == 1;
            QTimer::singleShot(captureDelay - 400, editor, [editor, screenshotTime, noPause] {
                if (!noPause) QMetaObject::invokeMethod(editor, "pause");
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
        if (!qEnvironmentVariableIsSet("OMAREEL_SCREENSHOT_KEEP_OPEN")) app.exit(saved ? 0 : 2);
    });
}

static void reportQmlWarnings(QQmlApplicationEngine &engine)
{
    // QQmlApplicationEngine installs a file selector, which checks every URL QML loads (each
    // icon, image and component) for "+selector" directories omareel doesn't have.
    const QList<QQmlAbstractUrlInterceptor *> interceptors = engine.urlInterceptors();
    for (QQmlAbstractUrlInterceptor *interceptor : interceptors) engine.removeUrlInterceptor(interceptor);
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
    if (options.outputPath.endsWith(QStringLiteral(".srt"), Qt::CaseInsensitive)
        || options.outputPath.endsWith(QStringLiteral(".vtt"), Qt::CaseInsensitive))
        return omareel::cli::exportSubtitles(options.bundlePath.toStdString(), options.outputPath.toStdString());
    if (!options.outputPath.endsWith(QStringLiteral(".mp4"), Qt::CaseInsensitive)
        && !options.outputPath.endsWith(QStringLiteral(".gif"), Qt::CaseInsensitive))
        return usage(QStringLiteral("export output must end in .mp4, .gif, .srt or .vtt"));
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
        if (frame == total) QTextStream(stderr) << "  Finishing encoding..." << Qt::flush;
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

// `omareel frame`: frames of the export as images (Exporter::renderStills), one file per time or
// a contact sheet. The arguments are checked in Rust (export::still::parse_frame_args).
static int frameCommand(const QStringList &arguments)
{
    const omareel::exporter::FrameRequest request = omareel::exporter::parseFrameArgs(arguments);
    if (!request.error.isEmpty()) return usage(request.error);
    StillOptions options;
    options.bundlePath = QDir(request.bundle).absolutePath();
    for (const double time : request.times) options.times.append(time);
    options.count = request.count;
    options.fps = request.fps;
    const int stills = request.count > 0 ? request.count : int(request.times.size());
    const omareel::exporter::SheetGrid grid = omareel::exporter::sheetGrid(stills, request.width);
    // A sheet's --width is the sheet's; each tile renders at its share.
    options.width = request.sheet ? grid.tile_width : request.width;
    const QString output = QFileInfo(request.output).absoluteFilePath();
    // Bad arguments, found before anything renders: exit status 2, as for the others.
    if (!QFileInfo(QFileInfo(output).absolutePath()).isDir()) {
        QTextStream(stderr) << "omareel: no directory " << QFileInfo(output).absolutePath() << '\n';
        return 2;
    }
    if (!QImageWriter::supportedImageFormats().contains(QFileInfo(output).suffix().toLower().toLatin1())) {
        QTextStream(stderr) << "omareel: this Qt cannot write ." << QFileInfo(output).suffix() << " images\n";
        return 2;
    }

    Exporter exporter;
    // Progress only for a person watching: an agent reading stderr gets the errors alone.
    if (stills > 1 && isatty(STDERR_FILENO)) {
        QObject::connect(&exporter, &Exporter::progress, [](int frame, int total) {
            QTextStream(stderr) << '\r' << "Rendering " << frame << '/' << total << (frame == total ? "\n" : "")
                                << Qt::flush;
        });
    }
    const auto save = [](const QImage &image, const QString &path) {
        const bool jpeg = path.endsWith(QStringLiteral(".jpg"), Qt::CaseInsensitive)
                       || path.endsWith(QStringLiteral(".jpeg"), Qt::CaseInsensitive);
        if (image.save(path, nullptr, jpeg ? 90 : -1)) {
            QTextStream(stdout) << path << Qt::endl;
            return true;
        }
        return false;
    };
    // Separate stills are written as they render, so only one is held at a time and an
    // unwritable output stops at the first. A sheet's tiles are small; it keeps them.
    const QStringList paths = omareel::exporter::stillOutputPaths(output, stills);
    if (!request.sheet) {
        options.take = [&](int index, const Still &still, QString *error) {
            if (save(still.image, paths[index])) return true;
            *error = QStringLiteral("could not write %1").arg(paths[index]);
            return false;
        };
    }
    QList<Still> rendered;
    double duration = 0.0;
    QString failure;
    if (!exporter.renderStills(options, &rendered, &duration, &failure)) {
        QTextStream(stderr) << "omareel: " << (failure.isEmpty() ? QStringLiteral("Rendering failed") : failure) << '\n';
        return 1;
    }
    for (const double time : request.times) {
        if (time >= duration)
            QTextStream(stderr) << "omareel: " << time << " s is past the end (" << duration
                                << " s); showing the last frame\n";
    }
    if (!request.sheet) return 0;
    if (save(Exporter::contactSheet(rendered, grid.columns, grid.gap), output)) return 0;
    QTextStream(stderr) << "omareel: could not write " << output << '\n';
    return 1;
}

static QScreen *recordBarScreen(const QString &recordedMonitor)
{
    for (QScreen *screen : QGuiApplication::screens()) {
        if (screen->name() == recordedMonitor) return screen;
    }
    return nullptr;
}

// Whether Hyprland took a hyprctl request: it answers `ok`, and a refusal comes back with exit
// status 0 all the same.
static bool hyprctlAccepted(const QStringList &arguments)
{
    QProcess request;
    request.start(QStringLiteral("hyprctl"), arguments);
    return request.waitForFinished(3000) && request.exitCode() == 0
        && request.readAllStandardOutput().trimmed() == "ok";
}

static void configureOverlayRule(const QString &scope)
{
    // Named Lua rules merge effects. Retire the previous rule before adding ours.
    const QString expression = QStringLiteral(
        "hl.layer_rule({ name = '%1-private', enabled = false }); "
        "hl.layer_rule({ name = '%1-display', match = { namespace = '%1' }, no_anim = true })").arg(scope);
    // Hyprland 0.56 with a hyprlang configuration (Omarchy's stable channel) answers eval with
    // "eval is only supported with the lua config manager" and takes the rule as a keyword.
    if (!hyprctlAccepted({QStringLiteral("eval"), expression})
        && !hyprctlAccepted({QStringLiteral("keyword"), QStringLiteral("layerrule"),
                             QStringLiteral("no_anim on, match:namespace %1").arg(scope)}))
        qWarning().noquote() << "omareel: could not apply the overlay animation rule";
}

// `omareel edit --wait <recorder pid> <bundle>`: the recorder starts the editor while it records,
// so that when Stop is pressed the editor only has to open the recording and show its window.
// Until then it starts Qt, compiles the editor's QML and warms the GL driver and Qt Multimedia.
// The recorder sends SIGUSR1 once the recording is saved; a recorder that exits without sending
// it (discarded, failed) ends the wait, and the editor with it.
//
// `omareel __record-bar --wait <starter pid>` is the recording bar the same way, started by
// `omareel record` before its region picker and by the launcher (waiting_bar.rs). The recorder
// adopts it with SIGUSR1 sent by sigqueue, the value its flags (1: hidden); one nobody adopts
// quits two seconds after its starter has gone.
static int s_openPipe[2] = {-1, -1};

static void requestOpen(int, siginfo_t *info, void *)
{
    const char byte = char(info && info->si_code == SI_QUEUE ? info->si_value.sival_int & 0x7f : 0);
    [[maybe_unused]] const ssize_t written = ::write(s_openPipe[1], &byte, 1);
}

static bool waitingEditor(int argc, char **argv)
{
    return argc == 5 && std::strcmp(argv[1], "edit") == 0 && std::strcmp(argv[2], "--wait") == 0;
}

static bool waitingBar(int argc, char **argv)
{
    return argc == 4 && std::strcmp(argv[1], "__record-bar") == 0 && std::strcmp(argv[2], "--wait") == 0;
}

// First thing in main: the recorder checks the handler is installed (SigCgt, or the bar's pid
// file) before signalling.
static void installOpenRequest()
{
    if (::pipe2(s_openPipe, O_CLOEXEC | O_NONBLOCK) != 0) return;
    struct sigaction action {};
    action.sa_sigaction = requestOpen;
    sigemptyset(&action.sa_mask);
    action.sa_flags = SA_RESTART | SA_SIGINFO;
    ::sigaction(SIGUSR1, &action, nullptr);
}

// Lays out the printable ASCII range in each family, size and weight the editor's text uses:
// the font engines and glyph metrics are then cached for its first layout (15 ms of it).
static void warmFonts(const QObject &theme)
{
    QString sample;
    for (char16_t c = 0x20; c < 0x7f; ++c) sample += QChar(c);
    sample += QStringLiteral("·—–…×←→↑↓⌘⇧⌥");
    QList<int> sizes;
    const QVariantMap type = theme.property("type").toMap();
    for (const QVariant &size : type) sizes << size.toInt() << size.toInt() - 1;
    const QStringList families{theme.property("fontFamily").toString(), theme.property("monoFamily").toString()};
    for (const QString &family : families) {
        for (const int size : std::as_const(sizes)) {
            if (size <= 0) continue;
            for (const QFont::Weight weight : {QFont::Normal, QFont::Medium, QFont::DemiBold, QFont::Bold}) {
                QFont font(family);
                font.setPixelSize(size);
                font.setWeight(weight);
                QTextLayout layout(sample, font);
                layout.beginLayout();
                layout.createLine();
                layout.endLayout();
            }
        }
    }
}

static void warmEditor(QQmlApplicationEngine &engine, std::unique_ptr<QQmlComponent> &main, const QObject &theme)
{
    warmFonts(theme);
    // Compiling Main.qml loads every type and import it uses; the engine keeps the compiled
    // units, so the load after Stop only creates the objects.
    main = std::make_unique<QQmlComponent>(&engine, QUrl(QStringLiteral("qrc:/qt/qml/Omareel/Main.qml")),
                                           QQmlComponent::PreferSynchronous);
    // The driver's first context (and its shader cache) and the scene graph's render loop.
    {
        QOffscreenSurface surface;
        surface.create();
        QOpenGLContext context;
        if (context.create() && context.makeCurrent(&surface)) context.doneCurrent();
    }
    { QQuickWindow warm; }
    // Qt Multimedia's backend (the FFmpeg plugin and its hardware probes).
    { QMediaPlayer player; }
}

// Waits for the signal: its flags, or -1 when `starterPid` exited without sending it (after
// `graceMs` more, for a signal on its way).
static int waitForOpenRequest(qint64 starterPid, int graceMs)
{
    int flags = -1;
    QEventLoop loop;
    auto requested = [&] {
        char byte;
        if (flags < 0 && ::read(s_openPipe[0], &byte, 1) == 1) flags = byte;
        return flags >= 0;
    };
    if (requested()) return flags;
    QSocketNotifier openNotifier(s_openPipe[0], QSocketNotifier::Read);
    QObject::connect(&openNotifier, &QSocketNotifier::activated, &loop, [&] { if (requested()) loop.quit(); });
    const int starter = int(::syscall(SYS_pidfd_open, pid_t(starterPid), 0));
    if (starter < 0) {
        QTimer::singleShot(graceMs, &loop, &QEventLoop::quit);
        loop.exec();
        requested();
        return flags;
    }
    QSocketNotifier starterNotifier(starter, QSocketNotifier::Read);
    QObject::connect(&starterNotifier, &QSocketNotifier::activated, &loop, [&] {
        starterNotifier.setEnabled(false);
        QTimer::singleShot(graceMs, &loop, [&] { requested(); loop.quit(); });
    });
    loop.exec();
    ::close(starter);
    requested();
    return flags;
}

int main(int argc, char **argv)
{
    // A coding agent omareel started (the editor's Polish, `omareel agent`) may only describe,
    // edit, look at and transcribe its own recording, whichever omareel it runs
    // (crates/omareel_core/src/agent/scope.rs).
    if (std::getenv("OMAREEL_AGENT_SCOPE")) {
        ::rust::Vec<::rust::String> arguments;
        for (int i = 1; i < argc; ++i)
            arguments.push_back(::rust::String(QString::fromLocal8Bit(argv[i]).toStdString()));
        const ::rust::String refusal = omareel::cli::agentScopeRefusal(std::move(arguments));
        if (!refusal.empty()) {
            std::fputs(std::string(refusal).c_str(), stderr);
            return 126;
        }
    }
    if (waitingEditor(argc, argv) || waitingBar(argc, argv)) installOpenRequest();
    if (waitingBar(argc, argv)) omareel::record::registerWaitingBar();
    omareel::cli::installMessageHandler();
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
    // The recorder daemon shows nothing, so it has no display connection: its screen capture
    // opens the only one it needs.
    // Transcribing needs no Qt application.
    if (argc > 1 && std::strcmp(argv[1], "transcribe") == 0) {
        ::rust::Vec<::rust::String> arguments;
        for (int i = 2; i < argc; ++i) arguments.push_back(::rust::String(argv[i]));
        return omareel::cli::transcribe(std::move(arguments));
    }
    if (argc > 1 && std::strcmp(argv[1], "__record-daemon") == 0) {
        QCoreApplication app(argc, argv);
        app.setApplicationName(QStringLiteral("omareel"));
        app.setOrganizationName(QStringLiteral("omareel"));
        app.setApplicationVersion(QStringLiteral(OMAREEL_VERSION));
        return omareel::record::daemonMain(app.arguments().mid(2));
    }
    // Nor do `omareel record` (the keybinding's start and stop, and the launcher's) and the
    // bar's and self-view's commands: a QGuiApplication's display connection, fonts and D-Bus
    // registration were 40 ms of each, before Stop even reached the recorder.
    if (argc > 1 && (std::strcmp(argv[1], "describe") == 0 || std::strcmp(argv[1], "apply") == 0
                     || std::strcmp(argv[1], "skill") == 0 || std::strcmp(argv[1], "agent") == 0)) {
        ::rust::Vec<::rust::String> arguments;
        // rust::String throws on invalid UTF-8; QString substitutes it as other commands do.
        for (int i = 2; i < argc; ++i)
            arguments.push_back(::rust::String(QString::fromLocal8Bit(argv[i]).toStdString()));
        return omareel::cli::agentCommand(argv[1], std::move(arguments));
    }
    if (argc > 1 && (std::strcmp(argv[1], "record") == 0 || std::strcmp(argv[1], "__selfview-ipc") == 0
                     || std::strcmp(argv[1], "__record-bar-ipc") == 0)) {
        QCoreApplication app(argc, argv);
        app.setApplicationName(QStringLiteral("omareel"));
        app.setOrganizationName(QStringLiteral("omareel"));
        app.setApplicationVersion(QStringLiteral(OMAREEL_VERSION));
        const QStringList args = app.arguments();
        if (args[1] == QLatin1String("record")) return omareel::record::recordCommand(args.mid(2));
        if (args[1] == QLatin1String("__selfview-ipc")) return omareel::record::selfViewIpc(args.mid(2));
        return omareel::record::recordBarIpc(args.mid(2));
    }
    const bool exporting = argc > 1 && (QByteArray(argv[1]) == "export" || QByteArray(argv[1]) == "frame");
    const bool layerTest = argc > 1 && QByteArray(argv[1]) == "__layer-test";
    const bool windowTest = argc > 1 && QByteArray(argv[1]) == "__window-test";
    const bool recordBar = argc > 1 && QByteArray(argv[1]) == "__record-bar";
    if (recordBar) QLoggingCategory::setFilterRules(QStringLiteral("qt.multimedia.ffmpeg*=false"));
    const bool hiddenRecordBar = recordBar && argc > 2 && QByteArray(argv[2]) == "--hidden";
    const bool graphical = argc == 1 || layerTest || windowTest || recordBar || (argc > 1 && QByteArray(argv[1]) == "edit");
    // Opening a recording runs ffprobe and reads its project and input log, and the theme asks
    // fc-match and hyprctl: start them now, so they overlap Qt's own start-up.
    if (argc == 3 && QByteArray(argv[1]) == "edit")
        omareel::cli::preloadEditor(QString::fromLocal8Bit(argv[2]).toStdString());
    if (graphical) omareel::cli::prefetchTheme();
    if (argc == 1) omareel::cli::prefetchLauncher();
    // FFmpeg's libraries are opened, not linked (ffmpeg_dl.h): a window process opens them on a
    // thread now, so Qt Multimedia's FFmpeg plugin, which links them, finds them loaded.
    if (graphical || exporting) std::thread([] { omr_ff_preload(); }).detach();
    if (recordBar || layerTest) LayerShellQt::Shell::useLayerShell();
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
    configureVideoDecoding();
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
    if (graphical) {
        // Every icon is a bundled SVG, but QQuickIconImage still asks the icon theme for each
        // (QIconLoader parsing index.theme files and testing every directory of the theme and
        // its parents): 120 ms of the editor's start. A search path holding no theme ends
        // those lookups at once (an empty list would mean the system's paths).
        QIcon::setThemeSearchPaths({QStringLiteral(":/omareel/no-icon-themes")});
        QIcon::setFallbackSearchPaths({});
    }
    const ResolvedFonts resolvedFonts = resolveFonts();
    QFont applicationFont(resolvedFonts.ui);
    applicationFont.setPixelSize(qRound(omareel::theme::shellFontBaseSize() * 1.083));
    QGuiApplication::setFont(applicationFont);
    ThemeFonts::setConfiguredFamily(resolvedFonts.ui);
    if (graphical && qEnvironmentVariable("OMAREEL_DEBUG") == QLatin1String("1"))
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
        // A recording bar starts waiting once the launcher is on screen (waiting_bar.rs).
        if (qEnvironmentVariableIsEmpty("OMAREEL_SCREENSHOT"))
            QTimer::singleShot(300, &app, [] { omareel::record::startWaitingBar(); });
        configureDebugScreenshot(engine, app);
        return app.exec();
    }
    const QString command = args[1];
    if (layerTest || windowTest) return runCaptureTestClient(app, args, layerTest);
    if (command == QLatin1String("__selfview-ipc")) return omareel::record::selfViewIpc(args.mid(2));
    if (command == QLatin1String("__record-bar-ipc")) return omareel::record::recordBarIpc(args.mid(2));
    if (command == QLatin1String("__record-bar")) {
        Theme theme;
        // The bar outlives the engine, as its bindings read it while the engine tears down; it
        // is made after the wait, which already uses the engine.
        std::optional<RecordingBar> bar;
        QQmlApplicationEngine engine;
        reportQmlWarnings(engine);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        bool hidden = hiddenRecordBar;
        std::unique_ptr<QQmlComponent> warmBar;
        if (waitingBar(argc, argv)) {
            warmFonts(theme);
            warmBar = std::make_unique<QQmlComponent>(&engine, QUrl(QStringLiteral("qrc:/qt/qml/Omareel/RecordingBar.qml")),
                                                      QQmlComponent::PreferSynchronous);
            {
                QOffscreenSurface surface;
                surface.create();
                QOpenGLContext context;
                if (context.create() && context.makeCurrent(&surface)) context.doneCurrent();
            }
            { QQuickWindow warm; }
            const int flags = waitForOpenRequest(args[3].toLongLong(), 2000);
            omareel::record::unregisterWaitingBar();
            if (flags < 0) return 0;
            hidden = (flags & 1) != 0;
        }
        const int ownerIndex = args.indexOf(QStringLiteral("--owner"));
        RecordingBar &recordingBar = bar.emplace(hidden, args.contains(QStringLiteral("--standby")),
                                                 ownerIndex >= 0 ? args.value(ownerIndex + 1).toLongLong() : 0);
        if (args.contains(QStringLiteral("--standby")) && !recordingBar.ownsStandbyHost()) return 0;
        engine.rootContext()->setContextProperty(QStringLiteral("recordingBar"), &recordingBar);
        QObject::connect(&recordingBar, &RecordingBar::finished, &app, &QCoreApplication::quit);
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/Omareel/RecordingBar.qml")));
        if (engine.rootObjects().isEmpty()) return 2;
        auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        if (!window) return 2;
        QScreen *screen = recordBarScreen(recordingBar.recordedMonitor());
        if (screen) window->setScreen(screen);
        configureOverlayRule(QStringLiteral("omareel-record-bar"));
        auto *layerWindow = LayerShellQt::Window::get(window);
        layerWindow->setLayer(LayerShellQt::Window::LayerOverlay);
        layerWindow->setAnchors(LayerShellQt::Window::AnchorTop);
        layerWindow->setExclusiveZone(0);
        layerWindow->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        layerWindow->setMargins(QMargins(0, 12, 0, 0));
        layerWindow->setScope(QStringLiteral("omareel-record-bar"));
        layerWindow->setActivateOnShow(false);
        if (screen) layerWindow->setScreen(screen);
        if (!args.contains(QStringLiteral("--standby")))
            recordingBar.publishBarGeometry(window->width(), window->height());
        const auto updateBarVisibility = [&recordingBar, window, layerWindow] {
            QScreen *target = recordBarScreen(recordingBar.recordedMonitor());
            if (target && layerWindow->screen() != target) { window->hide(); window->destroy(); window->setScreen(target); layerWindow->setScreen(target); }
            if (target && recordingBar.captureStarted() && !recordingBar.hidden()) window->show();
            else window->hide();
        };
        QObject::connect(&recordingBar, &RecordingBar::captureStartedChanged,
                         window, updateBarVisibility);
        auto barMappedLogged = std::make_shared<bool>(false);
        QObject::connect(window, &QWindow::visibleChanged, window,
                         [&recordingBar, window, barMappedLogged](bool visible) {
            if (!visible || *barMappedLogged) return;
            QTimer::singleShot(0, window, [&recordingBar, window, barMappedLogged] {
                if (!window->isVisible() || *barMappedLogged) return;
                *barMappedLogged = true;
                recordingBar.logUiStage(QStringLiteral("bar_mapped"));
            });
        });
        QObject::connect(&app, &QGuiApplication::screenRemoved, window, updateBarVisibility);
        updateBarVisibility();

        QQuickWindow *countdownWindow = window->findChild<QQuickWindow *>(
            QStringLiteral("countdownWindow"));
        if (!countdownWindow) {
            for (QWindow *candidate : QGuiApplication::allWindows()) {
                if (candidate->objectName() == QLatin1String("countdownWindow")) {
                    countdownWindow = qobject_cast<QQuickWindow *>(candidate);
                    break;
                }
            }
        }
        if (countdownWindow) {
            if (screen) countdownWindow->setScreen(screen);
            auto *countdownLayer = LayerShellQt::Window::get(countdownWindow);
            countdownLayer->setLayer(LayerShellQt::Window::LayerOverlay);
            countdownLayer->setAnchors({});
            countdownLayer->setExclusiveZone(0);
            countdownLayer->setKeyboardInteractivity(
                LayerShellQt::Window::KeyboardInteractivityNone);
            countdownLayer->setScope(QStringLiteral("omareel-record-bar"));
            countdownLayer->setActivateOnShow(false);
            if (screen) countdownLayer->setScreen(screen);
            if (!args.contains(QStringLiteral("--standby")))
                recordingBar.publishCountdownGeometry(countdownWindow->width(), countdownWindow->height());
            const auto updateCountdownVisibility = [&recordingBar, countdownWindow, countdownLayer] {
                QScreen *target = recordBarScreen(recordingBar.recordedMonitor());
                if (target && target != countdownLayer->screen()) {
                    countdownWindow->hide(); countdownWindow->destroy(); countdownWindow->setScreen(target); countdownLayer->setScreen(target);
                }
                if (target && recordingBar.countdownActive() && !recordingBar.captureStarted())
                    countdownWindow->show();
                else
                    countdownWindow->hide();
            };
            QObject::connect(&recordingBar, &RecordingBar::countdownChanged,
                             countdownWindow, updateCountdownVisibility);
            QObject::connect(&recordingBar, &RecordingBar::captureStartedChanged,
                             countdownWindow, updateCountdownVisibility);
            updateCountdownVisibility();
        }

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
                if (qEnvironmentVariableIsSet("OMAREEL_SELFVIEW_DRAG_LOG"))
                    qInfo().nospace() << "self-view logical-screen="
                                      << selfViewScreen->geometry().width() << 'x'
                                      << selfViewScreen->geometry().height()
                                      << " scale=" << selfViewScreen->devicePixelRatio();
            }
            configureOverlayRule(QStringLiteral("omareel-selfview"));
            auto *selfViewLayer = LayerShellQt::Window::get(selfViewWindow);
            selfViewLayer->setLayer(LayerShellQt::Window::LayerOverlay);
            selfViewLayer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorTop)
                                      | LayerShellQt::Window::AnchorLeft);
            selfViewLayer->setExclusiveZone(0);
            selfViewLayer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
            selfViewLayer->setScope(QStringLiteral("omareel-selfview"));
            selfViewLayer->setActivateOnShow(false);
            if (selfViewScreen) selfViewLayer->setScreen(selfViewScreen);
            const auto updateSelfViewPlacement = [&recordingBar, selfViewLayer, selfViewWindow] {
                for (QScreen *target : QGuiApplication::screens()) {
                    if (target->name() != recordingBar.selfViewMonitor() || target == selfViewLayer->screen()) continue;
                    selfViewWindow->hide();
                    selfViewWindow->destroy();
                    selfViewWindow->setScreen(target);
                    selfViewLayer->setScreen(target);
                    recordingBar.setSelfViewScreenSize(target->geometry().size());
                }
                selfViewLayer->setMargins(QMargins(recordingBar.selfViewX(),
                                                   recordingBar.selfViewY(), 0, 0));
                selfViewWindow->resize(recordingBar.selfViewPixels(), recordingBar.selfViewPixels());
                selfViewWindow->requestUpdate();
                // Margins are double-buffered Wayland state. Commit geometry immediately,
                // including when the camera has stopped delivering frames.
                if (QGuiApplication::platformName().startsWith(QLatin1String("wayland"))) {
                    auto *native = QGuiApplication::platformNativeInterface();
                    auto *surface = static_cast<wl_surface *>(native->nativeResourceForWindow("surface", selfViewWindow));
                    auto *display = static_cast<wl_display *>(native->nativeResourceForIntegration("display"));
                    if (surface && display) { wl_surface_commit(surface); wl_display_flush(display); }
                }
            };
            updateSelfViewPlacement();
            QObject::connect(&recordingBar, &RecordingBar::selfViewPlacementChanged,
                             selfViewWindow, updateSelfViewPlacement);
            const auto updateSelfViewVisibility = [&recordingBar, selfViewWindow] {
                const bool visible = recordingBar.bubbleMapped() && recordBarScreen(recordingBar.selfViewMonitor());
                selfViewWindow->setMask(visible
                    ? QRegion(0, 0, selfViewWindow->width(), selfViewWindow->height())
                    : QRegion());
                if (visible) selfViewWindow->show();
                else selfViewWindow->hide();
            };
            QObject::connect(&recordingBar, &RecordingBar::selfViewVisibilityChanged,
                             selfViewWindow, updateSelfViewVisibility);
            QObject::connect(&recordingBar, &RecordingBar::captureStartedChanged,
                             selfViewWindow, updateSelfViewVisibility);
            QObject::connect(&recordingBar, &RecordingBar::selfViewPlacementChanged,
                             selfViewWindow, updateSelfViewVisibility);
            QObject::connect(&app, &QGuiApplication::screenRemoved, selfViewWindow, updateSelfViewVisibility);
            updateSelfViewVisibility();
        }
        const auto grabHook = [](QQuickWindow *target, const char *variable) {
            const QString path = qEnvironmentVariable(variable);
            if (!target || path.isEmpty()) return;
            const int delay = qEnvironmentVariableIsSet("OMAREEL_SCREENSHOT_DELAY_MS")
                ? qEnvironmentVariableIntValue("OMAREEL_SCREENSHOT_DELAY_MS") : 4000;
            QTimer::singleShot(qMax(0, delay), target, [target, path] { target->grabWindow().save(path); });
        };
        grabHook(selfViewWindow, "OMAREEL_SCREENSHOT_SELFVIEW");
        grabHook(window, "OMAREEL_SCREENSHOT_BAR");
        configureDebugScreenshot(engine, app);
        return app.exec();
    }
    if (command == QLatin1String("record")) return omareel::record::recordCommand(args.mid(2));
    if (command == QLatin1String("probe")) {
        if (args.size() != 3) return usage(QStringLiteral("probe requires a bundle"));
        return omareel::cli::probe(QDir(args[2]).absolutePath().toStdString());
    }
    if (command == QLatin1String("export")) return exportCommand(args.mid(2));
    if (command == QLatin1String("frame")) return frameCommand(args.mid(2));
    if (command == QLatin1String("edit")) {
        const bool waiting = waitingEditor(argc, argv);
        if (args.size() != 3 && !waiting) return usage(QStringLiteral("edit requires a bundle"));
        const QString bundle = QDir(args[waiting ? 4 : 2]).absolutePath();
        std::unique_ptr<Theme> waitingTheme;
        std::unique_ptr<QQmlApplicationEngine> waitingEngine;
        std::unique_ptr<QQmlComponent> warmMain;
        if (waiting) {
            waitingTheme = std::make_unique<Theme>();
            waitingEngine = std::make_unique<QQmlApplicationEngine>();
            reportQmlWarnings(*waitingEngine);
            waitingEngine->rootContext()->setContextProperty(QStringLiteral("theme"), waitingTheme.get());
            warmEditor(*waitingEngine, warmMain, *waitingTheme);
            // A signal sent just before the recorder exited may still be on its way to a thread.
            if (waitForOpenRequest(args[3].toLongLong(), 100) < 0) return 0;
        }
        Editor editor(bundle);
        if (!editor.isValid()) {
            QTextStream(stderr) << "omareel: " << editor.errorString() << '\n';
            return 1;
        }
        // The coding agent the AI drawer runs on this bundle (agent_runner.rs); made before the
        // engine so it outlives the QML that reads it.
        AgentRunner agent(bundle);
        std::unique_ptr<Theme> ownTheme = waiting ? std::move(waitingTheme) : std::make_unique<Theme>();
        Theme &theme = *ownTheme;
        std::unique_ptr<QQmlApplicationEngine> ownEngine = waiting ? std::move(waitingEngine) : std::make_unique<QQmlApplicationEngine>();
        QQmlApplicationEngine &engine = *ownEngine;
        if (!waiting) reportQmlWarnings(engine);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("comp"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("agent"), &agent);
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
