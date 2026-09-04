#include "Recorder.h"
#include "CursorSampler.h"
#include "EvdevListener.h"
#include "ScreenCapture.h"
#include "core/RecordingPreferences.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLockFile>
#include <QProcess>
#include <QRectF>
#include <QSaveFile>
#include <QStandardPaths>
#include <QTextStream>
#include <QThread>
#include <QVector>
#include <algorithm>
#include <atomic>
#include <csignal>
#include <cmath>
#include <cerrno>
#include <cstring>
#include <future>
#include <unistd.h>

using namespace Omareel;

static volatile sig_atomic_t stopRequested = 0;
static volatile sig_atomic_t discardRequested = 0;
static void requestStop(int) { stopRequested = 1; }
static void requestDiscard(int) { discardRequested = 1; stopRequested = 1; }

static void debugStartStage(const QString &stage, qint64 startUs,
                            const QString &detail = {})
{
    if (qEnvironmentVariable("OMAREEL_DEBUG") != QLatin1String("1")) return;
    QFile file(QStringLiteral("/tmp/omareel.log"));
    if (!file.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text)) return;
    const qint64 now = CursorSampler::monotonicUs();
    QString line = QStringLiteral("OMAREEL_START stage=%1 monotonic_us=%2 elapsed_ms=%3")
        .arg(stage).arg(now).arg((now - startUs) / 1000.0, 0, 'f', 3);
    if (!detail.isEmpty()) line += QLatin1Char(' ') + detail;
    file.write((line + QLatin1Char('\n')).toUtf8());
}

QString Recorder::stateFilePath()
{
    QString runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    if (runtime.isEmpty()) runtime = QStringLiteral("/tmp");
    return runtime + QStringLiteral("/omareel/recording.json");
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

QJsonObject Recorder::recordingState()
{
    return readState();
}

bool Recorder::updateRecordingState(const QJsonObject &values, QString *error)
{
    QLockFile lock(stateFilePath() + QStringLiteral(".lock"));
    lock.setStaleLockTime(10000);
    if (!lock.tryLock(1000)) {
        if (error) *error = lock.error() == QLockFile::LockFailedError
            ? QStringLiteral("Recording state is busy") : QStringLiteral("Could not lock recording state");
        return false;
    }
    QJsonObject state = readState();
    for (auto it = values.begin(); it != values.end(); ++it) state.insert(it.key(), it.value());
    QSaveFile file(stateFilePath());
    if (!file.open(QIODevice::WriteOnly)) {
        if (error) *error = file.errorString();
        return false;
    }
    file.write(QJsonDocument(state).toJson(QJsonDocument::Indented));
    const bool committed = file.commit();
    if (!committed && error) *error = file.errorString();
    if (committed && error) error->clear();
    return committed;
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

static QVector<qint64> processDescendants(qint64 parentPid)
{
    QVector<qint64> result;
    QVector<qint64> pending{parentPid};
    while (!pending.isEmpty()) {
        const qint64 parent = pending.takeLast();
        QFile children(QStringLiteral("/proc/%1/task/%1/children").arg(parent));
        if (!children.open(QIODevice::ReadOnly | QIODevice::Text)) continue;
        const QList<QByteArray> ids = children.readAll().simplified().split(' ');
        for (const QByteArray &id : ids) {
            bool ok = false;
            const qint64 child = id.toLongLong(&ok);
            if (!ok || child <= 1 || result.contains(child)) continue;
            result << child;
            pending << child;
        }
    }
    return result;
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

QJsonObject Recorder::cameraCaptureBlock(const QString &device, int requestedHeight,
                                         int width, int height, double fps,
                                         qint64 firstFrameUs, const QString &backend,
                                         int rotation, bool flipHorizontal)
{
    return QJsonObject{
        {QStringLiteral("device"), device},
        {QStringLiteral("requestedHeight"), requestedHeight == 720 ? 720 : 1080},
        {QStringLiteral("width"), width},
        {QStringLiteral("height"), height},
        {QStringLiteral("fps"), fps},
        {QStringLiteral("first_frame_us"), firstFrameUs},
        {QStringLiteral("backend"), backend},
        {QStringLiteral("rotation"), rotation},
        {QStringLiteral("flipHorizontal"), flipHorizontal}
    };
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
                                 << QStringLiteral("--webcam-height") << QString::number(options.webcamHeight)
                                 << QStringLiteral("--webcam-rotation") << QString::number(options.webcamRotation);
    if (options.webcam && options.webcamFlipHorizontal)
        arguments << QStringLiteral("--webcam-flip-horizontal");
    if (!options.selfView) arguments << QStringLiteral("--no-selfview");
    arguments << QStringLiteral("--selfview-size") << options.selfViewSize;
    arguments << QStringLiteral("--capture-backend") << options.captureBackend;
    if (options.noOpen) arguments << QStringLiteral("--no-open");
    if (options.noBar) arguments << QStringLiteral("--no-bar");
    if (options.countdown) arguments << QStringLiteral("--countdown");
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
    const qint64 spawnUs = CursorSampler::monotonicUs();
    debugStartStage(QStringLiteral("daemon_spawn"), spawnUs,
                    QStringLiteral("pid=%1").arg(pid));
    QElapsedTimer readyTimer;
    readyTimer.start();
    bool stateAppeared = false;
    const int startupTimeoutMs = options.countdown ? 7000 : 3000;
    while (readyTimer.elapsed() < startupTimeoutMs) {
        const auto state = readState();
        if (state.value("pid").toVariant().toLongLong() == pid) {
            stateAppeared = true;
            if (state.value(QStringLiteral("capture_started")).toBool(false)) {
                debugStartStage(QStringLiteral("cli_ready"), spawnUs);
                if (message) {
                    *message = selectionNote.isEmpty()
                        ? QStringLiteral("Recording started")
                        : selectionNote + QLatin1Char('\n') + QStringLiteral("Recording started");
                }
                return 0;
            }
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
    if (message) *message = QStringLiteral("Recorder daemon did not start capture in time");
    return 2;
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
    const QVector<qint64> descendants = processDescendants(pid);
    for (qint64 child : descendants) {
        QFile commandLine(QStringLiteral("/proc/%1/cmdline").arg(child));
        if (!commandLine.open(QIODevice::ReadOnly)) continue;
        const QByteArray command = commandLine.readAll();
        if (command.contains("ffmpeg") || command.contains("__record-gsr"))
            ::kill(pid_t(child), SIGKILL);
    }
    ::kill(pid_t(pid), SIGKILL);
    QFile::remove(stateFilePath());
    const QString timeoutMessage = QStringLiteral(
        "Recorder did not stop within 20 seconds; force-killed the recorder and capture processes");
    if (error) *error = timeoutMessage;
    notifyStopFailure(timeoutMessage + QStringLiteral(". The video may be incomplete."));
    return 2;
}

bool Recorder::discardTimeline(const DiscardActions &actions)
{
    if (actions.abortCapture) actions.abortCapture();
    if (actions.killAudio) actions.killAudio();
    if (actions.requestCameraStop) actions.requestCameraStop();
    if (actions.waitForCameraStop) actions.waitForCameraStop(1000);
    const bool removed = !actions.removeBundle || actions.removeBundle();
    if (actions.removeState) actions.removeState();
    return removed;
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

static QString commandOutput(const QString &program, const QStringList &arguments)
{
    QProcess process;
    process.start(program, arguments);
    if (!process.waitForFinished(3000) || process.exitCode() != 0) return {};
    return QString::fromUtf8(process.readAllStandardOutput()).trimmed();
}

static bool writeStartTimestamp(const QString &path, qint64 monotonic)
{
    timespec realtime{};
    clock_gettime(CLOCK_REALTIME, &realtime);
    const qint64 realtimeMicroseconds = qint64(realtime.tv_sec) * 1000000 + realtime.tv_nsec / 1000;
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) return false;
    QTextStream stream(&file);
    stream << "monotonic_microsec\trealtime_microsec\n"
           << monotonic << '\t' << realtimeMicroseconds << '\n';
    return file.commit();
}

static bool startAudioCapture(QProcess *process, const RecordOptions &options,
                              const QString &path, qint64 *startedUs)
{
    QStringList sources;
    if (options.desktopAudio) {
        const QString sink = commandOutput(QStringLiteral("pactl"), {QStringLiteral("get-default-sink")});
        if (!sink.isEmpty()) sources << sink + QStringLiteral(".monitor");
    }
    if (options.microphoneAudio) {
        QString source = options.microphoneDevice;
        if (source == QLatin1String("default_input"))
            source = commandOutput(QStringLiteral("pactl"), {QStringLiteral("get-default-source")});
        if (!source.isEmpty()) sources << source;
    }
    if (sources.isEmpty()) return false;
    QStringList arguments{QStringLiteral("-y"), QStringLiteral("-loglevel"), QStringLiteral("error")};
    for (const QString &source : std::as_const(sources))
        arguments << QStringLiteral("-thread_queue_size") << QStringLiteral("1024")
                  << QStringLiteral("-f") << QStringLiteral("pulse")
                  << QStringLiteral("-i") << source;
    if (sources.size() > 1)
        arguments << QStringLiteral("-filter_complex")
                  << QStringLiteral("[0:a][1:a]amix=inputs=2:duration=longest[a]")
                  << QStringLiteral("-map") << QStringLiteral("[a]");
    else
        arguments << QStringLiteral("-map") << QStringLiteral("0:a");
    arguments << QStringLiteral("-c:a") << QStringLiteral("aac")
              << QStringLiteral("-b:a") << QStringLiteral("192k") << path;
    process->setProcessChannelMode(QProcess::MergedChannels);
    *startedUs = CursorSampler::monotonicUs();
    process->start(QStringLiteral("ffmpeg"), arguments);
    return process->waitForStarted(5000) && writeStartTimestamp(path + QStringLiteral(".ts"), *startedUs);
}

static bool finishAudioCapture(QProcess *process, const QString &screenPath,
                               const QString &audioPath, const QString &outputPath,
                               qint64 screenStartedUs, qint64 audioStartedUs)
{
    if (process->state() != QProcess::NotRunning) {
        ::kill(pid_t(process->processId()), SIGINT);
        if (!process->waitForFinished(10000)) {
            process->kill();
            process->waitForFinished(1000);
        }
    }
    const bool cleanAudioStop = process->exitStatus() == QProcess::NormalExit
        && (process->exitCode() == 0 || process->exitCode() == 255);
    if (!cleanAudioStop
        || QFileInfo(audioPath).size() <= 0) return false;
    const double offset = (audioStartedUs - screenStartedUs) / 1000000.0;
    QProcess mux;
    mux.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-loglevel"),
        QStringLiteral("error"), QStringLiteral("-i"), screenPath, QStringLiteral("-itsoffset"),
        QString::number(offset, 'f', 6), QStringLiteral("-i"), audioPath,
        QStringLiteral("-map"), QStringLiteral("0:v:0"), QStringLiteral("-map"),
        QStringLiteral("1:a:0"), QStringLiteral("-c"), QStringLiteral("copy"),
        QStringLiteral("-shortest"), QStringLiteral("-movflags"), QStringLiteral("+faststart"),
        outputPath});
    return mux.waitForFinished(30000) && mux.exitCode() == 0 && QFileInfo(outputPath).size() > 0;
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

namespace {

struct MonitorLayout {
    QString name;
    QRectF geometry;
    QMarginsF reserved;
};

struct SelfViewPlacement {
    bool visible = false;
    bool safe = false;
    QString monitor;
    QString sizeName = QStringLiteral("M");
    int pixels = 160;
    QPoint position;
};

QList<MonitorLayout> monitorLayouts()
{
    QProcess process;
    process.start(QStringLiteral("hyprctl"), {QStringLiteral("-j"), QStringLiteral("monitors")});
    if (!process.waitForFinished(3000) || process.exitCode() != 0) return {};
    QList<MonitorLayout> result;
    for (const QJsonValue &value : QJsonDocument::fromJson(process.readAllStandardOutput()).array()) {
        const QJsonObject monitor = value.toObject();
        if (monitor.value(QStringLiteral("disabled")).toBool(false)) continue;
        const double scale = std::max(0.01, monitor.value(QStringLiteral("scale")).toDouble(1.0));
        const QJsonArray reserved = monitor.value(QStringLiteral("reserved")).toArray();
        const auto reservedAt = [&reserved](qsizetype index) {
            return index < reserved.size() ? reserved.at(index).toDouble() : 0.0;
        };
        result << MonitorLayout{
            monitor.value(QStringLiteral("name")).toString(),
            QRectF(monitor.value(QStringLiteral("x")).toDouble(),
                   monitor.value(QStringLiteral("y")).toDouble(),
                   monitor.value(QStringLiteral("width")).toDouble() / scale,
                   monitor.value(QStringLiteral("height")).toDouble() / scale),
            QMarginsF(reservedAt(0), reservedAt(1), reservedAt(2), reservedAt(3))};
    }
    return result;
}

int selfViewPixels(const QString &sizeName)
{
    return sizeName == QLatin1String("S") ? 120 : sizeName == QLatin1String("L") ? 220 : 160;
}

SelfViewPlacement selfViewPlacement(const RecordOptions &options, const CaptureRegion &capture,
                                    const RecordingPreferences &preferences)
{
    SelfViewPlacement placement;
    placement.visible = options.webcam && options.selfView;
    placement.sizeName = options.selfViewSize;
    placement.pixels = selfViewPixels(placement.sizeName);
    if (!placement.visible) return placement;

    const QList<MonitorLayout> monitors = monitorLayouts();
    auto recorded = std::find_if(monitors.cbegin(), monitors.cend(), [&](const MonitorLayout &monitor) {
        return monitor.name == capture.monitorName;
    });
    if (recorded == monitors.cend()) return placement;

    auto target = recorded;
    placement.safe = true;
    placement.monitor = target->name;

    const auto persistedPosition = [&](int pixels) {
        const int xRange = std::max(0, qRound(target->geometry.width()) - pixels - 32);
        const int yRange = std::max(0, qRound(target->geometry.height()) - pixels - 32);
        return QPoint(16 + qRound(preferences.selfViewX * xRange),
                      16 + qRound(preferences.selfViewY * yRange));
    };
    placement.position = persistedPosition(placement.pixels);
    return placement;
}

QVector<QRect> maskedCaptureRects(const QJsonObject &state, const CaptureRegion &capture,
                                  const MonitorLayout &monitor)
{
    QVector<QRect> result;
    const double scale = std::max(0.01, capture.scale);
    const QPointF captureOrigin(capture.x - monitor.geometry.x(),
                                capture.y - monitor.geometry.y());
    const auto pixelRect = [&](const QRectF &logical) {
        const int left = qFloor((logical.x() - captureOrigin.x()) * scale);
        const int top = qFloor((logical.y() - captureOrigin.y()) * scale);
        const int right = qCeil((logical.x() + logical.width() - captureOrigin.x()) * scale);
        const int bottom = qCeil((logical.y() + logical.height() - captureOrigin.y()) * scale);
        return QRect(left, top, right - left, bottom - top)
            .intersected(QRect(0, 0, capture.physicalWidth, capture.physicalHeight));
    };
    if (state.value(QStringLiteral("bar_visible")).toBool(true)) {
        const int width = state.value(QStringLiteral("bar_width")).toInt(
            state.value(QStringLiteral("webcam")).toBool(false) ? 520 : 276);
        const int height = state.value(QStringLiteral("bar_height")).toInt(40);
        const double usableWidth = monitor.geometry.width() - monitor.reserved.left()
                                   - monitor.reserved.right();
        const QRect bar = pixelRect(QRectF(monitor.reserved.left()
                                               + (usableWidth - width) / 2.0,
                                           monitor.reserved.top() + 12.0, width, height));
        if (bar.isValid()) result << bar;
    }
    if (state.value(QStringLiteral("countdown_active")).toBool(false)) {
        const int width = state.value(QStringLiteral("countdown_width")).toInt(240);
        const int height = state.value(QStringLiteral("countdown_height")).toInt(240);
        const QRect countdown = pixelRect(QRectF(
            monitor.geometry.width() / 2.0 - width / 2.0,
            monitor.geometry.height() / 2.0 - height / 2.0, width, height));
        if (countdown.isValid()) result << countdown;
    }
    if (state.value(QStringLiteral("selfview")).toBool(false)
        && state.value(QStringLiteral("selfview_monitor")).toString() == capture.monitorName) {
        const int pixels = state.value(QStringLiteral("selfview_pixels")).toInt(160);
        const QRect selfView = pixelRect(QRectF(monitor.reserved.left()
                                                    + state.value(QStringLiteral("selfview_x")).toInt(16),
                                                monitor.reserved.top()
                                                    + state.value(QStringLiteral("selfview_y")).toInt(16),
                                                pixels, pixels));
        if (selfView.isValid()) result << selfView;
    }
    return result;
}

} // namespace

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
    const qint64 daemonStartUs = CursorSampler::monotonicUs();
    debugStartStage(QStringLiteral("daemon_start"), daemonStartUs);
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
    options.countdown = arguments.contains(QStringLiteral("--countdown"));
    options.webcam = arguments.contains(QStringLiteral("--webcam"));
    options.webcamDevice = valueAfter(arguments, QStringLiteral("--webcam-device"));
    if (options.webcamDevice.isEmpty()) options.webcamDevice = QStringLiteral("/dev/video2");
    options.webcamHeight = valueAfter(arguments, QStringLiteral("--webcam-height")).toInt();
    if (options.webcamHeight != 720) options.webcamHeight = 1080;
    options.webcamRotation = valueAfter(arguments, QStringLiteral("--webcam-rotation")).toInt();
    if (options.webcamRotation != 90 && options.webcamRotation != 180
        && options.webcamRotation != 270) options.webcamRotation = 0;
    options.webcamFlipHorizontal = arguments.contains(QStringLiteral("--webcam-flip-horizontal"));
    options.selfView = !arguments.contains(QStringLiteral("--no-selfview"));
    options.selfViewSize = valueAfter(arguments, QStringLiteral("--selfview-size")).toUpper();
    if (options.selfViewSize != QLatin1String("S") && options.selfViewSize != QLatin1String("L"))
        options.selfViewSize = QStringLiteral("M");
    options.captureBackend = valueAfter(arguments, QStringLiteral("--capture-backend"));
    if (options.captureBackend.isEmpty()) options.captureBackend = QStringLiteral("auto");
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
    RecordingPreferences preferences = RecordingPreferences::load();
    const SelfViewPlacement selfView = selfViewPlacement(options, region, preferences);

    const QDateTime now = QDateTime::currentDateTime();
    const QString displayName = QStringLiteral("Recording %1").arg(now.toString(QStringLiteral("yyyy-MM-dd HH-mm-ss")));
    QDir().mkpath(options.outputDirectory);
    QString bundle = QDir(options.outputDirectory).filePath(displayName + QStringLiteral(".omareel"));
    int suffix = 2;
    while (QFileInfo::exists(bundle))
        bundle = QDir(options.outputDirectory).filePath(displayName + QStringLiteral(" %1.omareel").arg(suffix++));
    if (!QDir().mkpath(bundle)) return 2;

    const qint64 daemonPid = QCoreApplication::applicationPid();
    const qint64 startedUs = daemonStartUs;
    QString error;

    CursorSampler cursorSampler;
    EvdevListener evdevListener;

    const QString video = bundle + QStringLiteral("/screen.mp4");
    const QString cameraVideo = bundle + QStringLiteral("/camera.mp4");
    QStringList gsr;
    if (region.mode == CaptureMode::Fullscreen) {
        gsr << QStringLiteral("-w") << region.monitorName
            << QStringLiteral("-s") << QStringLiteral("0x0");
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
    QFile debugLog(QStringLiteral("/tmp/omareel.log"));
    if (qEnvironmentVariable("OMAREEL_DEBUG") == QLatin1String("1"))
        (void)debugLog.open(QIODevice::WriteOnly | QIODevice::Append);
    bool cameraAvailable = false;
    QString cameraBackend;
    const QString environmentBackend = qEnvironmentVariable("OMAREEL_CAPTURE").toLower();
    bool nativeCapture = environmentBackend != QLatin1String("gsr")
        && options.captureBackend != QLatin1String("gsr");
    QString captureBackend = nativeCapture ? QStringLiteral("ext-image-copy-capture")
                                           : QStringLiteral("gsr");
    const bool audioRequested = options.desktopAudio || options.microphoneAudio;
    const QString nativeVideo = audioRequested
        ? bundle + QStringLiteral("/screen-video.mp4") : video;
    const QString audioVideo = bundle + QStringLiteral("/audio.m4a");
    QProcess audioRecorder;
    qint64 audioStartedUs = 0;
    bool audioActive = false;
    bool audioMuxed = !audioRequested;
    qint64 countdownEndUs = 0;
    QJsonObject state{{"pid", daemonPid}, {"bundle", bundle}, {"started_us", startedUs},
                      {"daemon_started_us", daemonStartUs},
                      {"monitor", region.monitorName}, {"no_open", options.noOpen},
                      {"webcam", options.webcam},
                      {"camera_device", options.webcamDevice},
                      {"camera_height", options.webcamHeight},
                      {"camera_rotation", options.webcamRotation},
                      {"camera_flip_horizontal", options.webcamFlipHorizontal},
                      {"capture_mode", modeName(region.mode)},
                      {"capture_x", region.x}, {"capture_y", region.y},
                      {"capture_width", region.width}, {"capture_height", region.height},
                      {"capture_backend", captureBackend},
                      {"capture_started", false},
                      {"countdown_requested", options.countdown},
                      {"countdown_active", false},
                      {"countdown_ready", false},
                      {"countdown_end_us", countdownEndUs},
                      {"countdown_width", 240}, {"countdown_height", 240},
                      {"bar_visible", !options.noBar},
                      {"bar_width", options.webcam ? 520 : 276}, {"bar_height", 40},
                      {"selfview", selfView.visible}, {"selfview_safe", selfView.safe},
                      {"selfview_monitor", selfView.monitor},
                      {"selfview_size", selfView.sizeName}, {"selfview_pixels", selfView.pixels},
                      {"selfview_x", selfView.position.x()}, {"selfview_y", selfView.position.y()},
                      {"camera_status", options.webcam ? QStringLiteral("starting")
                                                       : QStringLiteral("disabled")},
                      {"camera_record", false},
                      {"camera_stop", false}};
    if (!writeJson(stateFilePath(), state, &error)) {
        const QString reason = error.isEmpty()
            ? QStringLiteral("Could not write recorder state")
            : QStringLiteral("Could not write recorder state: %1").arg(error);
        return failRecorderStartup(reason, bundle, video, &cursorSampler, &evdevListener);
    }
    std::future<bool> barLaunch;
    if (!options.noBar || options.webcam || options.countdown) {
        QStringList barArguments{QStringLiteral("__record-bar")};
        if (options.noBar) barArguments << QStringLiteral("--hidden");
        const QString application = QCoreApplication::applicationFilePath();
        barLaunch = std::async(std::launch::async, [application, barArguments] {
            return QProcess::startDetached(application, barArguments);
        });
    }
    ScreenCapture screenCapture;
    MonitorLayout recordedMonitorLayout{
        region.monitorName, QRectF(region.x, region.y, region.width, region.height), {}};
    if (nativeCapture) {
        QRect crop;
        if (region.mode != CaptureMode::Fullscreen) {
            const QList<MonitorLayout> layouts = monitorLayouts();
            const auto monitor = std::find_if(layouts.cbegin(), layouts.cend(), [&](const MonitorLayout &layout) {
                return layout.name == region.monitorName;
            });
            if (monitor != layouts.cend()) {
                recordedMonitorLayout = *monitor;
                crop = QRect(qRound((region.x - monitor->geometry.x()) * region.scale),
                             qRound((region.y - monitor->geometry.y()) * region.scale),
                             region.physicalWidth, region.physicalHeight);
            }
        } else {
            crop = QRect(0, 0,
                         region.physicalWidth, region.physicalHeight);
        }
        ScreenCaptureConfig config{region.monitorName, nativeVideo, video + QStringLiteral(".ts"),
                                   crop, options.fps};
        if (!screenCapture.start(config, &error)) {
            nativeCapture = false;
            captureBackend = QStringLiteral("gsr");
            updateRecordingState(QJsonObject{{QStringLiteral("capture_backend"), captureBackend}});
        } else {
            screenCapture.setMaskedRects(maskedCaptureRects(readState(), region,
                                                              recordedMonitorLayout));
        }
    }
    if (!nativeCapture) {
        QStringList recorderArguments{QStringLiteral("__record-gsr")};
        recorderArguments << gsr;
        recorder.start(QCoreApplication::applicationFilePath(), recorderArguments);
        if (!recorder.waitForStarted(5000)) {
            if (options.webcam) updateRecordingState(QJsonObject{{QStringLiteral("camera_stop"), true}});
            drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
            QString reason = lastNonEmptyLine(recorderOutput);
            if (reason.isEmpty()) reason = recorder.errorString();
            return failRecorderStartup(reason, bundle, video, &cursorSampler, &evdevListener);
        }
    }
    if (nativeCapture && audioRequested)
        audioActive = startAudioCapture(&audioRecorder, options, audioVideo, &audioStartedUs);
    if (options.countdown) {
        QElapsedTimer countdownHostTimer;
        countdownHostTimer.start();
        while (!readState().value(QStringLiteral("countdown_ready")).toBool(false)
               && countdownHostTimer.elapsed() < 3000 && !stopRequested)
            QThread::msleep(20);
        countdownEndUs = CursorSampler::monotonicUs() + 3000000;
        updateRecordingState(QJsonObject{
            {QStringLiteral("countdown_active"), true},
            {QStringLiteral("countdown_end_us"), countdownEndUs}});
        debugStartStage(QStringLiteral("countdown_begin"), daemonStartUs,
                        QStringLiteral("end_us=%1").arg(countdownEndUs));
        while (CursorSampler::monotonicUs() < countdownEndUs && !stopRequested)
            QThread::msleep(20);
        updateRecordingState(QJsonObject{{QStringLiteral("countdown_active"), false}});
        QThread::msleep(80);
        debugStartStage(QStringLiteral("countdown_end"), daemonStartUs);
    }
    const qint64 recorderStartedUs = CursorSampler::monotonicUs();
    updateRecordingState(QJsonObject{{QStringLiteral("started_us"), recorderStartedUs}});
    debugStartStage(QStringLiteral("capture_start"), daemonStartUs);
    bool captureLoopFailed = false;
    if (nativeCapture) {
        QElapsedTimer seedTimer;
        seedTimer.start();
        while (screenCapture.encodedFrames() == 0 && seedTimer.elapsed() < 2500
               && !stopRequested) {
            if (!screenCapture.captureFrame(&error)) {
                captureLoopFailed = true;
                break;
            }
            QThread::msleep(1);
        }
        if (screenCapture.encodedFrames() == 0 && !captureLoopFailed) {
            error = QStringLiteral("Capture did not produce its initial frame");
            captureLoopFailed = true;
        }
    }
    if (!nativeCapture && !captureLoopFailed) {
        QElapsedTimer firstFrameTimer;
        firstFrameTimer.start();
        while (firstFrameTimestamp(video + QStringLiteral(".ts")) <= 0
               && firstFrameTimer.elapsed() < 2500 && !stopRequested) {
            if (recorder.state() == QProcess::NotRunning) {
                captureLoopFailed = true;
                break;
            }
            QThread::msleep(5);
        }
        if (firstFrameTimestamp(video + QStringLiteral(".ts")) <= 0)
            captureLoopFailed = true;
    }
    if (!captureLoopFailed) {
        updateRecordingState(QJsonObject{{QStringLiteral("capture_started"), true}});
        const qint64 firstUs = nativeCapture ? screenCapture.firstFrameUs()
                                             : firstFrameTimestamp(video + QStringLiteral(".ts"));
        debugStartStage(QStringLiteral("first_frame"), daemonStartUs,
                        QStringLiteral("first_frame_us=%1").arg(firstUs));
    }
    QString warning;
    if (!cursorSampler.start(&warning)) qWarning().noquote() << warning;
    if (!evdevListener.start(&warning)) qWarning().noquote() << warning;
    if (options.webcam)
        updateRecordingState(QJsonObject{{QStringLiteral("camera_record"), true}});
    if (!captureLoopFailed)
        runOptionalDetached(QStringLiteral("omarchy-notification-send"),
                            {QStringLiteral("-t"), QStringLiteral("3000"),
                             QStringLiteral("Recording started"),
                             QStringLiteral("Stop with the bar, the REC indicator, or your keybind")});
    runOptionalDetached(QStringLiteral("omarchy-shell"),
                        {QStringLiteral("-q"), QStringLiteral("omarchy.indicators"), QStringLiteral("refresh")});
    QVector<QRect> currentMasks;
    while (!stopRequested && !captureLoopFailed) {
        if (options.webcam && !cameraAvailable) {
            const QString cameraStatus = readState().value(QStringLiteral("camera_status")).toString();
            if (cameraStatus == QLatin1String("ready")) {
                cameraAvailable = true;
                cameraBackend = QStringLiteral("qt-multimedia");
            }
        }
        if (nativeCapture) {
            const QVector<QRect> nextMasks = maskedCaptureRects(readState(), region,
                                                                 recordedMonitorLayout);
            if (nextMasks != currentMasks) {
                screenCapture.setMaskedRects(nextMasks);
                currentMasks = nextMasks;
            }
            if (!screenCapture.captureFrame(&error)) { captureLoopFailed = true; break; }
        } else {
            if (recorder.state() == QProcess::NotRunning) { captureLoopFailed = true; break; }
            recorder.waitForFinished(0);
            drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
            QThread::msleep(40);
        }
    }
    const bool unexpectedExit = !stopRequested && captureLoopFailed;
    if (discardRequested) {
        cursorSampler.stop();
        evdevListener.stop();
        const bool removed = discardTimeline(DiscardActions{
            [&] {
                if (nativeCapture) {
                    screenCapture.abort();
                } else if (recorder.state() != QProcess::NotRunning) {
                    recorder.terminate();
                    if (!recorder.waitForFinished(1000)) {
                        recorder.kill();
                        recorder.waitForFinished(250);
                    }
                }
            },
            [&] {
                if (audioRecorder.state() == QProcess::NotRunning) return;
                audioRecorder.kill();
                audioRecorder.waitForFinished(250);
            },
            [&] {
                if (options.webcam)
                    updateRecordingState(QJsonObject{{QStringLiteral("camera_stop"), true}});
            },
            [&](int timeoutMs) {
                if (!options.webcam) return;
                QElapsedTimer cameraStopTimer;
                cameraStopTimer.start();
                while (cameraStopTimer.elapsed() < timeoutMs) {
                    const QString status = readState().value(QStringLiteral("camera_status")).toString();
                    if (status == QLatin1String("stopped") || status == QLatin1String("failed")) break;
                    QThread::msleep(20);
                }
            },
            [&] { return !QFileInfo::exists(bundle) || QDir(bundle).removeRecursively(); },
            [&] { QFile::remove(stateFilePath()); }
        });
        runOptionalDetached(QStringLiteral("omarchy-shell"),
                            {QStringLiteral("-q"), QStringLiteral("omarchy.indicators"), QStringLiteral("refresh")});
        runOptionalDetached(QStringLiteral("omarchy-notification-send"),
                            {QStringLiteral("Recording discarded"), QFileInfo(bundle).completeBaseName()});
        QTextStream(stderr) << (removed ? QStringLiteral("Recording discarded: ")
                                          : QStringLiteral("Could not discard recording: "))
                            << bundle << '\n';
        return removed ? 0 : 2;
    }
    bool forcedStop = false;
    if (options.webcam)
        updateRecordingState(QJsonObject{{QStringLiteral("camera_stop"), true}});
    if (nativeCapture) {
        if (!screenCapture.finish(&error)) captureLoopFailed = true;
        if (audioActive)
            audioMuxed = finishAudioCapture(&audioRecorder, nativeVideo, audioVideo, video,
                                            screenCapture.firstFrameUs(), audioStartedUs);
        if (!audioMuxed) {
            QFile::remove(video);
            if (nativeVideo != video) QFile::rename(nativeVideo, video);
        }
        QFile::remove(audioVideo);
        QFile::remove(audioVideo + QStringLiteral(".ts"));
        if (audioMuxed && nativeVideo != video) QFile::remove(nativeVideo);
    } else if (recorder.state() != QProcess::NotRunning) {
        ::kill(pid_t(recorder.processId()), SIGINT);
        if (!recorder.waitForFinished(5000)) {
            forcedStop = true;
            recorder.kill();
            recorder.waitForFinished(1000);
        }
    }
    if (options.webcam) {
        QElapsedTimer cameraStopTimer;
        cameraStopTimer.start();
        while (cameraStopTimer.elapsed() < 5000) {
            const QString status = readState().value(QStringLiteral("camera_status")).toString();
            if (status == QLatin1String("stopped") || status == QLatin1String("failed")) break;
            QThread::msleep(20);
        }
    }
    if (!nativeCapture) drainRecorderOutput(&recorder, &recorderOutput, &debugLog);
    const QString recorderErrorLine = nativeCapture ? error : lastNonEmptyLine(recorderOutput);
    const bool recorderFailed = nativeCapture ? captureLoopFailed
        : recorder.exitStatus() != QProcess::NormalExit || recorder.exitCode() != 0;
    const qint64 stoppedUs = CursorSampler::monotonicUs();
    cursorSampler.stop();
    evdevListener.stop();

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
    const int effectiveExitCode = nativeCapture ? (recorderFailed ? -1 : 0)
        : recorder.exitStatus() == QProcess::NormalExit ? recorder.exitCode() : -1;
    const auto exitClassification = classifyGsrExit(effectiveExitCode, stopRequested,
                                                     QFileInfo(video).size(),
                                                     durationOk ? probedDuration : 0.0);
    if (debugLog.isOpen()) {
        const char *classification = exitClassification == GsrExitClassification::UserStop ? "user-stop"
            : exitClassification == GsrExitClassification::ExternalStop ? "external-stop" : "failure";
        debugLog.write(QByteArray("omareel: capture exit classified as ") + classification
                       + ", exit=" + QByteArray::number(effectiveExitCode)
                       + ", bytes=" + QByteArray::number(QFileInfo(video).size())
                       + ", duration=" + QByteArray::number(probedDuration, 'f', 3) + "\n");
        debugLog.flush();
    }
    bool ok = !forcedStop && exitClassification != GsrExitClassification::Failure
        && firstFrameUs > 0;
    CaptureRegion recordedRegion = region;
    const int recordedFps = options.fps;
    if (nativeCapture && region.width > 0.0) {
        recordedRegion.physicalWidth = screenCapture.outputSize().width();
        recordedRegion.physicalHeight = screenCapture.outputSize().height();
        recordedRegion.scale = screenCapture.outputSize().width() / region.width;
    }
    ok = writeInputLog(bundle + QStringLiteral("/input.jsonl"), cursor, deviceEvents,
                       recordedRegion, &error) && ok;
    QJsonObject capture{
        {"version", 1}, {"backend", captureBackend}, {"fps", recordedFps},
        {"width", nativeCapture ? screenCapture.outputSize().width() : region.physicalWidth},
        {"height", nativeCapture ? screenCapture.outputSize().height() : region.physicalHeight},
        {"mode", modeName(region.mode)},
        {"region", QJsonObject{{"x", region.x}, {"y", region.y},
                                {"w", region.width}, {"h", region.height}}},
        {"scale", recordedRegion.scale}, {"monitor", region.monitorName},
        {"first_frame_us", firstFrameUs}, {"started_us", startedUs}, {"stopped_us", stoppedUs},
        {"audio", QJsonObject{{"desktop", options.desktopAudio && (!nativeCapture || audioMuxed)},
                                {"mic", options.microphoneAudio && (!nativeCapture || audioMuxed)},
                                {"microphoneDevice", options.microphoneDevice}}}
    };
    if (nativeCapture) {
        const qint64 capturedSpanUs = std::max<qint64>(1,
            screenCapture.lastFrameUs() - screenCapture.firstFrameUs());
        const double measuredRate = screenCapture.encodedFrames() > 1
            ? (screenCapture.encodedFrames() - 1) * 1000000.0 / capturedSpanUs : 0.0;
        capture.insert(QStringLiteral("capture"), QJsonObject{
            {QStringLiteral("frames"), screenCapture.encodedFrames()},
            {QStringLiteral("drops"), screenCapture.droppedFrames()},
            {QStringLiteral("measuredFps"), measuredRate},
            {QStringLiteral("conversion"), screenCapture.conversionMode()}
        });
        if (audioRequested && audioStartedUs > 0)
            capture[QStringLiteral("audio")] = QJsonObject{
                {QStringLiteral("desktop"), options.desktopAudio && audioMuxed},
                {QStringLiteral("mic"), options.microphoneAudio && audioMuxed},
                {QStringLiteral("microphoneDevice"), options.microphoneDevice},
                {QStringLiteral("startOffsetUs"), audioStartedUs - screenCapture.firstFrameUs()}
            };
    }
    if (options.webcam && QFileInfo(cameraVideo).size() > 0) {
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
            if (!stream.isEmpty()) {
                const QStringList rate = stream.value(QStringLiteral("r_frame_rate")).toString().split('/');
                const double fps = rate.size() == 2 && rate[1].toDouble() != 0.0
                    ? rate[0].toDouble() / rate[1].toDouble() : 30.0;
                const QJsonObject cameraState = readState();
                if (cameraBackend.isEmpty())
                    cameraBackend = cameraState.value(QStringLiteral("camera_backend"))
                                        .toString(QStringLiteral("qt-multimedia"));
                capture.insert(QStringLiteral("camera"), cameraCaptureBlock(
                    options.webcamDevice, options.webcamHeight,
                    stream.value(QStringLiteral("width")).toInt(),
                    stream.value(QStringLiteral("height")).toInt(), fps, cameraFirstFrameUs,
                    cameraBackend,
                    cameraState.value(QStringLiteral("camera_rotation")).toInt(options.webcamRotation),
                    cameraState.value(QStringLiteral("camera_flip_horizontal"))
                        .toBool(options.webcamFlipHorizontal)));
            } else {
                QFile::remove(cameraVideo);
                QFile::remove(cameraVideo + QStringLiteral(".ts"));
            }
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
            debugLog.write(QStringLiteral("omareel: Recording saved: %1\n").arg(bundle).toUtf8());
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
            : QStringLiteral("The recording could not be finalized. Set OMAREEL_DEBUG=1 and check /tmp/omareel.log.");
        if ((unexpectedExit || recorderFailed) && !recorderErrorLine.isEmpty())
            detail += QStringLiteral("\ngpu-screen-recorder: %1").arg(recorderErrorLine);
        notifyStopFailure(detail);
        writeLastError(detail);
    }
    if (debugLog.isOpen()) {
        debugLog.write(QByteArray("omareel: daemon exit=") + (ok ? "0\n" : "2\n"));
        debugLog.flush();
    }
    return ok ? 0 : 2;
}
