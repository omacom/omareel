#include "CursorSampler.h"

#include <QByteArray>
#include <QDir>
#include <QJsonDocument>
#include <QJsonObject>
#include <chrono>
#include <cstring>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

using namespace Omareel;

CursorSampler::~CursorSampler() { stop(); }

qint64 CursorSampler::monotonicUs()
{
    timespec now{};
    clock_gettime(CLOCK_MONOTONIC, &now);
    return qint64(now.tv_sec) * 1000000 + now.tv_nsec / 1000;
}

bool CursorSampler::start(QString *error)
{
    const QString runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    const QString signature = qEnvironmentVariable("HYPRLAND_INSTANCE_SIGNATURE");
    m_socketPath = runtime + QStringLiteral("/hypr/") + signature + QStringLiteral("/.socket.sock");
    if (runtime.isEmpty() || signature.isEmpty() || !QFileInfo::exists(m_socketPath)) {
        if (error) *error = QStringLiteral("Hyprland IPC socket is unavailable: %1").arg(m_socketPath);
        return false;
    }
    m_running = true;
    m_thread = std::thread(&CursorSampler::run, this);
    if (error) error->clear();
    return true;
}

void CursorSampler::stop()
{
    m_running = false;
    if (m_thread.joinable()) m_thread.join();
}

QVector<RawCursorSample> CursorSampler::samples() const
{
    std::lock_guard<std::mutex> lock(m_mutex);
    return m_samples;
}

static bool queryCursor(const QByteArray &path, QPointF *point)
{
    const int fd = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return false;
    const timeval timeout{0, 20000};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    if (path.size() >= int(sizeof(address.sun_path))) { ::close(fd); return false; }
    std::memcpy(address.sun_path, path.constData(), size_t(path.size() + 1));
    if (::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) != 0) {
        ::close(fd);
        return false;
    }
    const QByteArray request("j/cursorpos");
    if (::write(fd, request.constData(), size_t(request.size())) != request.size()) {
        ::close(fd);
        return false;
    }
    QByteArray reply;
    char buffer[256];
    ssize_t count = 0;
    for (int attempt = 0; attempt < 16; ++attempt) {
        count = ::read(fd, buffer, sizeof(buffer));
        if (count <= 0) break;
        reply.append(buffer, count);
    }
    ::close(fd);
    const auto object = QJsonDocument::fromJson(reply).object();
    if (!object.contains("x") || !object.contains("y")) return false;
    *point = QPointF(object.value("x").toDouble(), object.value("y").toDouble());
    return true;
}

bool CursorSampler::cursorPosition(QPointF *point)
{
    return queryCursor(QFile::encodeName(qEnvironmentVariable("XDG_RUNTIME_DIR")
        + "/hypr/" + qEnvironmentVariable("HYPRLAND_INSTANCE_SIGNATURE") + "/.socket.sock"), point);
}

void CursorSampler::run()
{
    const QByteArray path = QFile::encodeName(m_socketPath);
    QPointF previous;
    bool havePrevious = false;
    qint64 lastStored = 0;
    auto deadline = std::chrono::steady_clock::now();
    while (m_running) {
        QPointF current;
        const qint64 timestamp = monotonicUs();
        if (queryCursor(path, &current)
            && (!havePrevious || current != previous || timestamp - lastStored >= 250000)) {
            std::lock_guard<std::mutex> lock(m_mutex);
            m_samples << RawCursorSample{timestamp, current};
            previous = current;
            havePrevious = true;
            lastStored = timestamp;
        }
        deadline += std::chrono::microseconds(1000000 / 240);
        std::this_thread::sleep_until(deadline);
    }
}
