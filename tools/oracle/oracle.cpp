// omareel-oracle <corpus-dir> <output-dir>
//
// Runs the C++ core of rust-migration-base over tests/golden/corpus and writes what it produced
// under <output-dir>, one directory per family. tools/oracle/generate-golden.sh builds it from the
// tag and runs it in a hermetic environment; tests/golden/README.md describes every file.

#include "common.h"

#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>

#include <clocale>
#include <cstdlib>
#include <iostream>

using namespace Oracle;

// Qt's warnings (QColor range checks and the like) are expected while the corpus probes edge
// cases. They are dropped so they neither clutter the run nor land in the journal; set
// OMAREEL_ORACLE_VERBOSE=1 to see them.
static void messageHandler(QtMsgType type, const QMessageLogContext &, const QString &message)
{
    static const bool verbose = qgetenv("OMAREEL_ORACLE_VERBOSE") == "1";
    if (verbose || type == QtFatalMsg) std::cerr << message.toStdString() << std::endl;
    if (type == QtFatalMsg) std::abort();
}

int main(int argc, char **argv)
{
    if (argc != 3) {
        std::cerr << "usage: omareel-oracle <corpus-dir> <output-dir>" << std::endl;
        return 2;
    }
    const QString corpusArgument = QString::fromLocal8Bit(argv[1]);
    const QString outputArgument = QString::fromLocal8Bit(argv[2]);
    if (qgetenv("OMAREEL_ORACLE_HERMETIC") != "1")
        fail(QStringLiteral("refusing to run outside a hermetic environment: it writes under $HOME. "
                            "Use tools/oracle/generate-golden.sh, or set OMAREEL_ORACLE_HERMETIC=1 "
                            "with HOME and the XDG directories pointing at scratch directories."));

    qInstallMessageHandler(messageHandler);
    QGuiApplication app(argc, argv);
    // printf("%.17g") must use a '.' whatever locale Qt picked up.
    std::setlocale(LC_NUMERIC, "C");

    Context context;
    context.corpus = QFileInfo(corpusArgument).absoluteFilePath();
    context.output = QFileInfo(outputArgument).absoluteFilePath();
    if (!QFileInfo(context.corpus).isDir()) fail(QStringLiteral("no corpus at %1").arg(context.corpus));
    if (QFileInfo::exists(context.output) && !QDir(context.output).isEmpty())
        fail(QStringLiteral("output directory %1 is not empty").arg(context.output));
    QDir().mkpath(context.output);
    captureSanitizeRoots(context.corpus);

    const QString home = QDir::homePath();
    if (!QDir::setCurrent(home)) fail(QStringLiteral("cannot enter %1").arg(home));
    const QString scratchRoot = QDir(home).filePath(QStringLiteral("oracle-scratch"));
    QDir(scratchRoot).removeRecursively();

    const QList<QPair<QString, Family>> families{
        {QStringLiteral("qjson"), runQJson},
        {QStringLiteral("qnumber"), runQNumber},
        {QStringLiteral("qround"), runQRound},
        {QStringLiteral("qcolor"), runQColor},
        {QStringLiteral("easing"), runEasing},
        {QStringLiteral("inputlog"), runInputLog},
        {QStringLiteral("cursorpath"), runCursorPath},
        {QStringLiteral("zoomtimeline"), runZoomTimeline},
        {QStringLiteral("project"), runProject},
        {QStringLiteral("omarchypaths"), runOmarchyPaths},
        {QStringLiteral("theme"), runTheme},
        {QStringLiteral("cliptimeline"), runClipTimeline},
        {QStringLiteral("cameratimeline"), runCameraTimeline},
        {QStringLiteral("layouttimeline"), runLayoutTimeline},
        {QStringLiteral("keystroketrack"), runKeystrokeTrack},
        {QStringLiteral("motiontrack"), runMotionTrack},
        {QStringLiteral("recordingpreferences"), runRecordingPreferences},
        {QStringLiteral("mediaprobe"), runMediaProbe},
        {QStringLiteral("composition"), runComposition},
    };
    // For development only: OMAREEL_ORACLE_ONLY=qjson,theme runs a subset.
    const QStringList only = qEnvironmentVariable("OMAREEL_ORACLE_ONLY")
                                 .split(QLatin1Char(','), Qt::SkipEmptyParts);
    for (const auto &[name, run] : families) {
        if (!only.isEmpty() && !only.contains(name)) continue;
        Context family = context;
        family.output = QDir(context.output).filePath(name);
        family.scratch = QDir(scratchRoot).filePath(name);
        QDir().mkpath(family.output);
        QDir().mkpath(family.scratch);
        QDir::setCurrent(home);
        run(family);
    }
    QDir::setCurrent(home);
    QDir(scratchRoot).removeRecursively();
    // The goldens are Qt's behaviour at this version; a Qt upgrade shows up here first.
    writeFile(QDir(context.output).filePath(QStringLiteral("qt-version.txt")), QByteArray(qVersion()) + '\n');
    return 0;
}
