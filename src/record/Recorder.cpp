#include "Recorder.h"
#include "CursorSampler.h"
#include "EvdevListener.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QSaveFile>
#include <QStandardPaths>
#include <QTextStream>
#include <QThread>
#include <algorithm>
#include <atomic>
#include <csignal>
#include <cmath>
#include <cerrno>
#include <cstring>
#include <unistd.h>

using namespace OmaRecord;

static volatile sig_atomic_t stopRequested = 0;
static void requestStop(int) { stopRequested = 1; }

QString Recorder::stateFilePath()
{
    QString runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    if (runtime.isEmpty()) runtime = QStringLiteral("/tmp");
    return runtime + QStringLiteral("/omarecord/recording.json");
}

static QString lastErrorFilePath()
{
    return QFileInfo(Recorder::stateFilePath()).absolutePath()
        + QStringLiteral("/last-error.txt");
}

static QJsonObject readState()
{
    QFile file(Recorder::stateFilePath());
    if (!file.open(QIODevice::ReadOnly)) return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}

static void runOptionalDetached(const QString &program, const QStringList &arguments)
{
    if (!QStandardPaths::findExecutable(program).isEmpty()) QProcess::startDetached(program, arguments);
}

static void notifyStopFailure(const QString &detail)
{
    runOptionalDetached(QStringLiteral("omarchy-notification-send"),
                        {QStringLiteral("-u"), QStringLiteral("critical"),
                         QStringLiteral("-t"), QStringLiteral("5000"),
                         QStringLiteral("Screen recording error"), detail});
}

static void notifyStartFailure(const QString &detail)
{
    runOptionalDetached(QStringLiteral("omarchy-notification-send"),
                        {QStringLiteral("-u"), QStringLiteral("critical"),
                         QStringLiteral("Recording failed"), detail});
}

static void writeLastError(const QString &detail)
{
    QDir().mkpath(QFileInfo(lastErrorFilePath()).absolutePath());
    QSaveFile file(lastErrorFilePath());
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) return;
    file.write(detail.toUtf8());
    file.write("\n");
    file.commit();
}

static QString readLastError()
{
    QFile file(lastErrorFilePath());
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) return {};
    return QString::fromUtf8(file.readAll()).trimmed();
}

static bool processAlive(qint64 pid)
{
    return pid > 1 && (::kill(pid_t(pid), 0) == 0 || errno == EPERM);
}

static bool recorderDaemonAlive(qint64 pid)
{
    if (!processAlive(pid)) return false;
    QFile commandLine(QStringLiteral("/proc/%1/cmdline").arg(pid));
    if (!commandLine.open(QIODevice::ReadOnly)) return false;
    return commandLine.readAll().contains("__record-daemon");
}

bool Recorder::isRecording()
{
    const auto state = readState();
    const qint64 pid = state.value("pid").toVariant().toLongLong();
    if (recorderDaemonAlive(pid)) return true;
    if (!state.isEmpty()) QFile::remove(stateFilePath());
    return false;
}

static QString modeName(CaptureMode mode)
{
    if (mode == CaptureMode::Fullscreen) return QStringLiteral("fullscreen");
    if (mode == CaptureMode::Window) return QStringLiteral("window");
    return QStringLiteral("region");
}

int Recorder::startDetached(const RecordOptions &options, QString *message)
{
    CaptureRegion region;
    QString error;
    if (!RegionPicker::pick(options.mode, &region, &error)) {
        if (message) *message = error;
        return error == QLatin1String("Selection cancelled") ? 1 : 2;
    }
    const QString selectionNote = error;
    QDir().mkpath(QFileInfo(stateFilePath()).absolutePath());
    QFile::remove(lastErrorFilePath());
    QStringList arguments{
        QStringLiteral("__record-daemon"),
        QStringLiteral("--mode"), modeName(region.mode),
        QStringLiteral("--monitor"), region.monitorName,
        QStringLiteral("--x"), QString::number(region.x, 'g', 16),
        QStringLiteral("--y"), QString::number(region.y, 'g', 16),
        QStringLiteral("--w"), QString::number(region.width, 'g', 16),
        QStringLiteral("--h"), QString::number(region.height, 'g', 16),
        QStringLiteral("--scale"), QString::number(region.scale, 'g', 16),
        QStringLiteral("--physical-w"), QString::number(region.physicalWidth),
        QStringLiteral("--physical-h"), QString::number(region.physicalHeight),
        QStringLiteral("--fps"), QString::number(options.fps),
        QStringLiteral("--dir"), options.outputDirectory
    };
    if (options.desktopAudio) arguments << QStringLiteral("--desktop-audio");
    if (options.microphoneAudio) arguments << QStringLiteral("--microphone-audio");
    if (options.noOpen) arguments << QStringLiteral("--no-open");
    QProcess daemon;
    daemon.setProgram(QCoreApplication::applicationFilePath());
    daemon.setArguments(arguments);
    daemon.setStandardOutputFile(QProcess::nullDevice());
    daemon.setStandardErrorFile(QProcess::nullDevice());
    qint64 pid = 0;
    if (!daemon.startDetached(&pid)) {
        if (message) *message = QStringLiteral("Could not start recorder daemon");
        return 2;
    }
    QElapsedTimer readyTimer;
    readyTimer.start();
    bool stateAppeared = false;
    while (readyTimer.elapsed() < 2000) {
        const auto state = readState();
        if (state.value("pid").toVariant().toLongLong() == pid) {
            stateAppeared = true;
            break;
        }
        if (!recorderDaemonAlive(pid)) {
            QString reason = readLastError();
            if (reason.isEmpty()) reason = QStringLiteral("Recorder daemon exited during startup");
            QFile::remove(lastErrorFilePath());
            if (message) *message = reason;
            return 2;
        }
        QThread::msleep(20);
    }
    if (!stateAppeared) {
        if (message) *message = QStringLiteral("Recorder daemon did not become ready");
        return 2;
    }

    QElapsedTimer stableTimer;
    stableTimer.start();
    while (stableTimer.elapsed() < 2500) {
        const auto state = readState();
        if (state.value("pid").toVariant().toLongLong() != pid || !recorderDaemonAlive(pid)) {
            QString reason = readLastError();
            if (reason.isEmpty()) reason = QStringLiteral("Recorder daemon exited during startup");
            QFile::remove(lastErrorFilePath());
            if (message) *message = reason;
            return 2;
        }
        QThread::msleep(20);
    }
    if (message) {
        *message = selectionNote.isEmpty()
            ? QStringLiteral("Recording started")
            : selectionNote + QLatin1Char('\n') + QStringLiteral("Recording started");
    }
    return 0;
}

int Recorder::stopExisting(bool onlyStop, QString *bundlePath, QString *error)
{
    Q_UNUSED(onlyStop)
    const auto state = readState();
    const qint64 pid = state.value("pid").toVariant().toLongLong();
    if (!recorderDaemonAlive(pid)) {
        QFile::remove(stateFilePath());
        if (error) *error = QStringLiteral("No recording is active");
        return 1;
    }
    const QString bundle = state.value("bundle").toString();
    if (::kill(pid_t(pid), SIGUSR1) != 0) {
        if (error) *error = QString::fromLocal8Bit(strerror(errno));
        notifyStopFailure(QStringLiteral("Could not ask the recording process to stop. The video may be incomplete."));
        return 2;
    }
    for (int i = 0; i < 800; ++i) {
        if (!recorderDaemonAlive(pid) || !QFileInfo::exists(stateFilePath())) {
            if (bundlePath) *bundlePath = bundle;
            const bool complete = QFileInfo(bundle + QStringLiteral("/screen.mp4")).size() > 0
                && QFileInfo(bundle + QStringLiteral("/capture.json")).size() > 0
                && QFileInfo(bundle + QStringLiteral("/input.jsonl")).isFile();
            if (error) {
                if (complete) error->clear();
                else *error = QStringLiteral("Recording could not be finalized");
            }
            return complete ? 0 : 2;
        }
        QThread::msleep(25);
    }
    if (error) *error = QStringLiteral("Timed out waiting for recorder to finalize");
    notifyStopFailure(QStringLiteral("Timed out while stopping the recording. The video may be incomplete."));
    return 2;
}

static QString valueAfter(const QStringList &args, const QString &name)
{
    const int index = args.indexOf(name);
    return index >= 0 && index + 1 < args.size() ? args[index + 1] : QString();
}

static qint64 firstFrameTimestamp(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) return 0;
    while (!file.atEnd()) {
        const QList<QByteArray> parts = file.readLine().trimmed().split('\t');
        bool ok = false;
        const qint64 value = parts.value(0).toLongLong(&ok);
        if (ok) return value;
    }
    return 0;
}

static QPointF cursorAt(const QVector<RawCursorSample> &samples, qint64 time)
{
    if (samples.isEmpty()) return {};
    auto it = std::upper_bound(samples.begin(), samples.end(), time,
                               [](qint64 value, const RawCursorSample &sample) {
        return value < sample.monotonicUs;
    });
    if (it == samples.begin()) return it->logicalPosition;
    return (it - 1)->logicalPosition;
}

static QJsonObject positionObject(const QPointF &logical, const CaptureRegion &region)
{
    return QJsonObject{{"x", (logical.x() - region.x) * region.scale},
                       {"y", (logical.y() - region.y) * region.scale}};
}

static bool writeInputLog(const QString &path, const QVector<RawCursorSample> &cursor,
                          const QVector<DeviceEvent> &devices, const CaptureRegion &region,
                          QString *error)
{
    struct Serialized { qint64 time; QJsonObject object; };
    QVector<Serialized> all;
    all.reserve(cursor.size() + devices.size());
    for (const auto &sample : cursor) {
        QJsonObject object{{"t", sample.monotonicUs}, {"k", "m"}};
        const auto position = positionObject(sample.logicalPosition, region);
        object.insert("x", position.value("x")); object.insert("y", position.value("y"));
        all << Serialized{sample.monotonicUs, object};
    }
    for (const auto &event : devices) {
        QJsonObject object{{"t", event.monotonicUs}};
        if (event.kind == DeviceEventKind::ButtonDown || event.kind == DeviceEventKind::ButtonUp) {
            object["k"] = event.kind == DeviceEventKind::ButtonDown ? "d" : "u";
            object["b"] = event.button;
            const auto position = positionObject(cursorAt(cursor, event.monotonicUs), region);
            object.insert("x", position.value("x")); object.insert("y", position.value("y"));
        } else if (event.kind == DeviceEventKind::Scroll) {
            object["k"] = "s"; object["dx"] = event.dx; object["dy"] = event.dy;
            const auto position = positionObject(cursorAt(cursor, event.monotonicUs), region);
            object.insert("x", position.value("x")); object.insert("y", position.value("y"));
        } else {
            const bool down = event.kind == DeviceEventKind::KeyDown;
            object["k"] = down ? "kd" : "ku";
            object["code"] = event.keyCode;
            if (down) {
                object["name"] = event.keyName;
                object["mods"] = QJsonArray::fromStringList(event.modifiers);
            }
        }
        all << Serialized{event.monotonicUs, object};
    }
    std::stable_sort(all.begin(), all.end(), [](const auto &a, const auto &b) { return a.time < b.time; });
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) {
        if (error) *error = file.errorString(); return false;
    }
    for (const auto &entry : all)
        file.write(QJsonDocument(entry.object).toJson(QJsonDocument::Compact) + '\n');
    if (!file.commit()) { if (error) *error = file.errorString(); return false; }
    return true;
}

static bool writeJson(const QString &path, const QJsonObject &object, QString *error)
{
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) { if (error) *error = file.errorString(); return false; }
    file.write(QJsonDocument(object).toJson(QJsonDocument::Indented));
    if (!file.commit()) { if (error) *error = file.errorString(); return false; }
    return true;
}

static void drainRecorderOutput(QProcess *process, QByteArray *tail, QFile *debugLog)
{
    const QByteArray chunk = process->readAll();
    if (chunk.isEmpty()) return;
    if (debugLog && debugLog->isOpen()) {
        debugLog->write(chunk);
        debugLog->flush();
    }
    tail->append(chunk);
    constexpr qsizetype maximumTail = 4096;
    if (tail->size() > maximumTail) tail->remove(0, tail->size() - maximumTail);
}

static QString lastNonEmptyLine(const QByteArray &output)
{
    const QList<QByteArray> lines = output.split('\n');
    for (auto it = lines.crbegin(); it != lines.crend(); ++it) {
        const QString line = QString::fromUtf8(*it).trimmed();
        if (!line.isEmpty()) return line;
    }
    return {};
}

static int failRecorderStartup(const QString &reason, const QString &bundle, const QString &video,
                               CursorSampler *cursorSampler, EvdevListener *evdevListener)
{
    cursorSampler->stop();
    evdevListener->stop();
    if (!QFileInfo(video).isFile() || QFileInfo(video).size() == 0)
        QDir(bundle).removeRecursively();
    writeLastError(reason);
    QFile::remove(Recorder::stateFilePath());
    runOptionalDetached(QStringLiteral("omarchy-shell"),
                        {QStringLiteral("-q"), QStringLiteral("omarchy.indicators"),
                         QStringLiteral("refresh")});
    notifyStartFailure(reason);
    QTextStream(stderr) << reason << '\n';
    return 2;
}

int Recorder::daemonMain(const QStringList &arguments)
{
    stopRequested = 0;
    std::signal(SIGUSR1, requestStop);
    RecordOptions options;
    options.fps = valueAfter(arguments, QStringLiteral("--fps")).toInt();
    options.outputDirectory = valueAfter(arguments, QStringLiteral("--dir"));
    options.desktopAudio = arguments.contains(QStringLiteral("--desktop-audio"));
    options.microphoneAudio = arguments.contains(QStringLiteral("--microphone-audio"));
    options.noOpen = arguments.contains(QStringLiteral("--no-open"));
    CaptureRegion region;
    const QString mode = valueAfter(arguments, QStringLiteral("--mode"));
    region.mode = mode == QLatin1String("fullscreen") ? CaptureMode::Fullscreen
                : mode == QLatin1String("window") ? CaptureMode::Window : CaptureMode::Region;
    region.monitorName = valueAfter(arguments, QStringLiteral("--monitor"));
    region.x = valueAfter(arguments, QStringLiteral("--x")).toDouble();
    region.y = valueAfter(arguments, QStringLiteral("--y")).toDouble();
    region.width = valueAfter(arguments, QStringLiteral("--w")).toDouble();
    region.height = valueAfter(arguments, QStringLiteral("--h")).toDouble();
    region.scale = valueAfter(arguments, QStringLiteral("--scale")).toDouble();
    region.physicalWidth = valueAfter(arguments, QStringLiteral("--physical-w")).toInt();
    region.physicalHeight = valueAfter(arguments, QStringLiteral("--physical-h")).toInt();

    const QDateTime now = QDateTime::currentDateTime();
    const QString displayName = QStringLiteral("Recording %1").arg(now.toString(QStringLiteral("yyyy-MM-dd HH-mm-ss")));
    QDir().mkpath(options.outputDirectory);
    QString bundle = QDir(options.outputDirectory).filePath(displayName + QStringLiteral(".omarecord"));
    int suffix = 2;
    while (QFileInfo::exists(bundle))
        bundle = QDir(options.outputDirectory).filePath(displayName + QStringLiteral(" %1.omarecord").arg(suffix++));
    if (!QDir().mkpath(bundle)) return 2;

    const qint64 daemonPid = QCoreApplication::applicationPid();
    const qint64 startedUs = CursorSampler::monotonicUs();
    QString error;

    CursorSampler cursorSampler;
    EvdevListener evdevListener;
    QString warning;
    if (!cursorSampler.start(&warning)) qWarning().noquote() << warning;
    if (!evdevListener.start(&warning)) qWarning().noquote() << warning;

    const QString video = bundle + QStringLiteral("/screen.mp4");
    QStringList gsr;
    if (region.mode == CaptureMode::Fullscreen) {
        gsr << QStringLiteral("-w") << region.monitorName << QStringLiteral("-s") << QStringLiteral("0x0");
    } else {
        const int captureX = int(std::lround(region.x));
        const int captureY = int(std::lround(region.y));
        gsr << QStringLiteral("-w") << QStringLiteral("region") << QStringLiteral("-region")
            << QStringLiteral("%1x%2+%3+%4").arg(int(std::lround(region.width)))
                                                  .arg(int(std::lround(region.height)))
                                                  .arg(captureX).arg(captureY);
    }
    gsr << QStringLiteral("-cursor") << QStringLiteral("no")
        << QStringLiteral("-write-first-frame-ts") << QStringLiteral("yes")
        << QStringLiteral("-f") << QString::number(options.fps)
        << QStringLiteral("-fm") << QStringLiteral("cfr")
        << QStringLiteral("-k") << QStringLiteral("auto")
        << QStringLiteral("-fallback-cpu-encoding") << QStringLiteral("yes");
    // "a|b" merges sources into one track; separate -a flags would produce two
    // tracks that most players only play one of (same choice as omarchy-capture-screenrecording).
    QStringList audioSources;
    if (options.desktopAudio) audioSources << QStringLiteral("default_output");
    if (options.microphoneAudio) audioSources << QStringLiteral("default_input");
    if (!audioSources.isEmpty())
        gsr << QStringLiteral("-a") << audioSources.join(QLatin1Char('|'))
            << QStringLiteral("-ac") << QStringLiteral("aac");
    gsr << QStringLiteral("-o") << video;

    QProcess recorder;
    recorder.setProcessChannelMode(QProcess::MergedChannels);
    QByteArray recorderOutput;
    QFile debugLog(QStringLiteral("/tmp/omarecord.log"));
    if (qEnvironmentVariable("OMARECORD_DEBUG") == QLatin1String("1"))
        (void)debugLog.open(QIODevice::WriteOnly | QIODevice::Append);
    recorder.start(QStringLiteral("gpu-screen-recorder"), gsr);
    if (!recorder.waitForStarted(5000)) {
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        QString reason = lastNonEmptyLine(recorderOutput);
        if (reason.isEmpty()) reason = recorder.errorString();
        return failRecorderStartup(reason, bundle, video, &cursorSampler, &evdevListener);
    }
    const qint64 recorderStartedUs = CursorSampler::monotonicUs();
    QJsonObject state{{"pid", daemonPid}, {"bundle", bundle}, {"started_us", startedUs},
                      {"no_open", options.noOpen}};
    if (!writeJson(stateFilePath(), state, &error)) {
        ::kill(pid_t(recorder.processId()), SIGINT);
        recorder.waitForFinished(5000);
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        const QString reason = error.isEmpty()
            ? QStringLiteral("Could not write recorder state")
            : QStringLiteral("Could not write recorder state: %1").arg(error);
        return failRecorderStartup(reason, bundle, video, &cursorSampler, &evdevListener);
    }
    runOptionalDetached(QStringLiteral("omarchy-shell"),
                        {QStringLiteral("-q"), QStringLiteral("omarchy.indicators"), QStringLiteral("refresh")});
    while (!stopRequested && recorder.state() != QProcess::NotRunning
           && CursorSampler::monotonicUs() - recorderStartedUs < 2000000) {
        recorder.waitForFinished(0);
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        QThread::msleep(40);
    }
    drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
    if (!stopRequested && recorder.state() == QProcess::NotRunning) {
        QString reason = lastNonEmptyLine(recorderOutput);
        if (reason.isEmpty()) reason = QStringLiteral("gpu-screen-recorder exited before recording could start");
        return failRecorderStartup(reason, bundle, video, &cursorSampler, &evdevListener);
    }

    while (!stopRequested && recorder.state() != QProcess::NotRunning) {
        recorder.waitForFinished(0);
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        QThread::msleep(40);
    }
    const bool unexpectedExit = !stopRequested && recorder.state() == QProcess::NotRunning;
    bool forcedStop = false;
    if (recorder.state() != QProcess::NotRunning) {
        ::kill(pid_t(recorder.processId()), SIGINT);
        if (!recorder.waitForFinished(5000)) {
            forcedStop = true;
            recorder.kill();
            recorder.waitForFinished(1000);
        }
    }
    drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
    const QString recorderErrorLine = lastNonEmptyLine(recorderOutput);
    const bool recorderFailed = recorder.exitStatus() != QProcess::NormalExit || recorder.exitCode() != 0;
    const qint64 stoppedUs = CursorSampler::monotonicUs();
    cursorSampler.stop();
    evdevListener.stop();
    const auto cursor = cursorSampler.samples();
    const auto deviceEvents = evdevListener.events();
    const qint64 firstFrameUs = firstFrameTimestamp(video + QStringLiteral(".ts"));

    bool ok = !forcedStop && !recorderFailed && !unexpectedExit
        && firstFrameUs > 0 && QFileInfo(video).size() > 0;
    ok = writeInputLog(bundle + QStringLiteral("/input.jsonl"), cursor, deviceEvents, region, &error) && ok;
    QJsonObject capture{
        {"version", 1}, {"fps", options.fps}, {"width", region.physicalWidth},
        {"height", region.physicalHeight},
        {"region", QJsonObject{{"x", region.x}, {"y", region.y},
                                {"w", region.width}, {"h", region.height}}},
        {"scale", region.scale}, {"monitor", region.monitorName},
        {"first_frame_us", firstFrameUs}, {"started_us", startedUs}, {"stopped_us", stoppedUs},
        {"audio", QJsonObject{{"desktop", options.desktopAudio}, {"mic", options.microphoneAudio}}}
    };
    ok = writeJson(bundle + QStringLiteral("/capture.json"), capture, &error) && ok;

    QProcess ffmpeg;
    ffmpeg.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-loglevel"),
        QStringLiteral("error"), QStringLiteral("-i"), video, QStringLiteral("-frames:v"),
        QStringLiteral("1"), QStringLiteral("-q:v"), QStringLiteral("3"),
        bundle + QStringLiteral("/thumb.jpg")});
    ffmpeg.waitForFinished(10000);
    if (!ok && (!QFileInfo(video).isFile() || QFileInfo(video).size() == 0))
        QDir(bundle).removeRecursively();
    QFile::remove(stateFilePath());
    runOptionalDetached(QStringLiteral("omarchy-shell"),
                        {QStringLiteral("-q"), QStringLiteral("omarchy.indicators"), QStringLiteral("refresh")});
    if (ok) {
        runOptionalDetached(QStringLiteral("omarchy-notification-send"),
                            {QStringLiteral("Recording saved"), QFileInfo(bundle).completeBaseName(),
                             QStringLiteral("--exec"), QCoreApplication::applicationFilePath(),
                             QStringLiteral("edit"), bundle});
        if (!options.noOpen)
            QProcess::startDetached(QCoreApplication::applicationFilePath(), {QStringLiteral("edit"), bundle});
    } else {
        QString detail = forcedStop
            ? QStringLiteral("Recording process had to be force-killed. Video may be corrupted.")
            : QStringLiteral("The recording could not be finalized. Set OMARECORD_DEBUG=1 and check /tmp/omarecord.log.");
        if ((unexpectedExit || recorderFailed) && !recorderErrorLine.isEmpty())
            detail += QStringLiteral("\ngpu-screen-recorder: %1").arg(recorderErrorLine);
        notifyStopFailure(detail);
    }
    return ok ? 0 : 2;
}
