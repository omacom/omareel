#include "RegionPicker.h"
#include "ScreenCapture.h"
#include "core/RecordingPreferences.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QElapsedTimer>
#include <QProcess>
#include <QPointF>
#include <QRegularExpression>
#include <QStandardPaths>
#include <cmath>
#include <algorithm>

using namespace Omareel;

struct Monitor {
    QString name;
    double x, y, logicalWidth, logicalHeight, scale;
    int physicalWidth, physicalHeight;
    bool focused;
};

static QVector<Monitor> monitors(QString *error)
{
    QProcess process;
    process.start(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("monitors")});
    if (!process.waitForFinished(3000) || process.exitCode() != 0) {
        if (error) *error = QStringLiteral("hyprctl monitors failed: %1").arg(QString::fromUtf8(process.readAllStandardError()));
        return {};
    }
    QVector<Monitor> result;
    for (const auto &value : QJsonDocument::fromJson(process.readAllStandardOutput()).array()) {
        const auto object = value.toObject();
        const double scale = object.value("scale").toDouble(1.0);
        int physicalWidth = object.value("width").toInt();
        int physicalHeight = object.value("height").toInt();
        const int transform = object.value("transform").toInt();
        if (transform % 2) std::swap(physicalWidth, physicalHeight);
        result << Monitor{object.value("name").toString(), object.value("x").toDouble(),
                          object.value("y").toDouble(), physicalWidth / scale,
                          physicalHeight / scale, scale, physicalWidth, physicalHeight,
                          object.value("focused").toBool()};
    }
    return result;
}

QStringList RegionPicker::parseCaptureOptions(const QString &output)
{
    static const QRegularExpression pattern(
        QStringLiteral(R"(^\s*([^|\r\n]+?)\s*\|\s*\d+x\d+\s*$)"));
    QStringList result;
    for (const QString &line : output.split(QLatin1Char('\n'))) {
        const auto match = pattern.match(line);
        if (match.hasMatch()) {
            const QString name = match.captured(1).trimmed();
            if (!name.isEmpty() && !result.contains(name)) result << name;
        }
    }
    return result;
}

bool RegionPicker::isClickSelection(double width, double height)
{
    return width > 0.0 && height > 0.0 && width * height < 20.0;
}

static QStringList captureOptions()
{
    QProcess process;
    QElapsedTimer timer;
    timer.start();
    process.start(QStringLiteral("gpu-screen-recorder"),
                  {QStringLiteral("--list-capture-options")});
    if (!process.waitForStarted(3000)) return {};
    const int remainingMs = std::max(0, 3000 - int(timer.elapsed()));
    if (!process.waitForFinished(remainingMs)) {
        process.kill();
        process.waitForFinished(1000);
        return {};
    }
    if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0) return {};
    return RegionPicker::parseCaptureOptions(QString::fromUtf8(process.readAllStandardOutput()));
}

static const Monitor *capturableMonitor(const QVector<Monitor> &values, const Monitor *requested,
                                        QString *note)
{
    const QString backend = qEnvironmentVariable("OMAREEL_CAPTURE").toLower();
    if (backend != QLatin1String("gsr")
        && RecordingPreferences::load().captureBackend != QLatin1String("gsr")
        && ScreenCapture::isSupported()) return requested;
    const QStringList options = captureOptions();
    if (options.isEmpty() || options.contains(requested->name)) return requested;
    for (const QString &name : options) {
        for (const auto &monitor : values) {
            if (monitor.name == name) {
                if (note) {
                    *note = QStringLiteral("%1 is not capturable by gpu-screen-recorder; recording %2 instead")
                                .arg(requested->name, monitor.name);
                }
                return &monitor;
            }
        }
    }
    return requested;
}

static bool parseRect(const QString &text, double *x, double *y, double *w, double *h)
{
    static const QRegularExpression pattern(
        QStringLiteral(R"(^\s*(-?\d+(?:\.\d+)?),(-?\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)x(\d+(?:\.\d+)?)\s*$)"));
    const auto match = pattern.match(text);
    if (!match.hasMatch()) return false;
    *x = match.captured(1).toDouble(); *y = match.captured(2).toDouble();
    *w = match.captured(3).toDouble(); *h = match.captured(4).toDouble();
    return true;
}

static const Monitor *monitorFor(const QVector<Monitor> &values, double x, double y, double w, double h)
{
    const QPointF center(x + w / 2.0, y + h / 2.0);
    for (const auto &monitor : values) {
        if (center.x() >= monitor.x && center.x() < monitor.x + monitor.logicalWidth
            && center.y() >= monitor.y && center.y() < monitor.y + monitor.logicalHeight)
            return &monitor;
    }
    return values.isEmpty() ? nullptr : &values.first();
}

static QString windowRectangles(QString *error)
{
    QProcess workspace;
    workspace.start(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("activeworkspace")});
    if (!workspace.waitForFinished(3000) || workspace.exitCode() != 0) return {};
    const int activeId = QJsonDocument::fromJson(workspace.readAllStandardOutput()).object().value("id").toInt();
    QProcess clients;
    clients.start(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("clients")});
    if (!clients.waitForFinished(3000) || clients.exitCode() != 0) {
        if (error) *error = QStringLiteral("hyprctl clients failed");
        return {};
    }
    QString lines;
    for (const auto &value : QJsonDocument::fromJson(clients.readAllStandardOutput()).array()) {
        const auto client = value.toObject();
        if (client.value("workspace").toObject().value("id").toInt() != activeId
            || client.value("hidden").toBool()) continue;
        const auto at = client.value("at").toArray();
        const auto size = client.value("size").toArray();
        if (at.size() == 2 && size.size() == 2)
            lines += QStringLiteral("%1,%2 %3x%4\n").arg(at[0].toInt()).arg(at[1].toInt())
                     .arg(size[0].toInt()).arg(size[1].toInt());
    }
    return lines;
}

static bool snapToWindow(const QString &rectangles, double pointX, double pointY,
                         double *x, double *y, double *w, double *h)
{
    for (const QString &line : rectangles.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        double rectX, rectY, rectWidth, rectHeight;
        if (!parseRect(line, &rectX, &rectY, &rectWidth, &rectHeight)) continue;
        if (pointX < rectX || pointX >= rectX + rectWidth
            || pointY < rectY || pointY >= rectY + rectHeight) continue;
        *x = rectX;
        *y = rectY;
        *w = rectWidth;
        *h = rectHeight;
        return true;
    }
    return false;
}

bool RegionPicker::pick(CaptureMode mode, CaptureRegion *region, QString *error)
{
    const auto values = monitors(error);
    if (values.isEmpty()) return false;
    if (mode == CaptureMode::Fullscreen) {
        const Monitor *monitor = &values.first();
        for (const auto &candidate : values) if (candidate.focused) monitor = &candidate;
        monitor = capturableMonitor(values, monitor, error);
        *region = CaptureRegion{monitor->name, monitor->x, monitor->y,
                                monitor->logicalWidth, monitor->logicalHeight, monitor->scale,
                                monitor->physicalWidth, monitor->physicalHeight, mode};
        return true;
    }

    QString program;
    QStringList arguments;
    QByteArray standardInput;
    if (mode == CaptureMode::Region
        && !QStandardPaths::findExecutable(QStringLiteral("omarchy-capture-region")).isEmpty()) {
        program = QStringLiteral("omarchy-capture-region");
        arguments = {QStringLiteral("smart"), QStringLiteral("--match-monitor")};
    } else {
        program = QStringLiteral("slurp");
        arguments = {QStringLiteral("-f"), QStringLiteral("%x,%y %wx%h")};
        const QString rectangles = windowRectangles(error);
        standardInput = rectangles.toUtf8();
        if (mode == CaptureMode::Window) {
            arguments.prepend(QStringLiteral("-r"));
        }
    }
    QProcess picker;
    picker.start(program, arguments);
    if (!picker.waitForStarted(2000)) { if (error) *error = picker.errorString(); return false; }
    if (!standardInput.isEmpty()) picker.write(standardInput);
    picker.closeWriteChannel();
    if (!picker.waitForFinished(-1) || picker.exitCode() != 0) {
        if (error) *error = QStringLiteral("Selection cancelled");
        return false;
    }
    const QString selection = QString::fromUtf8(picker.readAllStandardOutput()).trimmed();
    if (selection.startsWith(QLatin1String("monitor:"))) {
        const QString name = selection.mid(8);
        for (const auto &monitor : values) if (monitor.name == name) {
            const Monitor *selected = capturableMonitor(values, &monitor, error);
            *region = CaptureRegion{selected->name, selected->x, selected->y,
                                    selected->logicalWidth, selected->logicalHeight,
                                    selected->scale, selected->physicalWidth,
                                    selected->physicalHeight, CaptureMode::Fullscreen};
            return true;
        }
        if (error) *error = QStringLiteral("Picker returned unknown monitor %1").arg(name);
        return false;
    }
    double x, y, w, h;
    if (!parseRect(selection, &x, &y, &w, &h)) {
        if (error) *error = QStringLiteral("Could not parse selection: %1").arg(selection);
        return false;
    }
    const Monitor *monitor = monitorFor(values, x, y, w, h);
    CaptureMode selectedMode = mode;
    if (mode == CaptureMode::Region && RegionPicker::isClickSelection(w, h)) {
        if (snapToWindow(QString::fromUtf8(standardInput), x, y, &x, &y, &w, &h)) {
            selectedMode = CaptureMode::Window;
        } else {
            x = monitor->x;
            y = monitor->y;
            w = monitor->logicalWidth;
            h = monitor->logicalHeight;
            selectedMode = CaptureMode::Fullscreen;
        }
    }
    if (selectedMode == CaptureMode::Fullscreen) {
        const Monitor *selected = capturableMonitor(values, monitor, error);
        *region = CaptureRegion{selected->name, selected->x, selected->y,
                                selected->logicalWidth, selected->logicalHeight,
                                selected->scale, selected->physicalWidth,
                                selected->physicalHeight, selectedMode};
    } else {
        *region = CaptureRegion{monitor->name, x, y, w, h, monitor->scale,
                                int(std::lround(w * monitor->scale)),
                                int(std::lround(h * monitor->scale)), selectedMode};
    }
    return true;
}
