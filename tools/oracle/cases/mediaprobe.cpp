// mediaprobe: probeVideo and probeFrameTimes (src/render/MediaProbe.cpp) over recorded and
// crafted ffprobe answers, which tests/golden/shims/ffprobe replays; the real ffprobe never runs.

#include "common.h"

#include "render/MediaProbe.h"

#include <QDir>
#include <QFile>

namespace Oracle {

namespace {

QByteArray probeRow(const QByteArray &label, const QString &path)
{
    QString error = QStringLiteral("<untouched>");
    const Omareel::VideoInfo info = Omareel::probeVideo(path, &error);
    return QList<QByteArray>{label, QByteArray::number(info.width), QByteArray::number(info.height),
                             num(info.duration), num(info.fps), boolText(info.audio),
                             boolText(info.isValid()), quote(sanitize(error))}.join('\t');
}

QByteArray framesRow(const QByteArray &label, const QString &path)
{
    const QList<double> times = Omareel::probeFrameTimes(path);
    QList<QByteArray> cells{label, QByteArray::number(times.size())};
    for (const double time : times) cells << num(time);
    return cells.join('\t');
}

} // namespace

void runMediaProbe(const Context &context)
{
    const QString recordings = QDir(context.corpus).filePath(QStringLiteral("mediaprobe/ffprobe"));
    qputenv("OMAREEL_ORACLE_FFPROBE_DIR", QFile::encodeName(recordings));
    // probeVideo and probeFrameTimes only call ffprobe for a path that is a file, and the shim
    // only looks at the file name, so empty files stand in for the recorded media.
    const QString files = QDir(context.scratch).filePath(QStringLiteral("files"));
    QDir().mkpath(files);
    Text probe, frames;
    const auto both = [&](const QByteArray &label, const QString &path) {
        probe << probeRow(label, path) << "\n";
        frames << framesRow(label, path) << "\n";
    };
    for (const QString &name : sortedEntries(recordings, true)) {
        const QString path = QDir(files).filePath(name);
        writeFile(path, QByteArray());
        both(name.toUtf8(), path);
    }
    both("<missing file>", QDir(files).filePath(QStringLiteral("missing.mp4")));
    QDir().mkpath(QDir(files).filePath(QStringLiteral("directory.mp4")));
    both("<directory>", QDir(files).filePath(QStringLiteral("directory.mp4")));
    both("<empty path>", QString());
    // Relative to the working directory ($HOME): ffprobe gets the path as it was given.
    both("<relative path>", QStringLiteral("oracle-scratch/mediaprobe/files/testsrc-320x240-30.mp4"));
    const QByteArray path = qgetenv("PATH");
    const QString emptyBin = QDir(context.scratch).filePath(QStringLiteral("empty-bin"));
    QDir().mkpath(emptyBin);
    qputenv("PATH", QFile::encodeName(emptyBin));
    both("<ffprobe not on PATH>", QDir(files).filePath(QStringLiteral("testsrc-320x240-30.mp4")));
    qputenv("PATH", path);
    qunsetenv("OMAREEL_ORACLE_FFPROBE_DIR");
    writeFile(QDir(context.output).filePath(QStringLiteral("probe-video.txt")), probe.data());
    writeFile(QDir(context.output).filePath(QStringLiteral("probe-frame-times.txt")), frames.data());
}

} // namespace Oracle
