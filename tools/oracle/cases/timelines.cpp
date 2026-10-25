// cliptimeline, cameratimeline and layouttimeline: every ClipTimeline function, mapCameraTime
// and sampleLayout, over crafted clip lists and fine time grids around every clip boundary.

#include "timeline.h"

#include "core/CameraTimeline.h"
#include "core/ClipTimeline.h"
#include "core/LayoutTimeline.h"

#include <QDir>

#include <algorithm>

namespace Oracle {

using Omareel::Clip;
using Omareel::ClipTimeline;

Clip clipFromJson(const QJsonObject &o)
{
    Clip clip;
    clip.id = o.value(QStringLiteral("id")).toString();
    clip.in = o.value(QStringLiteral("in")).toDouble();
    clip.out = o.value(QStringLiteral("out")).toDouble();
    clip.speed = o.value(QStringLiteral("speed")).toDouble(1.0);
    if (o.contains(QStringLiteral("layout"))) clip.layout = o.value(QStringLiteral("layout")).toString();
    if (o.contains(QStringLiteral("media"))) clip.media = o.value(QStringLiteral("media")).toString();
    if (o.contains(QStringLiteral("fit"))) clip.fit = o.value(QStringLiteral("fit")).toString();
    return clip;
}

QVector<Clip> clipsFromJson(const QJsonArray &array)
{
    QVector<Clip> clips;
    for (const QJsonValue &value : array) clips << clipFromJson(value.toObject());
    return clips;
}

QByteArray clipsText(const QVector<Clip> &clips)
{
    QList<QByteArray> items;
    for (const Clip &c : clips)
        items << quote(c.id) + ' ' + num(c.in) + ' ' + num(c.out) + ' ' + num(c.speed) + ' '
                     + quote(c.layout) + ' ' + quote(c.media) + ' ' + quote(c.fit);
    return '[' + items.join(';') + ']';
}

QVector<double> sortedUnique(QVector<double> values)
{
    std::sort(values.begin(), values.end());
    values.erase(std::unique(values.begin(), values.end()), values.end());
    return values;
}

namespace {

constexpr double Infinity = std::numeric_limits<double>::infinity();
const double NaN = std::numeric_limits<double>::quiet_NaN();

QVector<double> uniform(int denominator, int from, int to)
{
    QVector<double> values;
    for (int i = from; i <= to; ++i) values << i / double(denominator);
    return values;
}

// Where each clip starts in output time, and where the last one ends: b[0] = 0, then the
// clip lengths accumulated the way clipAt/sourceTime/split do (negative for inverted clips).
QVector<double> clipBoundaries(const QVector<Clip> &clips)
{
    QVector<double> bounds{0.0};
    double start = 0.0;
    for (const Clip &clip : clips) {
        start += (clip.out - clip.in) / std::max(0.000001, clip.speed);
        bounds << start;
    }
    return bounds;
}

QByteArray index(int value)
{
    return QByteArray::number(value);
}

void clipTimelineQueries(Text &out, const QVector<Clip> &clips)
{
    const ClipTimeline timeline(clips);
    const int count = clips.size();
    const double total = timeline.totalDuration();
    const QVector<double> bounds = clipBoundaries(clips);
    out.row({"clips", clipsText(timeline.clips())});
    out.row({"totalDuration", num(total)});

    QVector<double> grid = uniform(20, -10, int(std::ceil(20 * std::min(total, 12.0))) + 10);
    for (const double b : bounds) grid << b - 1e-9 << before(b) << b << after(b) << b + 1e-9;
    grid = sortedUnique(grid) << -Infinity << Infinity << NaN;
    for (const double t : grid) {
        double start = -7.0;
        const int at = timeline.clipAt(t, &start);
        out.row({"at", num(t), index(at), num(start), num(timeline.sourceTime(t)),
                 num(timeline.recordingSourceTime(t))});
    }

    for (int i = -1; i <= count; ++i)
        for (const double sourceDuration : {8.0, 100.0}) {
            const auto bounds = timeline.recordingBounds(i, sourceDuration);
            out.row({"recordingBounds", index(i), num(sourceDuration), num(bounds.first), num(bounds.second)});
        }

    QVector<double> sources = uniform(2, -2, 24);
    for (const Clip &clip : clips)
        sources << before(clip.in) << clip.in << after(clip.in) << before(clip.out) << clip.out << after(clip.out);
    sources = sortedUnique(sources) << -Infinity << Infinity << NaN;
    for (int i = -1; i <= count; ++i)
        for (const double source : sources)
            out.row({"outputTime", index(i), num(source), num(timeline.outputTime(i, source))});

    QVector<double> ends{-1.0, 0.0, 100.0};
    for (const Clip &clip : clips) ends << clip.in << clip.out << (clip.in + clip.out) / 2;
    ends = sortedUnique(ends);
    for (const double from : ends)
        for (const double to : ends) {
            const auto range = timeline.outputRange(from, to);
            out.row({"outputRange", num(from), num(to), num(range.first), num(range.second)});
        }
}

void clipTimelineEdits(Text &out, const QVector<Clip> &clips, const QJsonArray &splitSequence)
{
    const ClipTimeline timeline(clips);
    const int count = clips.size();
    const double total = timeline.totalDuration();
    const QVector<double> bounds = clipBoundaries(clips);

    QVector<double> splits = uniform(4, -2, int(std::ceil(4 * std::min(total, 12.0))) + 2);
    for (const double b : bounds) {
        splits << before(b - 1e-9) << b - 1e-9 << after(b - 1e-9) << b;
        splits << before(b + 1e-9) << b + 1e-9 << after(b + 1e-9);
    }
    splits = sortedUnique(splits) << -Infinity << Infinity << NaN;
    for (const double t : splits) {
        ClipTimeline edited = timeline;
        const bool ok = edited.split(t);
        out.row({"split", num(t), boolText(ok), clipsText(edited.clips())});
    }
    // One timeline split again and again: ids count up in the object.
    ClipTimeline sequence = timeline;
    for (const QJsonValue &value : splitSequence) {
        const bool ok = sequence.split(value.toDouble());
        out.row({"splitSequence", num(value.toDouble()), boolText(ok), clipsText(sequence.clips())});
    }

    const QList<QPair<double, double>> trims{
        {0.0, 1.0}, {0.5, 7.5}, {-0.0, 2.0}, {-1e-300, 2.0}, {2.0, 2.0}, {3.0, 2.0},
        {0.0, 1e-300}, {5.0, Infinity}, {NaN, 1.0}, {1.0, NaN}};
    for (int i = -1; i <= count; ++i)
        for (const auto &[newIn, newOut] : trims) {
            ClipTimeline edited = timeline;
            const bool ok = edited.trim(i, newIn, newOut);
            out.row({"trim", index(i), num(newIn), num(newOut), boolText(ok), clipsText(edited.clips())});
        }

    for (int i = -1; i <= count; ++i) {
        ClipTimeline edited = timeline;
        const bool ok = edited.remove(i);
        out.row({"remove", index(i), boolText(ok), clipsText(edited.clips())});
    }

    QVector<double> points{-1.0, 0.0, total, total + 1.0};
    for (int k = 1; k < count; ++k) points << bounds[k] - 0.05 << bounds[k] << bounds[k] + 0.05;
    for (int k = 1; k <= count; ++k) points << (bounds[k - 1] + bounds[k]) / 2;
    points = sortedUnique(points);
    QList<QPair<double, double>> ranges;
    for (const double from : points)
        for (const double to : points)
            if (from < to) ranges << qMakePair(from, to);
    ranges << qMakePair(total, 0.0) << qMakePair(NaN, total) << qMakePair(0.0, NaN)
           << qMakePair(-Infinity, total) << qMakePair(0.0, Infinity) << qMakePair(-Infinity, Infinity);
    const QString existing = clips.isEmpty() ? QStringLiteral("c1") : clips.first().id;
    for (const auto &[from, to] : ranges) {
        ClipTimeline withId = timeline, withoutId = timeline, withExisting = timeline;
        const bool ok = withId.deleteRange(from, to, QStringLiteral("r"));
        const bool okWithout = withoutId.deleteRange(from, to, QString());
        const bool okExisting = withExisting.deleteRange(from, to, existing);
        // The README promises the other two remainder ids either fail and leave the clips
        // alone or give exactly the "r" result; check it rather than assume it.
        const QPair<const ClipTimeline *, bool> others[] = {{&withoutId, okWithout}, {&withExisting, okExisting}};
        for (const auto &[edited, result] : others) {
            const QVector<Clip> &expected = result ? withId.clips() : clips;
            if ((result && !ok) || clipsText(edited->clips()) != clipsText(expected))
                fail(QStringLiteral("deleteRange(%1, %2) breaks the remainder-id rule")
                         .arg(QString::fromLatin1(num(from)), QString::fromLatin1(num(to))));
        }
        out.row({"deleteRange", num(from), num(to), boolText(ok), boolText(okWithout),
                 boolText(okExisting), clipsText(withId.clips())});
    }
}

} // namespace

void runClipTimeline(const Context &context)
{
    const QJsonObject cases = readJson(QDir(context.corpus).filePath(QStringLiteral("cliptimeline/cases.json"))).toObject();
    const QJsonObject timelines = cases.value(QStringLiteral("timelines")).toObject();
    const QJsonObject sequences = cases.value(QStringLiteral("splitSequences")).toObject();
    Text queries, edits;
    for (auto it = timelines.begin(); it != timelines.end(); ++it) {
        const QVector<Clip> clips = clipsFromJson(it.value().toArray());
        queries << "== " << it.key().toUtf8() << "\n";
        clipTimelineQueries(queries, clips);
        edits << "== " << it.key().toUtf8() << "\n";
        clipTimelineEdits(edits, clips, sequences.value(it.key()).toArray());
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("queries.txt")), queries.data());
    writeFile(QDir(context.output).filePath(QStringLiteral("edits.txt")), edits.data());
}

void runCameraTimeline(const Context &context)
{
    Text out;
    for (const double offset : {-2.0, -0.5, 0.0, 0.25, 1.5, 3.0, NaN})
        for (const double duration : {-1.0, 0.0, 0.5, 4.0, 8.0, Infinity, NaN}) {
            out << "== offset=" << num(offset) << " duration=" << num(duration) << "\n";
            QVector<double> times = uniform(4, -12, 48);
            for (const double edge : {offset, offset + duration})
                if (std::isfinite(edge)) times << before(edge) << edge << after(edge);
            times = sortedUnique(times) << -0.0 << -Infinity << Infinity << NaN;
            for (const double t : times) {
                const Omareel::CameraTime mapped = Omareel::mapCameraTime(t, offset, duration);
                out.row({num(t), num(mapped.seconds), boolText(mapped.beforeStart), boolText(mapped.beyondEnd)});
            }
        }
    writeFile(QDir(context.output).filePath(QStringLiteral("map.txt")), out.data());
}

void runLayoutTimeline(const Context &context)
{
    const QJsonObject timelines = readJson(QDir(context.corpus).filePath(QStringLiteral("layouttimeline/cases.json")))
                                      .toObject().value(QStringLiteral("timelines")).toObject();
    struct Variant { bool camera; bool bubble; double transition; bool explicitTransition; };
    QList<Variant> variants{{false, true, 0.5, false}};
    for (const bool bubble : {false, true}) {
        variants << Variant{true, bubble, 0.5, false};
        for (const double transition : {0.0, 0.25, 1.5}) variants << Variant{true, bubble, transition, true};
    }
    Text out;
    for (auto it = timelines.begin(); it != timelines.end(); ++it) {
        const QVector<Clip> clips = clipsFromJson(it.value().toArray());
        // Clip starts as sampleLayout accumulates them: lengths are clamped at 0.
        QVector<double> starts{0.0};
        for (const Clip &clip : clips)
            starts << starts.last() + std::max(0.0, (clip.out - clip.in) / std::max(0.000001, clip.speed));
        for (const Variant &v : variants) {
            out << "== " << it.key().toUtf8() << " cameraAvailable=" << boolText(v.camera)
                << " bubbleEnabled=" << boolText(v.bubble) << " transition="
                << (v.explicitTransition ? num(v.transition) : QByteArray("default")) << "\n";
            QVector<double> times = uniform(20, -10, int(std::ceil(20 * std::min(starts.last(), 12.0))) + 10);
            for (const double s : starts)
                times << before(s) << s << after(s) << before(s + v.transition) << s + v.transition
                      << after(s + v.transition);
            times = sortedUnique(times) << -Infinity << Infinity << NaN;
            for (const double t : times) {
                const Omareel::LayoutSample sample = v.explicitTransition
                    ? Omareel::sampleLayout(clips, t, v.camera, v.bubble, v.transition)
                    : Omareel::sampleLayout(clips, t, v.camera, v.bubble);
                out.row({num(t), num(sample.cameraFull), num(sample.bubble)});
            }
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("sample.txt")), out.data());
}

} // namespace Oracle
