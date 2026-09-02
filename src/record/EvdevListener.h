#pragma once

#include <QString>
#include <QStringList>
#include <QVector>
#include <atomic>
#include <mutex>
#include <thread>

namespace OmaRecord {

enum class DeviceEventKind { ButtonDown, ButtonUp, Scroll, KeyDown, KeyUp };

struct DeviceEvent {
    qint64 monotonicUs = 0;
    DeviceEventKind kind = DeviceEventKind::KeyDown;
    QString button;
    int dx = 0;
    int dy = 0;
    int keyCode = 0;
    QString keyName;
    QStringList modifiers;
};

class EvdevListener
{
public:
    EvdevListener() = default;
    ~EvdevListener();
    bool start(QString *warning = nullptr);
    void stop();
    QVector<DeviceEvent> events() const;

private:
    struct Device;
    void run();
    void append(const DeviceEvent &event);
    std::atomic_bool m_running{false};
    QVector<Device *> m_devices;
    mutable std::mutex m_mutex;
    QVector<DeviceEvent> m_events;
    std::thread m_thread;
};

} // namespace OmaRecord
