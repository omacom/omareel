#include "ui/CaptureTestClient.h"
#include <qpa/qplatformnativeinterface.h>
#include <wayland-client.h>
#include "Exporter.h"
#include "render/VideoDecoding.h"
#include "omareel_qt/src/cli.cxxqt.h"
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
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QQuickStyle>
#include <QQuickItem>
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
#include <functional>
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
    QString ui = ThemeFonts::bundledUiFamily();
    if (ui.isEmpty()) ui = QFontDatabase::systemFont(QFontDatabase::GeneralFont).family();
    return {ui, family};
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

static QScreen *recordBarScreen(const QString &recordedMonitor)
{
    for (QScreen *screen : QGuiApplication::screens()) {
        if (screen->name() == recordedMonitor) return screen;
    }
    return nullptr;
}

static void configureOverlayRule(const QString &scope)
{
    QProcess rule;
    // Named Lua rules merge effects. Retire the previous rule before adding ours.
    const QString expression = QStringLiteral(
        "hl.layer_rule({ name = '%1-private', enabled = false }); "
        "hl.layer_rule({ name = '%1-display', match = { namespace = '%1' }, no_anim = true })").arg(scope);
    rule.start(QStringLiteral("hyprctl"), {QStringLiteral("eval"), expression});
    if (!rule.waitForFinished(3000) || rule.exitCode() != 0)
        qWarning().noquote() << "omareel: could not apply the overlay animation rule";
}

int main(int argc, char **argv)
{
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
    if (argc > 1 && std::strcmp(argv[1], "__record-daemon") == 0) {
        QCoreApplication app(argc, argv);
        app.setApplicationName(QStringLiteral("omareel"));
        app.setOrganizationName(QStringLiteral("omareel"));
        app.setApplicationVersion(QStringLiteral(OMAREEL_VERSION));
        return omareel::record::daemonMain(app.arguments().mid(2));
    }
    const bool exporting = argc > 1 && QByteArray(argv[1]) == "export";
    const bool layerTest = argc > 1 && QByteArray(argv[1]) == "__layer-test";
    const bool windowTest = argc > 1 && QByteArray(argv[1]) == "__window-test";
    const bool recordBar = argc > 1 && QByteArray(argv[1]) == "__record-bar";
    if (recordBar) QLoggingCategory::setFilterRules(QStringLiteral("qt.multimedia.ffmpeg*=false"));
    const bool hiddenRecordBar = recordBar && argc > 2 && QByteArray(argv[2]) == "--hidden";
    const bool graphical = argc == 1 || layerTest || windowTest || recordBar || (argc > 1 && QByteArray(argv[1]) == "edit");
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
    const ResolvedFonts resolvedFonts = resolveFonts();
    QFont applicationFont(resolvedFonts.ui);
    applicationFont.setPixelSize(qRound(omareel::theme::shellFontBaseSize() * 1.083));
    QGuiApplication::setFont(applicationFont);
    ThemeFonts::setConfiguredFamily(resolvedFonts.ui);
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
    if (layerTest || windowTest) return runCaptureTestClient(app, args, layerTest);
    if (command == QLatin1String("__selfview-ipc")) return omareel::record::selfViewIpc(args.mid(2));
    if (command == QLatin1String("__record-bar-ipc")) return omareel::record::recordBarIpc(args.mid(2));
    if (command == QLatin1String("__record-bar")) {
        Theme theme;
        const int ownerIndex = args.indexOf(QStringLiteral("--owner"));
        RecordingBar recordingBar(hiddenRecordBar, args.contains(QStringLiteral("--standby")),
                                  ownerIndex >= 0 ? args.value(ownerIndex + 1).toLongLong() : 0);
        if (args.contains(QStringLiteral("--standby")) && !recordingBar.ownsStandbyHost()) return 0;
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
