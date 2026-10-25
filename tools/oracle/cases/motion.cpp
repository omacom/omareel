// cursorpath and zoomtimeline: CursorPath::decimate / positionAt and every ZoomTimeline
// function, over crafted inputs, the synthetic recording, and the zooms of a few projects.

#include "motion.h"

#include "core/Project.h"
#include "core/ZoomTimeline.h"

#include <QDir>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

#include <algorithm>
#include <limits>

namespace Oracle {

using Omareel::CursorSample;
using Omareel::ZoomSegment;

Omareel::InputLog syntheticLog(const Context &context)
{
    QString error;
    const Omareel::InputLog log = Omareel::InputLog::loadBundle(
        QDir(context.corpus).filePath(QStringLiteral("inputlog/bundles/synthetic")), -1.0, &error);
    if (!error.isEmpty() || log.events().isEmpty()) fail(QStringLiteral("synthetic bundle: %1").arg(error));
    return log;
}

QVector<CursorSample> syntheticSamples(const Context &context, bool normalized)
{
    QVector<CursorSample> samples;
    for (const auto &event : syntheticLog(context).moves()) {
        samples << CursorSample{event.time, normalized
            ? QPointF(event.position.x() / 640.0, event.position.y() / 360.0) : event.position};
    }
    return samples;
}

QVector<double> syntheticClickTimes(const Context &context)
{
    QVector<double> times;
    for (const auto &event : syntheticLog(context).clickDowns(true)) times << event.time;
    return times;
}

namespace {

QVector<CursorSample> samplesFromJson(const QJsonArray &array)
{
    QVector<CursorSample> samples;
    for (const QJsonValue &value : array) {
        const QJsonArray triple = value.toArray();
        samples << CursorSample{triple.at(0).toDouble(), QPointF(triple.at(1).toDouble(), triple.at(2).toDouble())};
    }
    return samples;
}

QByteArray sampleIndices(const QVector<CursorSample> &result, const QVector<CursorSample> &raw)
{
    QList<QByteArray> indices;
    qsizetype from = 0;
    for (const CursorSample &sample : result) {
        qsizetype found = -1;
        for (qsizetype i = from; i < raw.size(); ++i) {
            if (raw[i].time == sample.time && raw[i].position.x() == sample.position.x()
                && raw[i].position.y() == sample.position.y()) { found = i; break; }
        }
        if (found < 0) return "unmatched";
        indices << QByteArray::number(found);
        from = found + 1;
    }
    return indices.isEmpty() ? QByteArray("-") : indices.join(',');
}

QByteArray point(const QPointF &p)
{
    return num(p.x()) + '\t' + num(p.y());
}

QByteArray segmentRow(const ZoomSegment &s)
{
    return QList<QByteArray>{quote(s.id), num(s.start), num(s.end), num(s.level),
                             boolText(s.automaticTarget), num(s.target.x()), num(s.target.y())}
        .join('\t');
}

QVector<ZoomSegment> segmentsFromJson(const QJsonArray &array)
{
    QVector<ZoomSegment> segments;
    for (const QJsonValue &value : array) {
        const QJsonObject o = value.toObject();
        segments << ZoomSegment{o.value(QStringLiteral("id")).toString(),
                                o.value(QStringLiteral("start")).toDouble(),
                                o.value(QStringLiteral("end")).toDouble(),
                                o.value(QStringLiteral("level")).toDouble(),
                                o.value(QStringLiteral("auto")).toBool(),
                                QPointF(o.value(QStringLiteral("x")).toDouble(),
                                        o.value(QStringLiteral("y")).toDouble())};
    }
    return segments;
}

QVector<double> timeGrid()
{
    QVector<double> times;
    for (int i = -15; i <= 255; ++i) times << i / 30.0;
    return times;
}

} // namespace

void runCursorPath(const Context &context)
{
    const QJsonObject cases = readJson(QDir(context.corpus).filePath(QStringLiteral("cursorpath/cases.json"))).toObject();
    QList<QPair<QString, QVector<CursorSample>>> sets;
    const QJsonObject crafted = cases.value(QStringLiteral("samples")).toObject();
    for (auto it = crafted.begin(); it != crafted.end(); ++it)
        sets << qMakePair(it.key(), samplesFromJson(it.value().toArray()));
    sets << qMakePair(QStringLiteral("synthetic-normalized"), syntheticSamples(context, true));
    sets << qMakePair(QStringLiteral("synthetic-pixels"), syntheticSamples(context, false));

    Text decimate;
    const QJsonArray parameters = cases.value(QStringLiteral("decimate")).toArray();
    for (const auto &[name, raw] : sets) {
        for (const QJsonValue &parameter : parameters) {
            const double fps = parameter.toArray().at(0).toDouble();
            const double minDistance = parameter.toArray().at(1).toDouble();
            const auto result = Omareel::CursorPath::decimate(raw, fps, minDistance);
            decimate.row({name.toUtf8(), num(fps), num(minDistance),
                          QByteArray::number(result.size()), sampleIndices(result, raw)});
        }
        // The default minDistance argument.
        const auto result = Omareel::CursorPath::decimate(raw, 60.0);
        decimate.row({name.toUtf8(), num(60.0), "default", QByteArray::number(result.size()),
                      sampleIndices(result, raw)});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("decimate.txt")), decimate.data());

    Text positions;
    QVector<double> queries;
    for (const QJsonValue &value : cases.value(QStringLiteral("positionAt")).toArray()) queries << value.toDouble();
    for (const auto &[name, raw] : sets) {
        QVector<double> times = queries;
        if (name.startsWith(QLatin1String("synthetic"))) {
            times.clear();
            for (int i = -12; i <= 972; ++i) times << i / 120.0;
        }
        for (const double time : times)
            positions.row({name.toUtf8(), num(time), point(Omareel::CursorPath::positionAt(raw, time))});
    }
    // MotionTrack's input: the synthetic samples decimated at the capture rate with a
    // one-pixel threshold in normalised units.
    const auto decimated = Omareel::CursorPath::decimate(syntheticSamples(context, true), 60.0, 1.0 / 640.0);
    for (int i = -12; i <= 972; ++i)
        positions.row({"synthetic-normalized-decimated", num(i / 120.0),
                       point(Omareel::CursorPath::positionAt(decimated, i / 120.0))});
    writeFile(QDir(context.output).filePath(QStringLiteral("position-at.txt")), positions.data());
}

void runZoomTimeline(const Context &context)
{
    const QJsonObject cases = readJson(QDir(context.corpus).filePath(QStringLiteral("zoomtimeline/cases.json"))).toObject();

    Text generate;
    QList<QPair<QString, QVector<ZoomSegment>>> segmentSets;
    const QJsonObject generateCases = cases.value(QStringLiteral("generate")).toObject();
    const auto emitGenerate = [&](const QString &name, const QVector<double> &clicks, double duration) {
        const auto segments = Omareel::ZoomTimeline::generate(clicks, duration);
        generate << "== " << name.toUtf8() << " duration=" << num(duration) << "\n";
        for (const auto &segment : segments) generate << segmentRow(segment) << "\n";
        return segments;
    };
    for (auto it = generateCases.begin(); it != generateCases.end(); ++it) {
        QVector<double> clicks;
        for (const QJsonValue &value : it.value().toObject().value(QStringLiteral("clicks")).toArray())
            clicks << value.toDouble();
        for (const QJsonValue &duration : it.value().toObject().value(QStringLiteral("durations")).toArray())
            emitGenerate(it.key(), clicks, duration.toDouble());
    }
    const QVector<double> clicks = syntheticClickTimes(context);
    QList<QByteArray> clickCells{"synthetic clicks"};
    for (const double click : clicks) clickCells << num(click);
    generate.row(clickCells);
    for (const double duration : {8.0, 5.0, 3.0})
        emitGenerate(QStringLiteral("synthetic"), clicks, duration);
    writeFile(QDir(context.output).filePath(QStringLiteral("generate.txt")), generate.data());

    segmentSets << qMakePair(QStringLiteral("synthetic-generated"), Omareel::ZoomTimeline::generate(clicks, 8.0));
    const QJsonObject crafted = cases.value(QStringLiteral("segments")).toObject();
    for (auto it = crafted.begin(); it != crafted.end(); ++it)
        segmentSets << qMakePair(it.key(), segmentsFromJson(it.value().toArray()));

    const QVector<CursorSample> synthetic = Omareel::CursorPath::decimate(
        syntheticSamples(context, true), 60.0, 1.0 / 640.0);
    const QJsonObject cursorCases = readJson(QDir(context.corpus).filePath(QStringLiteral("cursorpath/cases.json")))
                                        .toObject().value(QStringLiteral("samples")).toObject();
    const QList<QPair<QString, QVector<CursorSample>>> sampleSets{
        {QStringLiteral("synthetic"), synthetic},
        {QStringLiteral("random-walk-normalized"), samplesFromJson(cursorCases.value(QStringLiteral("random-walk-normalized")).toArray())},
        {QStringLiteral("empty"), {}},
        {QStringLiteral("single"), samplesFromJson(cursorCases.value(QStringLiteral("single")).toArray())},
    };

    Text target;
    const auto emitTargets = [&](const QString &segmentsName, const QVector<ZoomSegment> &segments,
                                 const QString &samplesName, const QVector<CursorSample> &samples,
                                 const Omareel::ZoomStyle &style, const QByteArray &styleName) {
        target << "== segments=" << segmentsName.toUtf8() << " samples=" << samplesName.toUtf8()
               << " snapToEdgesRatio=" << styleName << "\n";
        for (const double time : timeGrid()) {
            const Omareel::ZoomFrame frame = Omareel::ZoomTimeline::targetAt(segments, samples, time, style);
            target.row({num(time), num(frame.scale), point(frame.center)});
        }
    };
    for (const QJsonValue &ratio : cases.value(QStringLiteral("snapRatios")).toArray()) {
        Omareel::ZoomStyle style;
        style.snapToEdgesRatio = ratio.toDouble();
        emitTargets(segmentSets.first().first, segmentSets.first().second,
                    QStringLiteral("synthetic"), synthetic, style, num(style.snapToEdgesRatio));
    }
    for (const auto &[segmentsName, segments] : segmentSets.mid(1))
        for (const auto &[samplesName, samples] : sampleSets)
            emitTargets(segmentsName, segments, samplesName, samples, Omareel::ZoomStyle{}, "default");
    // The default style argument.
    target << "== segments=synthetic-generated samples=synthetic style=<omitted>\n";
    for (const double time : timeGrid()) {
        const auto frame = Omareel::ZoomTimeline::targetAt(segmentSets.first().second, synthetic, time);
        target.row({num(time), num(frame.scale), point(frame.center)});
    }
    // Zooms and zoom styles as projects store them.
    for (const char *file : {"crafted-edited.json", "crafted-clamps.json", "crafted-wrong-types.json"}) {
        const Omareel::Project project = Omareel::Project::fromJson(
            QJsonDocument::fromJson(readFile(QDir(context.corpus).filePath(QStringLiteral("project/") + QLatin1String(file)))).object());
        emitTargets(QStringLiteral("project:") + QLatin1String(file), project.zooms, QStringLiteral("synthetic"),
                    synthetic, project.zoomStyle, num(project.zoomStyle.snapToEdgesRatio));
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("target-at.txt")), target.data());

    Text clamp;
    const QJsonObject clampCases = cases.value(QStringLiteral("clampCenter")).toObject();
    for (const QJsonValue &p : clampCases.value(QStringLiteral("points")).toArray()) {
        const QPointF center(p.toArray().at(0).toDouble(), p.toArray().at(1).toDouble());
        for (const QJsonValue &level : clampCases.value(QStringLiteral("levels")).toArray())
            clamp.row({point(center), num(level.toDouble()),
                       point(Omareel::ZoomTimeline::clampCenter(center, level.toDouble()))});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("clamp-center.txt")), clamp.data());

    Text json;
    for (const auto &[name, segments] : segmentSets)
        for (const auto &segment : segments)
            json.row({name.toUtf8(), quote(segment.id), wrappedJson(Omareel::ZoomTimeline::targetToJson(segment))});
    for (const ZoomSegment &segment : {ZoomSegment{QStringLiteral("auto-with-target"), 0, 1, 2, true, {0.1, 0.2}},
                                       ZoomSegment{QStringLiteral("manual-negative-zero"), 0, 1, 2, false, {-0.0, 1e-300}},
                                       ZoomSegment{QStringLiteral("manual-big"), 0, 1, 2, false, {1e21, 123456789.125}},
                                       ZoomSegment{QStringLiteral("manual-nan"), 0, 1, 2, false,
                                                   {std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity()}}})
        json.row({"explicit", quote(segment.id), wrappedJson(Omareel::ZoomTimeline::targetToJson(segment))});
    writeFile(QDir(context.output).filePath(QStringLiteral("target-to-json.txt")), json.data());
}

} // namespace Oracle
