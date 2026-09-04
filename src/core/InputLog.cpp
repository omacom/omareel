#include "InputLog.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

using namespace Omareel;

static InputKind kindFor(const QString &kind)
{
    if (kind == QLatin1String("d")) return InputKind::ButtonDown;
    if (kind == QLatin1String("u")) return InputKind::ButtonUp;
    if (kind == QLatin1String("s")) return InputKind::Scroll;
    if (kind == QLatin1String("kd")) return InputKind::KeyDown;
    if (kind == QLatin1String("ku")) return InputKind::KeyUp;
    return InputKind::Move;
}

InputLog InputLog::load(const QString &inputPath, qint64 firstFrameUs,
                        double duration, QString *error)
{
    InputLog result;
    QFile file(inputPath);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        if (error) *error = file.errorString();
        return result;
    }
    int lineNumber = 0;
    while (!file.atEnd()) {
        ++lineNumber;
        const QByteArray line = file.readLine().trimmed();
        if (line.isEmpty()) continue;
        QJsonParseError parseError;
        const auto doc = QJsonDocument::fromJson(line, &parseError);
        if (!doc.isObject()) {
            if (error) *error = QStringLiteral("Invalid JSON on line %1: %2")
                                    .arg(lineNumber).arg(parseError.errorString());
            result.m_events.clear();
            return result;
        }
        const auto object = doc.object();
        InputEvent event;
        event.monotonicUs = object.value("t").toVariant().toLongLong();
        event.time = double(event.monotonicUs - firstFrameUs) / 1000000.0;
        if (event.time < 0.0 || (duration >= 0.0 && event.time > duration)) continue;
        event.kind = kindFor(object.value("k").toString());
        event.position = QPointF(object.value("x").toDouble(), object.value("y").toDouble());
        event.button = object.value("b").toString();
        event.dx = object.value("dx").toDouble();
        event.dy = object.value("dy").toDouble();
        event.keyCode = object.value("code").toInt();
        event.keyName = object.value("name").toString();
        for (const auto &mod : object.value("mods").toArray()) event.modifiers << mod.toString();
        result.m_events.push_back(event);
    }
    if (error) error->clear();
    return result;
}

InputLog InputLog::loadBundle(const QString &bundlePath, double duration, QString *error)
{
    QFile capture(bundlePath + QStringLiteral("/capture.json"));
    if (!capture.open(QIODevice::ReadOnly)) {
        if (error) *error = capture.errorString();
        return {};
    }
    const auto object = QJsonDocument::fromJson(capture.readAll()).object();
    const qint64 first = object.value("first_frame_us").toVariant().toLongLong();
    if (duration < 0.0) {
        const qint64 stopped = object.value("stopped_us").toVariant().toLongLong();
        if (stopped > first) duration = double(stopped - first) / 1000000.0;
    }
    return load(bundlePath + QStringLiteral("/input.jsonl"), first, duration, error);
}

QVector<InputEvent> InputLog::moves() const
{
    QVector<InputEvent> values;
    for (const auto &event : m_events) if (event.kind == InputKind::Move) values << event;
    return values;
}

QVector<InputEvent> InputLog::clickDowns(bool leftRightOnly) const
{
    QVector<InputEvent> values;
    for (const auto &event : m_events) {
        if (event.kind != InputKind::ButtonDown) continue;
        if (leftRightOnly && event.button != QLatin1String("left")
            && event.button != QLatin1String("right")) continue;
        values << event;
    }
    return values;
}

int InputLog::keyCount() const
{
    int count = 0;
    for (const auto &event : m_events)
        if (event.kind == InputKind::KeyDown) ++count;
    return count;
}
