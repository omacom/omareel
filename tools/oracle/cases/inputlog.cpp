// inputlog: InputLog::load and InputLog::loadBundle with every accessor.

#include "common.h"
#include "motion.h"

#include "core/InputLog.h"

#include <QDir>
#include <QFile>

namespace Oracle {

QByteArray kindName(Omareel::InputKind kind)
{
    switch (kind) {
    case Omareel::InputKind::Move: return "Move";
    case Omareel::InputKind::ButtonDown: return "ButtonDown";
    case Omareel::InputKind::ButtonUp: return "ButtonUp";
    case Omareel::InputKind::Scroll: return "Scroll";
    case Omareel::InputKind::KeyDown: return "KeyDown";
    case Omareel::InputKind::KeyUp: return "KeyUp";
    }
    return "Kind" + QByteArray::number(int(kind));
}

QList<InputLogCase> inputLogCases(const Context &context)
{
    const QString root = QDir(context.corpus).filePath(QStringLiteral("inputlog"));
    QList<InputLogCase> cases;
    const QList<double> durations{-1.0, 0.0, 0.5, 2.0};
    for (const QString &file : sortedEntries(root, false)) {
        const QString path = QDir(root).filePath(file);
        for (const qint64 first : {qint64(1000000), qint64(0), qint64(2000000)}) {
            for (const double duration : durations) {
                if (first != 1000000 && duration != -1.0) continue;
                cases << InputLogCase{QStringLiteral("load %1 first=%2 duration=%3")
                                          .arg(file).arg(first).arg(QString::fromLatin1(num(duration))),
                                      false, path, first, duration};
            }
        }
    }
    cases << InputLogCase{QStringLiteral("load missing.jsonl first=1000000 duration=-1"), false,
                          QDir(root).filePath(QStringLiteral("missing.jsonl")), 1000000, -1.0};
    cases << InputLogCase{QStringLiteral("load <directory> first=1000000 duration=-1"), false,
                          QDir(root).filePath(QStringLiteral("bundles")), 1000000, -1.0};
    cases << InputLogCase{QStringLiteral("load <empty path> first=1000000 duration=-1"), false,
                          QString(), 1000000, -1.0};
    const QString bundles = QDir(root).filePath(QStringLiteral("bundles"));
    for (const QString &bundle : sortedEntries(bundles, true)) {
        for (const double duration : {-1.0, 3.0, 0.0}) {
            cases << InputLogCase{QStringLiteral("loadBundle %1 duration=%2")
                                      .arg(bundle, QString::fromLatin1(num(duration))),
                                  true, QDir(bundles).filePath(bundle), 0, duration};
        }
    }
    cases << InputLogCase{QStringLiteral("loadBundle missing-bundle duration=-1"), true,
                          QDir(bundles).filePath(QStringLiteral("missing-bundle")), 0, -1.0};
    cases << InputLogCase{QStringLiteral("loadBundle synthetic/ duration=-1"), true,
                          QDir(bundles).filePath(QStringLiteral("synthetic")) + QLatin1Char('/'), 0, -1.0};
    return cases;
}

Omareel::InputLog loadCase(const InputLogCase &c, QString *error)
{
    return c.bundle ? Omareel::InputLog::loadBundle(c.path, c.duration, error)
                    : Omareel::InputLog::load(c.path, c.first, c.duration, error);
}

namespace {

QByteArray indices(const QVector<Omareel::InputEvent> &subset,
                   const QVector<Omareel::InputEvent> &events)
{
    // Every event is a distinct copy, so position in the full list identifies it.
    QList<QByteArray> result;
    qsizetype from = 0;
    for (const auto &event : subset) {
        for (qsizetype i = from; i < events.size(); ++i) {
            const auto &candidate = events.at(i);
            if (candidate.monotonicUs == event.monotonicUs && candidate.kind == event.kind
                && candidate.position.x() == event.position.x()
                && candidate.position.y() == event.position.y() && candidate.button == event.button
                && candidate.keyName == event.keyName && candidate.keyCode == event.keyCode) {
                result << QByteArray::number(i);
                from = i + 1;
                break;
            }
        }
    }
    return result.isEmpty() ? QByteArray("-") : result.join(',');
}

} // namespace

QByteArray eventRow(const Omareel::InputEvent &event)
{
    return QList<QByteArray>{QByteArray::number(event.monotonicUs), num(event.time),
                             kindName(event.kind), num(event.position.x()),
                             num(event.position.y()), quote(event.button), num(event.dx),
                             num(event.dy), QByteArray::number(event.keyCode),
                             quote(event.keyName), quoteList(event.modifiers)}
        .join('\t');
}

void runInputLog(const Context &context)
{
    Text out;
    for (const InputLogCase &c : inputLogCases(context)) {
        QString error = QStringLiteral("<untouched>");
        const Omareel::InputLog log = loadCase(c, &error);
        out << "== " << c.label.toUtf8() << "\n";
        out.row({"error", quote(sanitize(error))});
        const auto &events = log.events();
        out.row({"count", QByteArray::number(events.size())});
        for (const auto &event : events) out << eventRow(event) << "\n";
        out.row({"moves", indices(log.moves(), events)});
        out.row({"clickDowns", indices(log.clickDowns(false), events)});
        out.row({"clickDowns(leftRightOnly)", indices(log.clickDowns(true), events)});
        out.row({"keyCount", QByteArray::number(log.keyCount())});
        // A null error pointer must not change the result.
        const Omareel::InputLog again = loadCase(c, nullptr);
        if (again.events().size() != events.size()) out.row({"error=nullptr changes the result"});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("cases.txt")), out.data());
}

} // namespace Oracle
