#include "CaptureTestClient.h"
#include <LayerShellQt/Window>
#include <QGuiApplication>
#include <QQuickWindow>
#include <QQuickItem>
#include <QSGSimpleRectNode>
#include <QProcess>
#include <QTimer>
#include <QScreen>
#include <QRegularExpression>
#include <QFile>
#include <QFileSystemWatcher>
#include <qpa/qplatformnativeinterface.h>
#include <wayland-client.h>

namespace {
class AnimatedPatch : public QQuickItem
{
public:
    explicit AnimatedPatch(QQuickItem *parent) : QQuickItem(parent) { setFlag(ItemHasContents); }
    QColor color;
    QSGNode *updatePaintNode(QSGNode *old, UpdatePaintNodeData *) override {
        auto *node = static_cast<QSGSimpleRectNode *>(old);
        if (!node) node = new QSGSimpleRectNode;
        node->setRect(boundingRect());
        node->setColor(color);
        return node;
    }
};
}

int runCaptureTestClient(QGuiApplication &app, const QStringList &args, bool layer)
{
    const auto option = [&](const QString &key, const QString &fallback) {
        const int i = args.lastIndexOf(key);
        return i < 0 ? fallback : args.value(i + 1);
    };
    const QColor color(option("--color", "#ff00ff"));
    const QString scope = option("--namespace", "omareel-test");
    if (!color.isValid() || !QRegularExpression("^[a-zA-Z0-9_-]+$").match(scope).hasMatch()) return 2;
    QQuickWindow window;
    window.setTitle("Omareel capture test");
    window.setColor(color);
    window.resize(qBound(1, option("--w", "300").toInt(), 4096), qBound(1, option("--h", "200").toInt(), 4096));
    LayerShellQt::Window *shell = nullptr;
    if (layer) {
        QProcess rule;
        rule.start("hyprctl", {"eval", "hl.layer_rule({ name = '" + scope + "-test', match = { namespace = '" + scope + "' }, no_anim = true })"});
        if (!rule.waitForFinished(2000) || rule.exitCode() != 0) return 2;
        shell = LayerShellQt::Window::get(&window);
        shell->setLayer(LayerShellQt::Window::LayerOverlay);
        shell->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorTop) | LayerShellQt::Window::AnchorLeft);
        shell->setExclusiveZone(0);
        shell->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        shell->setScope(scope);
        shell->setActivateOnShow(false);
        for (auto *screen : app.screens())
            if (screen->name() == option("--monitor", "")) { window.setScreen(screen); shell->setScreen(screen); }
        shell->setMargins(QMargins(option("--x", "400").toInt(), option("--y", "300").toInt(), 0, 0));
        window.show();
    } else {
        window.showFullScreen();
    }
    const int move = args.indexOf("--move-to");
    QFileSystemWatcher moveWatcher;
    if (shell && move >= 0) {
        const auto coordinates = args.value(move + 1).split(',');
        const int delay = args.value(move + 2) == "after" ? args.value(move + 3).toInt() : args.value(move + 2).toInt();
        if (coordinates.size() != 2) return 2;
        const auto moveLayer = [&, coordinates] {
            shell->setMargins(QMargins(coordinates[0].toInt(), coordinates[1].toInt(), 0, 0));
            window.requestUpdate();
            auto *native = QGuiApplication::platformNativeInterface();
            auto *surface = static_cast<wl_surface *>(native->nativeResourceForWindow("surface", &window));
            auto *display = static_cast<wl_display *>(native->nativeResourceForIntegration("display"));
            if (surface && display) { wl_surface_commit(surface); wl_display_flush(display); }
        };
        QTimer::singleShot(qBound(0, delay, 60000), &window, moveLayer);
        // The proof harness can align a move precisely with an already-started capture.
        const auto trigger = option("--move-trigger", "");
        if (!trigger.isEmpty() && moveWatcher.addPath(trigger))
            QObject::connect(&moveWatcher, &QFileSystemWatcher::fileChanged, &window,
                             [&, trigger, moveLayer] {
                QFile file(trigger);
                if (file.open(QIODevice::ReadOnly) && !file.readAll().isEmpty()) {
                    moveWatcher.removePath(trigger);
                    moveLayer();
                }
            });
    }
    if (args.contains("--unmap-after"))
        QTimer::singleShot(qBound(0, option("--unmap-after", "60000").toInt(), 60000), &window, &QWindow::hide);
    // Optional gentle colour changes drive real scene damage for mirror-hygiene tests.
    QTimer animation;
    AnimatedPatch patch(window.contentItem());
    if (!layer && args.contains("--animate")) {
        patch.setPosition(QPointF(50, 720));
        patch.setSize(QSizeF(100, 60));
        QObject::connect(&animation, &QTimer::timeout, &window, [&, tick = 0]() mutable {
            patch.color = QColor(180, 120, 50 + (++tick % 30));
            patch.update();
        });
        animation.start(100);
    }
    QTimer::singleShot(qBound(1, option("--timeout", "60000").toInt(), 60000), &app, &QCoreApplication::quit);
    return app.exec();
}
