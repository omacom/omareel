#include "ScreenCapture.h"

#include "ext-image-capture-source-v1-client-protocol.h"
#include "ext-image-copy-capture-v1-client-protocol.h"

#include <QByteArray>
#include <QDebug>
#include <QElapsedTimer>
#include <QProcess>
#include <QSaveFile>
#include <QTextStream>
#include <algorithm>
#include <atomic>
#include <cerrno>
#include <condition_variable>
#include <cstring>
#include <fcntl.h>
#include <mutex>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <thread>
#include <time.h>
#include <unistd.h>
#include <vector>
#include <wayland-client.h>

using namespace Omareel;

extern char **environ;

CaptureRingBookkeeping::CaptureRingBookkeeping(int slotCount)
    : m_states(std::max(1, slotCount), State::Available)
{
}

int CaptureRingBookkeeping::acquire()
{
    for (int index = 0; index < m_states.size(); ++index) {
        if (m_states[index] != State::Available) continue;
        m_states[index] = State::Capturing;
        return index;
    }
    ++m_drops;
    return -1;
}

bool CaptureRingBookkeeping::markQueued(int slot)
{
    if (slot < 0 || slot >= m_states.size() || m_states[slot] != State::Capturing)
        return false;
    m_states[slot] = State::Queued;
    m_queue.append(slot);
    return true;
}

int CaptureRingBookkeeping::takeQueued()
{
    if (m_queue.isEmpty()) return -1;
    const int slot = m_queue.takeFirst();
    if (slot < 0 || slot >= m_states.size() || m_states[slot] != State::Queued)
        return -1;
    m_states[slot] = State::Writing;
    return slot;
}

bool CaptureRingBookkeeping::release(int slot)
{
    if (slot < 0 || slot >= m_states.size() || m_states[slot] == State::Available)
        return false;
    m_states[slot] = State::Available;
    return true;
}

CaptureRingBookkeeping::State CaptureRingBookkeeping::state(int slot) const
{
    return slot >= 0 && slot < m_states.size() ? m_states[slot] : State::Available;
}

int CaptureRingBookkeeping::count(State state) const
{
    return int(std::count(m_states.cbegin(), m_states.cend(), state));
}

QVector<CaptureRowCopy> Omareel::captureCropRows(const QSize &sourceSize, int sourceStride,
                                                   const QRect &requestedCrop)
{
    QVector<CaptureRowCopy> result;
    if (sourceSize.isEmpty() || sourceStride < sourceSize.width() * 4) return result;
    const QRect crop = requestedCrop.isValid()
        ? requestedCrop.intersected(QRect(QPoint(), sourceSize)) : QRect(QPoint(), sourceSize);
    if (!crop.isValid()) return result;
    const qsizetype rowBytes = qsizetype(crop.width()) * 4;
    if (crop.x() == 0 && rowBytes == sourceStride) {
        result << CaptureRowCopy{qsizetype(crop.y()) * sourceStride, 0,
                                 rowBytes * crop.height()};
        return result;
    }
    result.reserve(crop.height());
    for (int row = 0; row < crop.height(); ++row) {
        result << CaptureRowCopy{
            qsizetype(crop.y() + row) * sourceStride + qsizetype(crop.x()) * 4,
            qsizetype(row) * rowBytes,
            rowBytes};
    }
    return result;
}

namespace {

qint64 monotonicUs()
{
    timespec value{};
    clock_gettime(CLOCK_MONOTONIC, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

qint64 realtimeUs()
{
    timespec value{};
    clock_gettime(CLOCK_REALTIME, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

bool preferredEncoderAvailable()
{
    QProcess probe;
    probe.start(QStringLiteral("ffmpeg"), {QStringLiteral("-hide_banner"), QStringLiteral("-loglevel"),
        QStringLiteral("error"), QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"),
        QStringLiteral("color=size=256x256:rate=1"), QStringLiteral("-frames:v"), QStringLiteral("1"),
        QStringLiteral("-c:v"), QStringLiteral("h264_nvenc"), QStringLiteral("-f"),
        QStringLiteral("null"), QStringLiteral("-")});
    return probe.waitForFinished(10000) && probe.exitCode() == 0;
}

bool gpuConversionAvailable()
{
    QProcess probe;
    probe.start(QStringLiteral("ffmpeg"), {QStringLiteral("-hide_banner"), QStringLiteral("-loglevel"),
        QStringLiteral("error"), QStringLiteral("-f"), QStringLiteral("rawvideo"),
        QStringLiteral("-pix_fmt"), QStringLiteral("bgra"), QStringLiteral("-s"),
        QStringLiteral("256x256"), QStringLiteral("-r"), QStringLiteral("1"),
        QStringLiteral("-i"), QStringLiteral("pipe:0"), QStringLiteral("-frames:v"), QStringLiteral("1"),
        QStringLiteral("-vf"), QStringLiteral("hwupload_cuda,scale_cuda=format=nv12"),
        QStringLiteral("-c:v"), QStringLiteral("h264_nvenc"), QStringLiteral("-f"),
        QStringLiteral("null"), QStringLiteral("-")});
    if (!probe.waitForStarted(5000)) return false;
    probe.write(QByteArray(256 * 256 * 4, '\0'));
    probe.closeWriteChannel();
    return probe.waitForFinished(10000) && probe.exitCode() == 0;
}

struct Output {
    wl_output *object = nullptr;
    QString name;
};

struct RegistryState {
    wl_display *display = nullptr;
    wl_registry *registry = nullptr;
    wl_shm *shm = nullptr;
    ext_image_copy_capture_manager_v1 *captureManager = nullptr;
    ext_output_image_capture_source_manager_v1 *sourceManager = nullptr;
    std::vector<Output> outputs;
};

void outputGeometry(void *, wl_output *, int32_t, int32_t, int32_t, int32_t, int32_t,
                    const char *, const char *, int32_t) {}
void outputMode(void *, wl_output *, uint32_t, int32_t, int32_t, int32_t) {}
void outputDone(void *, wl_output *) {}
void outputScale(void *, wl_output *, int32_t) {}
void outputName(void *data, wl_output *output, const char *name)
{
    auto *state = static_cast<RegistryState *>(data);
    for (auto &entry : state->outputs)
        if (entry.object == output) entry.name = QString::fromUtf8(name);
}
void outputDescription(void *, wl_output *, const char *) {}

const wl_output_listener outputListener{
    outputGeometry, outputMode, outputDone, outputScale, outputName, outputDescription
};

void registryGlobal(void *data, wl_registry *registry, uint32_t name,
                    const char *interface, uint32_t version)
{
    auto *state = static_cast<RegistryState *>(data);
    if (std::strcmp(interface, wl_shm_interface.name) == 0) {
        state->shm = static_cast<wl_shm *>(wl_registry_bind(registry, name, &wl_shm_interface, 1));
    } else if (std::strcmp(interface, ext_image_copy_capture_manager_v1_interface.name) == 0) {
        state->captureManager = static_cast<ext_image_copy_capture_manager_v1 *>(
            wl_registry_bind(registry, name, &ext_image_copy_capture_manager_v1_interface, 1));
    } else if (std::strcmp(interface, ext_output_image_capture_source_manager_v1_interface.name) == 0) {
        state->sourceManager = static_cast<ext_output_image_capture_source_manager_v1 *>(
            wl_registry_bind(registry, name, &ext_output_image_capture_source_manager_v1_interface, 1));
    } else if (std::strcmp(interface, wl_output_interface.name) == 0) {
        auto *output = static_cast<wl_output *>(wl_registry_bind(
            registry, name, &wl_output_interface, std::min(version, 4u)));
        state->outputs.push_back(Output{output, {}});
        wl_output_add_listener(output, &outputListener, state);
    }
}

void registryRemove(void *, wl_registry *, uint32_t) {}
const wl_registry_listener registryListener{registryGlobal, registryRemove};

bool connectRegistry(RegistryState *state)
{
    state->display = wl_display_connect(nullptr);
    if (!state->display) return false;
    state->registry = wl_display_get_registry(state->display);
    wl_registry_add_listener(state->registry, &registryListener, state);
    return wl_display_roundtrip(state->display) >= 0 && wl_display_roundtrip(state->display) >= 0;
}

void releaseRegistry(RegistryState *state)
{
    for (auto &output : state->outputs) if (output.object) wl_output_destroy(output.object);
    if (state->sourceManager) ext_output_image_capture_source_manager_v1_destroy(state->sourceManager);
    if (state->captureManager) ext_image_copy_capture_manager_v1_destroy(state->captureManager);
    if (state->shm) wl_shm_destroy(state->shm);
    if (state->registry) wl_registry_destroy(state->registry);
    if (state->display) wl_display_disconnect(state->display);
    *state = {};
}

bool startEncoder(const QStringList &arguments, pid_t *pid, int *inputFd, int *errorFd,
                  QString *error)
{
    int inputPipe[2] = {-1, -1};
    int errorPipe[2] = {-1, -1};
    if (pipe2(inputPipe, O_CLOEXEC) != 0 || pipe2(errorPipe, O_CLOEXEC) != 0) {
        if (inputPipe[0] >= 0) { close(inputPipe[0]); close(inputPipe[1]); }
        if (errorPipe[0] >= 0) { close(errorPipe[0]); close(errorPipe[1]); }
        if (error) *error = QString::fromLocal8Bit(std::strerror(errno));
        return false;
    }

    std::vector<QByteArray> encoded;
    encoded.reserve(size_t(arguments.size() + 1));
    encoded.push_back(QByteArrayLiteral("ffmpeg"));
    for (const QString &argument : arguments) encoded.push_back(argument.toLocal8Bit());
    std::vector<char *> argv;
    argv.reserve(encoded.size() + 1);
    for (QByteArray &argument : encoded) argv.push_back(argument.data());
    argv.push_back(nullptr);

    posix_spawn_file_actions_t actions;
    posix_spawn_file_actions_init(&actions);
    posix_spawn_file_actions_adddup2(&actions, inputPipe[0], STDIN_FILENO);
    posix_spawn_file_actions_adddup2(&actions, errorPipe[1], STDERR_FILENO);
    posix_spawn_file_actions_addclose(&actions, inputPipe[1]);
    posix_spawn_file_actions_addclose(&actions, errorPipe[0]);
    const int result = posix_spawnp(pid, "ffmpeg", &actions, nullptr, argv.data(), environ);
    posix_spawn_file_actions_destroy(&actions);
    close(inputPipe[0]);
    close(errorPipe[1]);
    if (result != 0) {
        close(inputPipe[1]);
        close(errorPipe[0]);
        if (error) *error = QString::fromLocal8Bit(std::strerror(result));
        return false;
    }
    const int inputFlags = fcntl(inputPipe[1], F_GETFL, 0);
    const int errorFlags = fcntl(errorPipe[0], F_GETFL, 0);
    fcntl(inputPipe[1], F_SETFL, inputFlags | O_NONBLOCK);
    fcntl(errorPipe[0], F_SETFL, errorFlags | O_NONBLOCK);
    *inputFd = inputPipe[1];
    *errorFd = errorPipe[0];
    return true;
}

bool writeTimestamp(const QString &path, qint64 monotonic, QString *error)
{
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) {
        if (error) *error = file.errorString();
        return false;
    }
    QTextStream stream(&file);
    stream << "monotonic_microsec\trealtime_microsec\n" << monotonic << '\t' << realtimeUs() << '\n';
    if (!file.commit()) {
        if (error) *error = file.errorString();
        return false;
    }
    return true;
}

bool writeAll(int fd, const char *bytes, qsizetype size, const std::atomic_bool &abort,
              QString *error)
{
    qsizetype written = 0;
    QElapsedTimer stall;
    stall.start();
    while (written < size && !abort.load(std::memory_order_relaxed)) {
        const ssize_t count = ::write(fd, bytes + written, size_t(size - written));
        if (count > 0) {
            written += count;
            stall.restart();
            continue;
        }
        if (count < 0 && errno == EINTR) continue;
        if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
            pollfd descriptor{fd, POLLOUT, 0};
            const int result = poll(&descriptor, 1, 100);
            if (result >= 0 && stall.elapsed() < 10000) continue;
            if (error) *error = result < 0
                ? QString::fromLocal8Bit(std::strerror(errno))
                : QStringLiteral("Encoder input stalled for ten seconds");
            return false;
        }
        if (error) *error = QString::fromLocal8Bit(std::strerror(errno));
        return false;
    }
    if (written == size) return true;
    if (error) *error = QStringLiteral("Encoder input was interrupted");
    return false;
}

} // namespace

struct ScreenCapture::Private {
    struct Slot {
        Private *owner = nullptr;
        int index = -1;
        ext_image_copy_capture_frame_v1 *frame = nullptr;
        wl_buffer *buffer = nullptr;
        uchar *memory = nullptr;
        bool ready = false;
        bool failed = false;
        qint64 presentationUs = 0;
    };

    RegistryState registry;
    ext_image_capture_source_v1 *source = nullptr;
    ext_image_copy_capture_session_v1 *session = nullptr;
    wl_shm_pool *pool = nullptr;
    int memoryFd = -1;
    uchar *memory = nullptr;
    qsizetype memorySize = 0;
    qsizetype slotBytes = 0;
    int width = 0;
    int height = 0;
    int stride = 0;
    std::vector<uint32_t> shmFormats;
    bool constraintsDone = false;
    bool stopped = false;
    ScreenCaptureConfig config;
    QRect crop;
    QSize encodedSize;
    QVector<CaptureRowCopy> cropRows;
    std::vector<Slot> captureSlots;
    CaptureRingBookkeeping ring{4};
    std::mutex ringMutex;
    std::condition_variable queuedFrame;
    std::thread writer;
    bool writerStopping = false;
    std::atomic_bool writerAbort{false};
    std::atomic_bool writerFailed{false};
    QString writerError;
    pid_t encoderPid = -1;
    int encoderFd = -1;
    int encoderErrorFd = -1;
    QByteArray encoderError;
    QString conversion = QStringLiteral("cpu");
    qint64 firstUs = 0;
    qint64 lastUs = 0;
    qint64 lastCaptureRequestUs = 0;
    std::atomic_int frames{0};
    int captureFailures = 0;
    QElapsedTimer rateClock;
    int rateFrames = 0;
    int rateDrops = 0;
    double lastRate = 0.0;

    void writerLoop();
    void readEncoderError();
    bool stopEncoder(bool drain, QString *error);
    void cleanup();
};

namespace {

void sessionSize(void *data, ext_image_copy_capture_session_v1 *, uint32_t width, uint32_t height)
{
    auto *d = static_cast<ScreenCapture::Private *>(data);
    d->width = int(width); d->height = int(height); d->stride = d->width * 4;
}
void sessionShmFormat(void *data, ext_image_copy_capture_session_v1 *, uint32_t format)
{ static_cast<ScreenCapture::Private *>(data)->shmFormats.push_back(format); }
void sessionDmabufDevice(void *, ext_image_copy_capture_session_v1 *, wl_array *) {}
void sessionDmabufFormat(void *, ext_image_copy_capture_session_v1 *, uint32_t, wl_array *) {}
void sessionDone(void *data, ext_image_copy_capture_session_v1 *)
{ static_cast<ScreenCapture::Private *>(data)->constraintsDone = true; }
void sessionStopped(void *data, ext_image_copy_capture_session_v1 *)
{ static_cast<ScreenCapture::Private *>(data)->stopped = true; }

const ext_image_copy_capture_session_v1_listener sessionListener{
    sessionSize, sessionShmFormat, sessionDmabufDevice, sessionDmabufFormat,
    sessionDone, sessionStopped
};

void frameTransform(void *, ext_image_copy_capture_frame_v1 *, uint32_t) {}
void frameDamage(void *, ext_image_copy_capture_frame_v1 *, int32_t, int32_t, int32_t, int32_t) {}
void framePresentation(void *data, ext_image_copy_capture_frame_v1 *, uint32_t high,
                       uint32_t low, uint32_t nanoseconds)
{
    auto *slot = static_cast<ScreenCapture::Private::Slot *>(data);
    const quint64 seconds = (quint64(high) << 32) | low;
    slot->presentationUs = qint64(seconds * 1000000ULL + nanoseconds / 1000);
}
void frameReady(void *data, ext_image_copy_capture_frame_v1 *)
{ static_cast<ScreenCapture::Private::Slot *>(data)->ready = true; }
void frameFailed(void *data, ext_image_copy_capture_frame_v1 *, uint32_t)
{ static_cast<ScreenCapture::Private::Slot *>(data)->failed = true; }

const ext_image_copy_capture_frame_v1_listener frameListener{
    frameTransform, frameDamage, framePresentation, frameReady, frameFailed
};

bool dispatchOnce(ScreenCapture::Private *d, int timeoutMs)
{
    if (wl_display_dispatch_pending(d->registry.display) < 0) return false;
    if (wl_display_flush(d->registry.display) < 0 && errno != EAGAIN) return false;
    pollfd descriptor{wl_display_get_fd(d->registry.display), POLLIN, 0};
    const int result = poll(&descriptor, 1, timeoutMs);
    if (result < 0) return errno == EINTR;
    if (result == 0) return true;
    // A stop signal (SIGUSR1/2) can land while dispatching; that is not a connection failure.
    if (wl_display_dispatch(d->registry.display) < 0) return errno == EINTR;
    return true;
}

} // namespace

void ScreenCapture::Private::writerLoop()
{
    QByteArray cropped;
    if (cropRows.size() > 1)
        cropped.resize(encodedSize.width() * encodedSize.height() * 4);
    for (;;) {
        int index = -1;
        {
            std::unique_lock<std::mutex> lock(ringMutex);
            queuedFrame.wait(lock, [this] {
                return writerStopping || ring.count(CaptureRingBookkeeping::State::Queued) > 0;
            });
            index = ring.takeQueued();
            if (index < 0 && writerStopping) break;
        }
        if (index < 0) continue;

        const char *bytes = reinterpret_cast<const char *>(captureSlots[size_t(index)].memory);
        const char *output = bytes + cropRows.first().sourceOffset;
        qsizetype outputBytes = cropRows.first().bytes;
        if (cropRows.size() > 1) {
            for (const CaptureRowCopy &row : std::as_const(cropRows))
                std::memcpy(cropped.data() + row.destinationOffset,
                            bytes + row.sourceOffset, size_t(row.bytes));
            output = cropped.constData();
            outputBytes = cropped.size();
        }
        QString error;
        const bool wrote = writeAll(encoderFd, output, outputBytes, writerAbort, &error);
        {
            std::lock_guard<std::mutex> lock(ringMutex);
            ring.release(index);
            if (!wrote) {
                writerError = error;
                writerFailed.store(true, std::memory_order_relaxed);
                writerStopping = true;
            }
        }
        if (!wrote) break;
        frames.fetch_add(1, std::memory_order_relaxed);
    }
}

void ScreenCapture::Private::readEncoderError()
{
    if (encoderErrorFd < 0) return;
    char buffer[4096];
    for (int attempt = 0; attempt < 256; ++attempt) {
        const ssize_t count = read(encoderErrorFd, buffer, sizeof(buffer));
        if (count > 0) {
            encoderError.append(buffer, count);
            continue;
        }
        if (count < 0 && errno == EINTR) continue;
        break;
    }
}

bool ScreenCapture::Private::stopEncoder(bool drain, QString *error)
{
    if (writer.joinable()) {
        if (!drain) writerAbort.store(true, std::memory_order_relaxed);
        {
            std::lock_guard<std::mutex> lock(ringMutex);
            writerStopping = true;
        }
        queuedFrame.notify_all();
        if (!drain && encoderPid > 1) kill(encoderPid, SIGTERM);
        writer.join();
    }
    if (encoderFd >= 0) {
        close(encoderFd);
        encoderFd = -1;
    }
    if (encoderPid <= 1) return !writerFailed.load(std::memory_order_relaxed);

    int status = 0;
    QElapsedTimer wait;
    wait.start();
    pid_t result = 0;
    const int timeoutMs = drain ? 15000 : 1000;
    while (wait.elapsed() < timeoutMs) {
        readEncoderError();
        result = waitpid(encoderPid, &status, WNOHANG);
        if (result == encoderPid || result < 0) break;
        timespec delay{0, 10000000};
        nanosleep(&delay, nullptr);
    }
    if (result == 0) {
        kill(encoderPid, SIGKILL);
        waitpid(encoderPid, &status, 0);
        if (error) *error = QStringLiteral("Timed out finalizing the screen video");
        encoderPid = -1;
        return false;
    }
    readEncoderError();
    if (encoderErrorFd >= 0) {
        close(encoderErrorFd);
        encoderErrorFd = -1;
    }
    encoderPid = -1;
    if (writerFailed.load(std::memory_order_relaxed)) {
        if (error) {
            *error = writerError;
            const QString detail = QString::fromUtf8(encoderError).trimmed();
            if (!detail.isEmpty()) *error += QLatin1Char('\n') + detail;
        }
        return false;
    }
    if (result < 0 || !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        if (error) {
            *error = QString::fromUtf8(encoderError).trimmed();
            if (error->isEmpty()) *error = QStringLiteral("The screen encoder exited unexpectedly");
        }
        return false;
    }
    return true;
}

void ScreenCapture::Private::cleanup()
{
    stopEncoder(false, nullptr);
    for (Slot &slot : captureSlots) {
        if (slot.frame) ext_image_copy_capture_frame_v1_destroy(slot.frame);
        if (slot.buffer) wl_buffer_destroy(slot.buffer);
    }
    captureSlots.clear();
    if (pool) wl_shm_pool_destroy(pool);
    if (memory && memory != MAP_FAILED) munmap(memory, size_t(memorySize));
    if (memoryFd >= 0) close(memoryFd);
    if (session) ext_image_copy_capture_session_v1_destroy(session);
    if (source) ext_image_capture_source_v1_destroy(source);
    pool = nullptr; memory = nullptr; memoryFd = -1;
    session = nullptr; source = nullptr;
    releaseRegistry(&registry);
}

ScreenCapture::ScreenCapture(): d(std::make_unique<Private>()) {}
ScreenCapture::~ScreenCapture() { d->cleanup(); }

bool ScreenCapture::isSupported()
{
    RegistryState registry;
    const bool connected = connectRegistry(&registry);
    const bool supported = connected && registry.captureManager && registry.sourceManager && registry.shm;
    releaseRegistry(&registry);
    return supported;
}

bool ScreenCapture::start(const ScreenCaptureConfig &config, QString *error)
{
    d->config = config;
    d->config.fps = std::max(1, config.fps);
    if (!connectRegistry(&d->registry)) {
        if (error) *error = QStringLiteral("Could not connect to the display server");
        return false;
    }
    if (!d->registry.captureManager || !d->registry.sourceManager || !d->registry.shm) {
        if (error) *error = QStringLiteral("The in-process capture protocol is unavailable");
        return false;
    }
    auto output = std::find_if(d->registry.outputs.begin(), d->registry.outputs.end(),
        [&](const Output &candidate) { return candidate.name == config.monitor; });
    if (output == d->registry.outputs.end()) {
        if (error) *error = QStringLiteral("The requested monitor is unavailable");
        return false;
    }
    d->source = ext_output_image_capture_source_manager_v1_create_source(
        d->registry.sourceManager, output->object);
    d->session = ext_image_copy_capture_manager_v1_create_session(
        d->registry.captureManager, d->source, 0);
    ext_image_copy_capture_session_v1_add_listener(d->session, &sessionListener, d.get());
    while (!d->constraintsDone && !d->stopped) {
        if (!dispatchOnce(d.get(), 100)) {
            if (error) *error = QStringLiteral("Capture setup failed");
            return false;
        }
    }
    if (d->stopped || d->width <= 0 || d->height <= 0) {
        if (error) *error = QStringLiteral("Capture stopped during setup");
        return false;
    }

    uint32_t format = WL_SHM_FORMAT_XRGB8888;
    if (std::find(d->shmFormats.begin(), d->shmFormats.end(), format) == d->shmFormats.end()) {
        format = WL_SHM_FORMAT_ARGB8888;
        if (std::find(d->shmFormats.begin(), d->shmFormats.end(), format) == d->shmFormats.end()) {
            if (error) *error = QStringLiteral("No supported shared-memory capture format was offered");
            return false;
        }
    }
    d->crop = config.crop.isValid() ? config.crop.intersected(QRect(0, 0, d->width, d->height))
                                    : QRect(0, 0, d->width, d->height);
    if (!d->crop.isValid()) {
        if (error) *error = QStringLiteral("The capture region is outside the monitor");
        return false;
    }
    d->encodedSize = d->crop.size();
    d->cropRows = captureCropRows(QSize(d->width, d->height), d->stride, d->crop);
    if (d->cropRows.isEmpty()) {
        if (error) *error = QStringLiteral("The capture crop could not be prepared");
        return false;
    }

    constexpr int slotCount = 4;
    d->slotBytes = qsizetype(d->stride) * d->height;
    d->memorySize = d->slotBytes * slotCount;
    d->memoryFd = memfd_create("omareel-capture", MFD_CLOEXEC);
    if (d->memoryFd < 0 || ftruncate(d->memoryFd, off_t(d->memorySize)) != 0) {
        if (error) *error = QString::fromLocal8Bit(std::strerror(errno));
        return false;
    }
    d->memory = static_cast<uchar *>(mmap(nullptr, size_t(d->memorySize),
        PROT_READ | PROT_WRITE, MAP_SHARED, d->memoryFd, 0));
    if (d->memory == MAP_FAILED) {
        if (error) *error = QString::fromLocal8Bit(std::strerror(errno));
        return false;
    }
    d->pool = wl_shm_create_pool(d->registry.shm, d->memoryFd, int(d->memorySize));
    d->captureSlots.resize(slotCount);
    for (int index = 0; index < slotCount; ++index) {
        Private::Slot &slot = d->captureSlots[size_t(index)];
        slot.owner = d.get();
        slot.index = index;
        slot.memory = d->memory + d->slotBytes * index;
        slot.buffer = wl_shm_pool_create_buffer(d->pool, int(d->slotBytes * index),
            d->width, d->height, d->stride, format);
    }

    const bool preferred = preferredEncoderAvailable();
    const QString requestedConversion = qEnvironmentVariable("OMAREEL_NATIVE_CONVERSION").toLower();
    const bool useGpuConversion = preferred && requestedConversion != QLatin1String("cpu")
        && gpuConversionAvailable();
    d->conversion = useGpuConversion ? QStringLiteral("gpu") : QStringLiteral("cpu");
    QStringList arguments{QStringLiteral("-y"), QStringLiteral("-loglevel"), QStringLiteral("error"),
        QStringLiteral("-f"), QStringLiteral("rawvideo"), QStringLiteral("-pix_fmt"),
        QStringLiteral("bgra"), QStringLiteral("-s"),
        QStringLiteral("%1x%2").arg(d->encodedSize.width()).arg(d->encodedSize.height()),
        QStringLiteral("-r"), QString::number(d->config.fps), QStringLiteral("-i"),
        QStringLiteral("pipe:0"), QStringLiteral("-an")};
    if (useGpuConversion)
        arguments << QStringLiteral("-vf") << QStringLiteral("hwupload_cuda,scale_cuda=format=nv12");
    arguments << QStringLiteral("-c:v");
    // Quality: the capture is an intermediate that gets re-encoded on export, so keep it near
    // visually lossless. A plain -cq without a rate mode made NVENC starve small regions (a
    // 440x234 capture came out at ~110 kbit/s with visible macroblocks), so pin VBR with a
    // generous ceiling scaled to the frame size, plus a 1-second GOP for clean seeking.
    const double megapixels = d->encodedSize.width() * double(d->encodedSize.height()) / 1e6;
    const int maxrateKbps = int(std::clamp(megapixels * 12000.0 * (d->config.fps / 60.0), 8000.0, 120000.0));
    // Debug aid: OMAREEL_NATIVE_ENCODER=lossless stores the raw capture losslessly so capture
    // problems can be told apart from encoder artefacts.
    const bool lossless = qEnvironmentVariable("OMAREEL_NATIVE_ENCODER") == QLatin1String("lossless");
    // Small captures (regions, windows) are cheap enough to store losslessly, and NVENC's lossy
    // modes smear high-contrast edges into visible macroblocks at these sizes. Larger frames use
    // a high-quality VBR tier instead.
    const bool smallFrame = megapixels < 1.5;
    if (lossless)
        arguments << QStringLiteral("libx264rgb") << QStringLiteral("-preset") << QStringLiteral("ultrafast")
                  << QStringLiteral("-qp") << QStringLiteral("0");
    else if (preferred && smallFrame)
        arguments << QStringLiteral("h264_nvenc") << QStringLiteral("-preset") << QStringLiteral("p4")
                  << QStringLiteral("-tune") << QStringLiteral("lossless")
                  << QStringLiteral("-g") << QString::number(d->config.fps)
                  << QStringLiteral("-bf") << QStringLiteral("0");
    else if (preferred)
        arguments << QStringLiteral("h264_nvenc") << QStringLiteral("-preset") << QStringLiteral("p4")
                  << QStringLiteral("-tune") << QStringLiteral("hq")
                  << QStringLiteral("-rc") << QStringLiteral("vbr")
                  << QStringLiteral("-cq") << QStringLiteral("16")
                  << QStringLiteral("-b:v") << QStringLiteral("0")
                  << QStringLiteral("-maxrate") << QStringLiteral("%1k").arg(maxrateKbps)
                  << QStringLiteral("-bufsize") << QStringLiteral("%1k").arg(maxrateKbps * 2)
                  << QStringLiteral("-g") << QString::number(d->config.fps)
                  << QStringLiteral("-bf") << QStringLiteral("0");
    else
        arguments << QStringLiteral("libx264") << QStringLiteral("-preset") << QStringLiteral("veryfast")
                  << QStringLiteral("-crf") << (smallFrame ? QStringLiteral("10") : QStringLiteral("16"))
                  << QStringLiteral("-g") << QString::number(d->config.fps);
    if (!useGpuConversion && !lossless) arguments << QStringLiteral("-pix_fmt") << QStringLiteral("yuv420p");
    arguments << QStringLiteral("-movflags") << QStringLiteral("+faststart") << config.outputPath;
    if (!startEncoder(arguments, &d->encoderPid, &d->encoderFd, &d->encoderErrorFd, error))
        return false;

    QTextStream(stderr) << QStringLiteral("capture conversion=%1 slots=%2 crop=%3x%4\n")
        .arg(d->conversion).arg(slotCount).arg(d->encodedSize.width()).arg(d->encodedSize.height());
    d->rateClock.start();
    d->writer = std::thread([state = d.get()] { state->writerLoop(); });
    return true;
}

bool ScreenCapture::captureFrame(QString *error)
{
    if (d->stopped || d->encoderPid <= 1 || d->writerFailed.load(std::memory_order_relaxed)) {
        if (error) {
            std::lock_guard<std::mutex> lock(d->ringMutex);
            *error = d->writerError.isEmpty()
                ? QStringLiteral("Capture or encoding stopped unexpectedly") : d->writerError;
        }
        return false;
    }
    const qint64 now = monotonicUs();
    const qint64 intervalUs = 1000000 / d->config.fps;
    if (d->lastCaptureRequestUs == 0 || now - d->lastCaptureRequestUs >= intervalUs) {
        // The protocol allows exactly one frame object per session; requesting another
        // while one is outstanding raises duplicate_frame and kills the connection.
        bool frameInFlight = false;
        for (const Private::Slot &slot : d->captureSlots)
            if (slot.frame) { frameInFlight = true; break; }
        int index = -1;
        if (!frameInFlight) {
            std::lock_guard<std::mutex> lock(d->ringMutex);
            index = d->ring.acquire();
        }
        if (d->lastCaptureRequestUs == 0) d->lastCaptureRequestUs = now;
        else {
            d->lastCaptureRequestUs += intervalUs;
            if (now - d->lastCaptureRequestUs > intervalUs)
                d->lastCaptureRequestUs = now;
        }
        if (index >= 0) {
            Private::Slot &slot = d->captureSlots[size_t(index)];
            slot.ready = false;
            slot.failed = false;
            slot.presentationUs = 0;
            slot.frame = ext_image_copy_capture_session_v1_create_frame(d->session);
            ext_image_copy_capture_frame_v1_add_listener(slot.frame, &frameListener, &slot);
            ext_image_copy_capture_frame_v1_attach_buffer(slot.frame, slot.buffer);
            ext_image_copy_capture_frame_v1_damage_buffer(slot.frame, 0, 0, d->width, d->height);
            ext_image_copy_capture_frame_v1_capture(slot.frame);
        }
    }
    if (!dispatchOnce(d.get(), 2)) {
        const int displayError = wl_display_get_error(d->registry.display);
        if (error) *error = QStringLiteral("Capture connection failed (errno %1: %2, display error %3)")
            .arg(errno).arg(QString::fromLocal8Bit(std::strerror(errno))).arg(displayError);
        return false;
    }

    std::vector<Private::Slot *> completed;
    for (Private::Slot &slot : d->captureSlots)
        if (slot.frame && (slot.ready || slot.failed)) completed.push_back(&slot);
    std::stable_sort(completed.begin(), completed.end(), [](const auto *left, const auto *right) {
        if (left->failed != right->failed) return !left->failed;
        return left->presentationUs < right->presentationUs;
    });
    for (Private::Slot *completedSlot : completed) {
        Private::Slot &slot = *completedSlot;
        ext_image_copy_capture_frame_v1_destroy(slot.frame);
        slot.frame = nullptr;
        if (slot.failed) {
            ++d->captureFailures;
            std::lock_guard<std::mutex> lock(d->ringMutex);
            d->ring.release(slot.index);
            continue;
        }
        if (d->firstUs == 0) {
            d->firstUs = slot.presentationUs > 0 ? slot.presentationUs : monotonicUs();
            if (!writeTimestamp(d->config.timestampPath, d->firstUs, error)) return false;
        }
        d->lastUs = slot.presentationUs > 0 ? slot.presentationUs : monotonicUs();
        {
            std::lock_guard<std::mutex> lock(d->ringMutex);
            if (!d->ring.markQueued(slot.index)) {
                if (error) *error = QStringLiteral("Capture slot state became inconsistent");
                return false;
            }
        }
        d->queuedFrame.notify_one();
    }

    if (d->rateClock.elapsed() >= 1000) {
        const int totalFrames = d->frames.load(std::memory_order_relaxed);
        int totalDrops = 0;
        {
            std::lock_guard<std::mutex> lock(d->ringMutex);
            totalDrops = d->ring.drops() + d->captureFailures;
        }
        const int intervalFrames = totalFrames - d->rateFrames;
        const int intervalDrops = totalDrops - d->rateDrops;
        d->lastRate = intervalFrames * 1000.0 / d->rateClock.elapsed();
        QTextStream(stderr) << QStringLiteral("capture frames=%1 fps=%2 drops=%3\n")
            .arg(intervalFrames).arg(d->lastRate, 0, 'f', 1).arg(intervalDrops);
        d->rateFrames = totalFrames;
        d->rateDrops = totalDrops;
        d->rateClock.restart();
    }
    return true;
}

bool ScreenCapture::finish(QString *error)
{
    for (Private::Slot &slot : d->captureSlots) {
        if (!slot.frame) continue;
        ext_image_copy_capture_frame_v1_destroy(slot.frame);
        slot.frame = nullptr;
        std::lock_guard<std::mutex> lock(d->ringMutex);
        d->ring.release(slot.index);
    }
    const bool encoded = d->stopEncoder(true, error);
    return encoded && d->firstUs > 0;
}

void ScreenCapture::abort()
{
    d->stopEncoder(false, nullptr);
}

qint64 ScreenCapture::firstFrameUs() const { return d->firstUs; }
qint64 ScreenCapture::lastFrameUs() const { return d->lastUs; }
QSize ScreenCapture::outputSize() const { return d->encodedSize; }
int ScreenCapture::encodedFrames() const { return d->frames.load(std::memory_order_relaxed); }
int ScreenCapture::droppedFrames() const
{
    std::lock_guard<std::mutex> lock(d->ringMutex);
    return d->ring.drops() + d->captureFailures;
}
double ScreenCapture::measuredFps() const { return d->lastRate; }
QString ScreenCapture::conversionMode() const { return d->conversion; }
