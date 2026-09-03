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
static volatile sig_atomic_t discardRequested = 0;
static void requestStop(int) { stopRequested = 1; }
static void requestDiscard(int) { discardRequested = 1; stopRequested = 1; }

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

qint64 Recorder::recordingStartedUs()
{
    return readState().value(QStringLiteral("started_us")).toVariant().toLongLong();
}

QString Recorder::recordedMonitor()
{
    return readState().value(QStringLiteral("monitor")).toString();
}

bool Recorder::recordingHasWebcam()
{
    return readState().value(QStringLiteral("webcam")).toBool(false);
}

bool Recorder::signalExisting(bool cancel, QString *error)
{
    const auto state = readState();
    const qint64 pid = state.value(QStringLiteral("pid")).toVariant().toLongLong();
    if (!recorderDaemonAlive(pid)) {
        QFile::remove(stateFilePath());
        if (error) *error = QStringLiteral("No recording is active");
        return false;
    }
    if (::kill(pid_t(pid), cancel ? SIGUSR2 : SIGUSR1) != 0) {
        if (error) *error = QString::fromLocal8Bit(strerror(errno));
        notifyStopFailure(QStringLiteral("Could not ask the recording process to stop. The video may be incomplete."));
        return false;
    }
    return true;
}

Recorder::GsrExitClassification Recorder::classifyGsrExit(
    int exitCode, bool requested, qint64 fileSize, double probedDuration)
{
    if (exitCode != 0 || fileSize <= 0 || !std::isfinite(probedDuration) || probedDuration <= 0.0)
        return GsrExitClassification::Failure;
    return requested ? GsrExitClassification::UserStop : GsrExitClassification::ExternalStop;
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
    if (options.microphoneAudio) arguments << QStringLiteral("--microphone-audio")
                                           << QStringLiteral("--microphone-device")
                                           << options.microphoneDevice;
    if (options.webcam) arguments << QStringLiteral("--webcam")
                                 << QStringLiteral("--webcam-device") << options.webcamDevice
                                 << QStringLiteral("--webcam-height") << QString::number(options.webcamHeight);
    if (options.noOpen) arguments << QStringLiteral("--no-open");
    if (options.noBar) arguments << QStringLiteral("--no-bar");
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
            if (reason.isEmpty()) {
                if (message) {
                    *message = selectionNote.isEmpty()
                        ? QStringLiteral("Recording finished")
                        : selectionNote + QLatin1Char('\n') + QStringLiteral("Recording finished");
                }
                return 0;
            }
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

int Recorder::stopExisting(bool cancel, QString *bundlePath, QString *error)
{
    const auto state = readState();
    const qint64 pid = state.value("pid").toVariant().toLongLong();
    if (!recorderDaemonAlive(pid)) {
        QFile::remove(stateFilePath());
        if (error) *error = QStringLiteral("No recording is active");
        return 1;
    }
    const QString bundle = state.value("bundle").toString();
    if (!signalExisting(cancel, error)) return 2;
    for (int i = 0; i < 800; ++i) {
        if (!recorderDaemonAlive(pid) || !QFileInfo::exists(stateFilePath())) {
            if (bundlePath) *bundlePath = cancel ? QString() : bundle;
            if (cancel) {
                const bool discarded = !QFileInfo::exists(bundle);
                if (error) {
                    if (discarded) error->clear();
                    else *error = QStringLiteral("Recording bundle could not be discarded");
                }
                return discarded ? 0 : 2;
            }
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

static bool writeFirstFrameTimestamp(const QString &path, qint64 monotonicUs)
{
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) return false;
    const qint64 realtimeUs = QDateTime::currentMSecsSinceEpoch() * 1000;
    file.write("monotonic_microsec\trealtime_microsec\n");
    file.write(QByteArray::number(monotonicUs) + '\t' + QByteArray::number(realtimeUs) + '\n');
    return file.commit();
}

static void drainCameraOutput(QProcess *process, QByteArray *tail, QByteArray *lines,
                              qint64 *fallbackFirstFrameUs, const QString &timestampPath,
                              QFile *debugLog)
{
    const QByteArray chunk = process->readAll();
    if (chunk.isEmpty()) return;
    if (debugLog && debugLog->isOpen()) {
        debugLog->write(chunk);
        debugLog->flush();
    }
    tail->append(chunk);
    if (tail->size() > 4096) tail->remove(0, tail->size() - 4096);
    if (!fallbackFirstFrameUs || *fallbackFirstFrameUs > 0) return;
    lines->append(chunk);
    qsizetype newline = -1;
    while ((newline = lines->indexOf('\n')) >= 0) {
        const QByteArray line = lines->left(newline).trimmed();
        lines->remove(0, newline + 1);
        if (!line.startsWith("frame=")) continue;
        bool ok = false;
        const int frame = line.mid(6).trimmed().toInt(&ok);
        if (ok && frame > 0) {
            *fallbackFirstFrameUs = CursorSampler::monotonicUs();
            writeFirstFrameTimestamp(timestampPath, *fallbackFirstFrameUs);
            break;
        }
    }
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
    discardRequested = 0;
    std::signal(SIGUSR1, requestStop);
    std::signal(SIGUSR2, requestDiscard);
    RecordOptions options;
    options.fps = valueAfter(arguments, QStringLiteral("--fps")).toInt();
    options.outputDirectory = valueAfter(arguments, QStringLiteral("--dir"));
    options.desktopAudio = arguments.contains(QStringLiteral("--desktop-audio"));
    options.microphoneAudio = arguments.contains(QStringLiteral("--microphone-audio"));
    options.microphoneDevice = valueAfter(arguments, QStringLiteral("--microphone-device"));
    if (options.microphoneDevice.isEmpty()) options.microphoneDevice = QStringLiteral("default_input");
    options.noOpen = arguments.contains(QStringLiteral("--no-open"));
    options.noBar = arguments.contains(QStringLiteral("--no-bar"));
    options.webcam = arguments.contains(QStringLiteral("--webcam"));
    options.webcamDevice = valueAfter(arguments, QStringLiteral("--webcam-device"));
    if (options.webcamDevice.isEmpty()) options.webcamDevice = QStringLiteral("/dev/video2");
    options.webcamHeight = valueAfter(arguments, QStringLiteral("--webcam-height")).toInt();
    if (options.webcamHeight != 720) options.webcamHeight = 1080;
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
    const QString cameraVideo = bundle + QStringLiteral("/camera.mp4");
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
    QStringList audioSources;
    if (options.desktopAudio) audioSources << QStringLiteral("default_output");
    if (options.microphoneAudio) audioSources << options.microphoneDevice;
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
    QStringList recorderArguments{QStringLiteral("__record-gsr")};
    recorderArguments << gsr;
    recorder.start(QCoreApplication::applicationFilePath(), recorderArguments);

    QProcess cameraRecorder;
    cameraRecorder.setProcessChannelMode(QProcess::MergedChannels);
    QByteArray cameraOutput;
    QByteArray cameraLines;
    qint64 fallbackFirstFrameUs = 0;
    bool cameraFallback = false;
    bool cameraAvailable = options.webcam;
    const QString cameraSize = options.webcamHeight == 720
        ? QStringLiteral("1280x720") : QStringLiteral("1920x1080");
    const auto startCameraFallback = [&] {
        QFile::remove(cameraVideo);
        QFile::remove(cameraVideo + QStringLiteral(".ts"));
        cameraFallback = true;
        const QStringList ffmpegArgs{QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("v4l2"), QStringLiteral("-framerate"), QStringLiteral("30"),
            QStringLiteral("-video_size"), cameraSize, QStringLiteral("-i"), options.webcamDevice,
            QStringLiteral("-an"), QStringLiteral("-c:v"), QStringLiteral("libx264"),
            QStringLiteral("-preset"), QStringLiteral("veryfast"), QStringLiteral("-progress"),
            QStringLiteral("pipe:1"), QStringLiteral("-nostats"), cameraVideo};
        cameraRecorder.start(QStringLiteral("ffmpeg"), ffmpegArgs);
        cameraAvailable = cameraRecorder.waitForStarted(1000);
    };
    if (cameraAvailable) {
        const QStringList cameraGsr{QStringLiteral("__record-gsr"), QStringLiteral("-w"),
            options.webcamDevice, QStringLiteral("-s"), cameraSize,
            QStringLiteral("-f"), QStringLiteral("30"), QStringLiteral("-fm"),
            QStringLiteral("cfr"), QStringLiteral("-k"), QStringLiteral("auto"),
            QStringLiteral("-cursor"), QStringLiteral("no"),
            QStringLiteral("-write-first-frame-ts"), QStringLiteral("yes"),
            QStringLiteral("-o"), cameraVideo};
        cameraRecorder.start(QCoreApplication::applicationFilePath(), cameraGsr);
    }
    if (!recorder.waitForStarted(5000)) {
        if (cameraRecorder.state() != QProcess::NotRunning) cameraRecorder.kill();
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        QString reason = lastNonEmptyLine(recorderOutput);
        if (reason.isEmpty()) reason = recorder.errorString();
        return failRecorderStartup(reason, bundle, video, &cursorSampler, &evdevListener);
    }

    if (cameraAvailable && !cameraRecorder.waitForStarted(5000)) startCameraFallback();
    const qint64 recorderStartedUs = CursorSampler::monotonicUs();
    QJsonObject state{{"pid", daemonPid}, {"bundle", bundle}, {"started_us", startedUs},
                      {"monitor", region.monitorName}, {"no_open", options.noOpen},
                      {"webcam", cameraAvailable}};
    if (!writeJson(stateFilePath(), state, &error)) {
        ::kill(pid_t(recorder.processId()), SIGINT);
        if (cameraRecorder.state() != QProcess::NotRunning)
            ::kill(pid_t(cameraRecorder.processId()), SIGINT);
        recorder.waitForFinished(5000);
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        const QString reason = error.isEmpty()
            ? QStringLiteral("Could not write recorder state")
            : QStringLiteral("Could not write recorder state: %1").arg(error);
        return failRecorderStartup(reason, bundle, video, &cursorSampler, &evdevListener);
    }
    if (!options.noBar)
        QProcess::startDetached(QCoreApplication::applicationFilePath(), {QStringLiteral("__record-bar")});
    runOptionalDetached(QStringLiteral("omarchy-notification-send"),
                        {QStringLiteral("-t"), QStringLiteral("3000"),
                         QStringLiteral("Recording started"),
                         QStringLiteral("Stop with the bar, the REC indicator, or your keybind")});
    runOptionalDetached(QStringLiteral("omarchy-shell"),
                        {QStringLiteral("-q"), QStringLiteral("omarchy.indicators"), QStringLiteral("refresh")});
    while (!stopRequested && recorder.state() != QProcess::NotRunning
           && CursorSampler::monotonicUs() - recorderStartedUs < 2000000) {
        recorder.waitForFinished(0);
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        if (cameraAvailable) {
            cameraRecorder.waitForFinished(0);
            if (cameraFallback)
                drainCameraOutput(&cameraRecorder, &cameraOutput, &cameraLines,
                                  &fallbackFirstFrameUs, cameraVideo + QStringLiteral(".ts"), &debugLog);
            else drainRecorderOutput(&cameraRecorder, &cameraOutput, &debugLog);
            if (!cameraFallback && cameraRecorder.state() == QProcess::NotRunning) {
                startCameraFallback();
            } else if (cameraFallback && cameraRecorder.state() == QProcess::NotRunning) {
                cameraAvailable = false;
            }
        }
        QThread::msleep(40);
    }
    drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
    while (!stopRequested && recorder.state() != QProcess::NotRunning) {
        recorder.waitForFinished(0);
        drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
        if (cameraAvailable) {
            cameraRecorder.waitForFinished(0);
            if (cameraFallback)
                drainCameraOutput(&cameraRecorder, &cameraOutput, &cameraLines,
                                  &fallbackFirstFrameUs, cameraVideo + QStringLiteral(".ts"), &debugLog);
            else drainRecorderOutput(&cameraRecorder, &cameraOutput, &debugLog);
        }
        QThread::msleep(40);
    }
    const bool unexpectedExit = !stopRequested && recorder.state() == QProcess::NotRunning;
    bool forcedStop = false;
    if (recorder.state() != QProcess::NotRunning) {
        ::kill(pid_t(recorder.processId()), SIGINT);
        if (cameraRecorder.state() != QProcess::NotRunning)
            ::kill(pid_t(cameraRecorder.processId()), SIGINT);
        if (!recorder.waitForFinished(5000)) {
            forcedStop = true;
            recorder.kill();
            recorder.waitForFinished(1000);
        }
    }
    if (cameraRecorder.state() != QProcess::NotRunning) {
        ::kill(pid_t(cameraRecorder.processId()), SIGINT);
        if (!cameraRecorder.waitForFinished(5000)) {
            cameraRecorder.kill();
            cameraRecorder.waitForFinished(1000);
        }
    }
    if (cameraFallback)
        drainCameraOutput(&cameraRecorder, &cameraOutput, &cameraLines,
                          &fallbackFirstFrameUs, cameraVideo + QStringLiteral(".ts"), &debugLog);
    else drainRecorderOutput(&cameraRecorder, &cameraOutput, &debugLog);
    drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
    const QString recorderErrorLine = lastNonEmptyLine(recorderOutput);
    const bool recorderFailed = recorder.exitStatus() != QProcess::NormalExit || recorder.exitCode() != 0;
    const qint64 stoppedUs = CursorSampler::monotonicUs();
    cursorSampler.stop();
    evdevListener.stop();

    if (discardRequested) {
        const bool removed = QDir(bundle).removeRecursively();
        QFile::remove(stateFilePath());
        runOptionalDetached(QStringLiteral("omarchy-shell"),
                            {QStringLiteral("-q"), QStringLiteral("omarchy.indicators"), QStringLiteral("refresh")});
        runOptionalDetached(QStringLiteral("omarchy-notification-send"),
                            {QStringLiteral("Recording discarded"), QFileInfo(bundle).completeBaseName()});
        QTextStream(stderr) << (removed ? QStringLiteral("Recording discarded: ")
                                          : QStringLiteral("Could not discard recording: "))
                            << bundle << '\n';
        return removed ? 0 : 2;
    }

    const auto cursor = cursorSampler.samples();
    const auto deviceEvents = evdevListener.events();
    const qint64 firstFrameUs = firstFrameTimestamp(video + QStringLiteral(".ts"));

    QProcess ffprobe;
    ffprobe.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-show_entries"), QStringLiteral("format=duration"),
        QStringLiteral("-of"), QStringLiteral("default=noprint_wrappers=1:nokey=1"), video});
    const bool probeFinished = ffprobe.waitForFinished(10000);
    bool durationOk = false;
    const double probedDuration = probeFinished && ffprobe.exitCode() == 0
        ? QString::fromUtf8(ffprobe.readAllStandardOutput()).trimmed().toDouble(&durationOk) : 0.0;
    const int effectiveExitCode = recorder.exitStatus() == QProcess::NormalExit ? recorder.exitCode() : -1;
    const auto exitClassification = classifyGsrExit(effectiveExitCode, stopRequested,
                                                     QFileInfo(video).size(),
                                                     durationOk ? probedDuration : 0.0);
    if (debugLog.isOpen()) {
        const char *classification = exitClassification == GsrExitClassification::UserStop ? "user-stop"
            : exitClassification == GsrExitClassification::ExternalStop ? "external-stop" : "failure";
        debugLog.write(QByteArray("omarecord: gsr exit classified as ") + classification
                       + ", exit=" + QByteArray::number(effectiveExitCode)
                       + ", bytes=" + QByteArray::number(QFileInfo(video).size())
                       + ", duration=" + QByteArray::number(probedDuration, 'f', 3) + "\n");
        debugLog.flush();
    }
    bool ok = !forcedStop && exitClassification != GsrExitClassification::Failure
        && firstFrameUs > 0;
    ok = writeInputLog(bundle + QStringLiteral("/input.jsonl"), cursor, deviceEvents, region, &error) && ok;
    QJsonObject capture{
        {"version", 1}, {"fps", options.fps}, {"width", region.physicalWidth},
        {"height", region.physicalHeight},
        {"region", QJsonObject{{"x", region.x}, {"y", region.y},
                                {"w", region.width}, {"h", region.height}}},
        {"scale", region.scale}, {"monitor", region.monitorName},
        {"first_frame_us", firstFrameUs}, {"started_us", startedUs}, {"stopped_us", stoppedUs},
        {"audio", QJsonObject{{"desktop", options.desktopAudio}, {"mic", options.microphoneAudio},
                                {"microphoneDevice", options.microphoneDevice}}}
    };
    if (cameraAvailable && QFileInfo(cameraVideo).size() > 0) {
        const qint64 cameraFirstFrameUs = firstFrameTimestamp(cameraVideo + QStringLiteral(".ts"));
        QProcess cameraProbe;
        cameraProbe.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-select_streams"), QStringLiteral("v:0"), QStringLiteral("-show_entries"),
            QStringLiteral("stream=width,height,r_frame_rate"), QStringLiteral("-of"), QStringLiteral("json"),
            cameraVideo});
        if (cameraProbe.waitForFinished(10000) && cameraProbe.exitCode() == 0 && cameraFirstFrameUs > 0) {
            const QJsonArray streams = QJsonDocument::fromJson(cameraProbe.readAllStandardOutput())
                                           .object().value(QStringLiteral("streams")).toArray();
            const QJsonObject stream = streams.isEmpty() ? QJsonObject{} : streams.first().toObject();
            const QStringList rate = stream.value(QStringLiteral("r_frame_rate")).toString().split('/');
            const double fps = rate.size() == 2 && rate[1].toDouble() != 0.0
                ? rate[0].toDouble() / rate[1].toDouble() : 30.0;
            capture.insert(QStringLiteral("camera"), QJsonObject{
                {QStringLiteral("device"), options.webcamDevice},
                {QStringLiteral("requestedHeight"), options.webcamHeight},
                {QStringLiteral("width"), stream.value(QStringLiteral("width")).toInt()},
                {QStringLiteral("height"), stream.value(QStringLiteral("height")).toInt()},
                {QStringLiteral("fps"), fps},
                {QStringLiteral("first_frame_us"), cameraFirstFrameUs},
                {QStringLiteral("backend"), cameraFallback ? QStringLiteral("ffmpeg-v4l2")
                                                            : QStringLiteral("gpu-screen-recorder")}});
        } else {
            QFile::remove(cameraVideo);
            QFile::remove(cameraVideo + QStringLiteral(".ts"));
        }
    }
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
        if (debugLog.isOpen()) {
            debugLog.write(QStringLiteral("omarecord: Recording saved: %1\n").arg(bundle).toUtf8());
            debugLog.flush();
        }
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
        writeLastError(detail);
    }
    if (debugLog.isOpen()) {
        debugLog.write(QByteArray("omarecord: daemon exit=") + (ok ? "0\n" : "2\n"));
        debugLog.flush();
    }
    return ok ? 0 : 2;
}
