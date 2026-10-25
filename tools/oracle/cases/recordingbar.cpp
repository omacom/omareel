// recordingbar: the inline decisions of src/ui/RecordingBar.h (selfViewHostShouldExit,
// clampedSelfViewDragPosition) over grids, and the arithmetic inside RecordingBar's methods
// (src/ui/RecordingBar.cpp, a QObject with a camera, watchers and timers the oracle does not run),
// copied below with the lines they come from: what the bar does with the state, without the
// preference saving, state-file writes, timers and signals around it.
// tests/golden/README.md, recordingbar.

#include "common.h"
#include "recording.h"
#include "replay.h"

#include "core/RecordingPreferences.h"
#include "ui/RecordingBar.h"

#include <QDir>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>

#include <algorithm>
#include <limits>

namespace Oracle {

namespace {

using Omareel::RecordingPreferences;

QByteArray pointText(const QPoint &point) { return QByteArray::number(point.x()) + ',' + QByteArray::number(point.y()); }
QByteArray pointText(const QPointF &point) { return num(point.x()) + ',' + num(point.y()); }

// The members of RecordingBar (src/ui/RecordingBar.h at the tag) the copies below read.
struct Bar {
    QString m_cameraShape = QStringLiteral("round");
    int m_cameraRadius = 16;
    int m_corner = 3;
    QSize m_selfViewScreenSize;
    int m_selfViewPixels = 160;
    int m_selfViewX = 16;
    int m_selfViewY = 16;
    double savedX = -7; // what saveSelfViewPosition stores in the preferences
    double savedY = -7;

    // src/ui/RecordingBar.cpp:294-303 (the start of cycleCameraShape), verbatim.
    void cycleCameraShape()
    {
        // Round, then square with rounded corners, then square with sharp corners.
        if (m_cameraShape == QLatin1String("round")) {
            m_cameraShape = QStringLiteral("square");
            if (m_cameraRadius == 0) m_cameraRadius = 16;
        } else if (m_cameraRadius > 0) {
            m_cameraRadius = 0;
        } else {
            m_cameraShape = QStringLiteral("round");
            m_cameraRadius = 16;
        }
    }

    // src/ui/RecordingBar.cpp:315-326, verbatim.
    void snapSelfViewToNextCorner()
    {
        if (m_selfViewScreenSize.isEmpty())
            return;
        m_corner = (m_corner + 1) % 4;
        const int margin = 24;
        const int maxX = std::max(margin, m_selfViewScreenSize.width() - m_selfViewPixels - margin);
        const int maxY = std::max(margin, m_selfViewScreenSize.height() - m_selfViewPixels - margin);
        const int x = (m_corner == 0 || m_corner == 2) ? margin : maxX;
        const int y = (m_corner == 0 || m_corner == 1) ? margin : maxY;
        setSelfViewPosition(x, y);
    }

    // src/ui/RecordingBar.cpp:404-410 without scheduleSelfViewPlacementUpdate().
    void setSelfViewScreenSize(const QSize &size)
    {
        m_selfViewScreenSize = size;
        m_selfViewX = std::clamp(m_selfViewX, 0, std::max(0, size.width() - m_selfViewPixels));
        m_selfViewY = std::clamp(m_selfViewY, 0, std::max(0, size.height() - m_selfViewPixels));
    }

    // src/ui/RecordingBar.cpp:417-426 without the timers and scheduleSelfViewPlacementUpdate().
    void setSelfViewPosition(int x, int y)
    {
        if (m_selfViewScreenSize.isEmpty()) return;
        const int nextX = std::clamp(x, 0,
                                     std::max(0, m_selfViewScreenSize.width() - m_selfViewPixels));
        const int nextY = std::clamp(y, 0,
                                     std::max(0, m_selfViewScreenSize.height() - m_selfViewPixels));
        if (nextX == m_selfViewX && nextY == m_selfViewY) return;
        m_selfViewX = nextX;
        m_selfViewY = nextY;
    }

    // src/ui/RecordingBar.cpp:519-524 (saveSelfViewPosition after its fallback check), with the
    // preferences' fields as savedX and savedY.
    void saveSelfViewPosition()
    {
        if (m_selfViewScreenSize.isEmpty()) return;
        const int xRange = std::max(1, m_selfViewScreenSize.width() - m_selfViewPixels - 32);
        const int yRange = std::max(1, m_selfViewScreenSize.height() - m_selfViewPixels - 32);
        savedX = std::clamp((m_selfViewX - 16.0) / xRange, 0.0, 1.0);
        savedY = std::clamp((m_selfViewY - 16.0) / yRange, 0.0, 1.0);
    }
};

// src/ui/RecordingBar.cpp:283-285 (in poll), with barMonotonicUs() and
// Recorder::recordingStartedUs() as parameters.
QString elapsedText(qint64 nowUs, qint64 startedUs)
{
    const qint64 seconds = std::max<qint64>(0, nowUs - startedUs) / 1000000;
    const QString elapsed = QStringLiteral("%1:%2").arg(seconds / 60, 2, 10, QLatin1Char('0'))
                                                   .arg(seconds % 60, 2, 10, QLatin1Char('0'));
    return elapsed;
}

// src/ui/RecordingBar.cpp:230-231 (in poll), with barMonotonicUs() as a parameter.
int countdownValueAt(qint64 countdownEndUs, qint64 nowUs)
{
    const int countdownValue = std::clamp(
        int((std::max<qint64>(0, countdownEndUs - nowUs) + 999999) / 1000000), 1, 3);
    return countdownValue;
}

// src/ui/RecordingBar.cpp:64-82 (the standby host's placement in the constructor) from
// RecordingPreferences::load() on, returning the state object and m_recordedMonitor.
QJsonObject standbyState(const RecordingPreferences &prefs, QString *recordedMonitor)
{
    QProcess monitors;
    monitors.start("hyprctl", {"-j", "monitors"});
    QJsonObject monitor;
    if (monitors.waitForFinished(1000)) {
        for (const auto &value : QJsonDocument::fromJson(monitors.readAllStandardOutput()).array()) {
            if (monitor.isEmpty() || value.toObject().value("focused").toBool()) monitor = value.toObject();
            if (value.toObject().value("focused").toBool()) break;
        }
    }
    const double scale = qMax(.01, monitor.value("scale").toDouble(1));
    const int pixels = prefs.selfViewSize == "S" ? 120 : prefs.selfViewSize == "L" ? 220 : 160;
    QString &m_recordedMonitor = *recordedMonitor;
    m_recordedMonitor = monitor.value("name").toString();
    QJsonObject state;
    state = {{"camera_rotation", prefs.webcamRotation}, {"camera_flip_horizontal", prefs.webcamFlipHorizontal},
             {"selfview", prefs.selfViewEnabled}, {"selfview_safe", true}, {"selfview_monitor", m_recordedMonitor},
             {"selfview_pixels", pixels}, {"camera_device", prefs.webcamDevice}, {"camera_height", prefs.webcamHeight},
             {"selfview_x", 16 + qRound(prefs.selfViewX * qMax(0, qRound(monitor.value("width").toDouble()/scale) - pixels - 32))},
             {"selfview_y", 16 + qRound(prefs.selfViewY * qMax(0, qRound(monitor.value("height").toDouble()/scale) - pixels - 32))}};
    return state;
}

// src/ui/RecordingBar.cpp:90-92 (in the constructor): the camera shape from the preferences.
QPair<QString, int> shapeFromPreferences(const RecordingPreferences &prefs)
{
    QString m_cameraShape;
    int m_cameraRadius;
    m_cameraShape = prefs.webcam.value(QStringLiteral("shape")).toString() == QLatin1String("square")
        ? QStringLiteral("square") : QStringLiteral("round");
    m_cameraRadius = qBound(0, prefs.webcam.value(QStringLiteral("radius")).toInt(16), 80);
    return {m_cameraShape, m_cameraRadius};
}

void barHostExit(const Context &context)
{
    Text out;
    for (const bool active : {false, true})
        for (const bool quit : {false, true})
            for (const qint64 owner : {std::numeric_limits<qint64>::min(), qint64(-1), qint64(0), qint64(1), qint64(42),
                                       std::numeric_limits<qint64>::max()})
                for (const bool alive : {false, true})
                    out.row({boolText(active), boolText(quit), QByteArray::number(owner), boolText(alive),
                             boolText(Omareel::selfViewHostShouldExit(active, quit, owner, alive))});
    writeFile(QDir(context.output).filePath(QStringLiteral("self-view-host-should-exit.txt")), out.data());
}

void barDrag(const Context &context)
{
    Text out;
    const QList<QPointF> presses{{100, 100}, {0.5, 0.5}, {-0.5, 2.5}};
    const QList<QPoint> bubbles{{400, 300}, {2200, 1200}, {20, 20}, {0, 0}};
    const QList<QPointF> pointers{{100, 100}, {160, 140}, {500, 500}, {-100, -100}, {100.5, 100.5},
                                  {99.5, 99.5}, {101.49999999999999, 98.50000000000001}, {1e9, -1e9}, {3000.25, 1700.75}};
    const QList<QSize> screens{{2400, 1350}, {160, 160}, {100, 100}, {0, 0}, {-10, -10}};
    for (const QPointF &press : presses)
        for (const QPoint &bubble : bubbles)
            for (const QPointF &pointer : pointers)
                for (const QSize &screen : screens)
                    for (const int pixels : {160, 220, 0, -5})
                        out.row({pointText(press), pointText(bubble), pointText(pointer),
                                 QByteArray::number(screen.width()) + 'x' + QByteArray::number(screen.height()),
                                 QByteArray::number(pixels),
                                 pointText(Omareel::clampedSelfViewDragPosition(press, bubble, pointer, screen, pixels))});
    writeFile(QDir(context.output).filePath(QStringLiteral("clamped-drag.txt")), out.data());
}

void barMethods(const Context &context)
{
    Text out;
    for (const qint64 started : {qint64(0), qint64(1000000), qint64(-5)})
        for (const qint64 now : {qint64(0), qint64(999999), qint64(1000000), qint64(1999999), qint64(59999999),
                                 qint64(60000000), qint64(61500000), qint64(3599999999LL), qint64(3600000000LL),
                                 qint64(5999999999LL), qint64(6000000000LL), qint64(359999999999LL),
                                 qint64(-1000000)})
            out.row({"elapsed", QByteArray::number(now), QByteArray::number(started), quote(elapsedText(now, started))});
    for (const qint64 now : {qint64(1000000), qint64(0)})
        for (const qint64 end : {qint64(0), qint64(1), qint64(999999), qint64(1000000), qint64(1000001), qint64(2000000),
                                 qint64(2000001), qint64(3000000), qint64(3000001), qint64(4000000), qint64(4000001),
                                 qint64(100000000), qint64(-5)})
            out.row({"countdownValue", QByteArray::number(end), QByteArray::number(now),
                     QByteArray::number(countdownValueAt(end, now))});
    for (const QString &shape : {QStringLiteral("round"), QStringLiteral("square"), QStringLiteral("rectangle"), QString()})
        for (const int radius : {16, 0, 8, 80, -1}) {
            Bar bar;
            bar.m_cameraShape = shape;
            bar.m_cameraRadius = radius;
            QList<QByteArray> cells{"cycleCameraShape", quote(shape), QByteArray::number(radius)};
            for (int step = 0; step < 4; ++step) {
                bar.cycleCameraShape();
                cells << quote(bar.m_cameraShape) + ' ' + QByteArray::number(bar.m_cameraRadius);
            }
            out.row(cells);
        }
    for (const QSize &screen : {QSize(2400, 1350), QSize(1920, 1080), QSize(200, 200), QSize(0, 0), QSize(100, 0)})
        for (const int pixels : {120, 160, 220}) {
            Bar bar;
            bar.m_selfViewPixels = pixels;
            bar.m_selfViewScreenSize = screen;
            QList<QByteArray> cells{"snapSelfViewToNextCorner",
                                    QByteArray::number(screen.width()) + 'x' + QByteArray::number(screen.height()),
                                    QByteArray::number(pixels)};
            for (int step = 0; step < 5; ++step) {
                bar.snapSelfViewToNextCorner();
                cells << QByteArray::number(bar.m_corner) + ':' + pointText(QPoint(bar.m_selfViewX, bar.m_selfViewY));
            }
            out.row(cells);
        }
    const QList<QPoint> positions{{16, 16}, {0, 0}, {-50, 3000}, {2224, 1174}, {1000, 500}, {17, 15}};
    for (const QSize &screen : {QSize(2400, 1350), QSize(1920, 1080), QSize(192, 192), QSize(100, 100), QSize()})
        for (const int pixels : {120, 160, 220})
            for (const QPoint &position : positions) {
                Bar bar;
                bar.m_selfViewPixels = pixels;
                bar.m_selfViewX = 500;
                bar.m_selfViewY = 400;
                bar.setSelfViewScreenSize(screen);
                const QPoint resized(bar.m_selfViewX, bar.m_selfViewY);
                bar.setSelfViewPosition(position.x(), position.y());
                const QPoint placed(bar.m_selfViewX, bar.m_selfViewY);
                bar.saveSelfViewPosition();
                out.row({"position", QByteArray::number(screen.width()) + 'x' + QByteArray::number(screen.height()),
                         QByteArray::number(pixels), pointText(resized), pointText(position), pointText(placed),
                         num(bar.savedX), num(bar.savedY)});
            }
    for (const QJsonObject &webcam : {QJsonObject{}, QJsonObject{{"shape", "square"}}, QJsonObject{{"shape", "square"}, {"radius", 0}},
                                      QJsonObject{{"shape", "rectangle"}, {"radius", 40}}, QJsonObject{{"shape", "Square"}},
                                      QJsonObject{{"radius", 81}}, QJsonObject{{"radius", -3}}, QJsonObject{{"radius", 12.5}},
                                      QJsonObject{{"radius", "20"}}, QJsonObject{{"shape", 1}, {"radius", 1e10}}}) {
        RecordingPreferences prefs;
        prefs.webcam = webcam;
        const auto [shape, radius] = shapeFromPreferences(prefs);
        out.row({"shapeFromPreferences", QJsonDocument(webcam).toJson(QJsonDocument::Compact), quote(shape),
                 QByteArray::number(radius)});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("methods.txt")), out.data());

    // The standby host's first placement, with hyprctl -j monitors replayed.
    Text standby;
    const QStringList files{QStringLiteral("hyprctl/monitors/crafted-two.json"), QStringLiteral("hyprctl/monitors/crafted-focus-none.json"),
                            QStringLiteral("hyprctl/monitors/crafted-focus-both.json"), QStringLiteral("hyprctl/monitors/crafted-scales.json"),
                            QStringLiteral("hyprctl/monitors/crafted-odd-size.json"), QStringLiteral("hyprctl/monitors/crafted-wrong-types.json"),
                            QStringLiteral("hyprctl/monitors/crafted-empty-array.json"), QStringLiteral("hyprctl/monitors/crafted-garbage.txt"),
                            QStringLiteral("recorder/monitors-tiny.json"), QStringLiteral("hyprctl/monitors/real-desktop.json")};
    struct Preference { QString size; double x; double y; };
    const QList<Preference> preferences{{QStringLiteral("M"), 1, 1}, {QStringLiteral("S"), 0, 0}, {QStringLiteral("L"), 0.5, 0.25},
                                        {QStringLiteral("XL"), 0.3333333333333333, 2}, {QStringLiteral("l"), -1, 0.5}};
    for (const QString &file : files)
        for (const Preference &preference : preferences)
            for (const int status : {0, 1}) {
                if (status && preference.size != QLatin1String("M")) continue;
                Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay")));
                Replay::Answer answer;
                answer.out = readFile(QDir(context.corpus).filePath(file));
                answer.status = status;
                replay.answer(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("monitors")}, answer);
                replay.activate();
                RecordingPreferences prefs;
                prefs.selfViewSize = preference.size;
                prefs.selfViewX = preference.x;
                prefs.selfViewY = preference.y;
                QString monitor = QStringLiteral("<untouched>");
                const QJsonObject state = standbyState(prefs, &monitor);
                standby.row({file.toUtf8(), QByteArray::number(status), quote(preference.size), num(preference.x),
                             num(preference.y), quote(monitor), QJsonDocument(state).toJson(QJsonDocument::Compact)});
            }
    writeFile(QDir(context.output).filePath(QStringLiteral("standby-placement.txt")), standby.data());
}

} // namespace

void runRecordingBar(const Context &context)
{
    barHostExit(context);
    barDrag(context);
    barMethods(context);
}

} // namespace Oracle
