#include "EvdevListener.h"
#include "CursorSampler.h"

#include <QDir>
#include <QFile>
#include <algorithm>
#include <fcntl.h>
#include <libevdev/libevdev.h>
#include <poll.h>
#include <unistd.h>

using namespace Omareel;

struct EvdevListener::Device {
    int fd = -1;
    libevdev *evdev = nullptr;
    bool keyboard = false;
};

EvdevListener::~EvdevListener() { stop(); }

static bool isKeyboard(const libevdev *device)
{
    return libevdev_has_event_code(device, EV_KEY, KEY_A)
        || libevdev_has_event_code(device, EV_KEY, KEY_ENTER)
        || libevdev_has_event_code(device, EV_KEY, KEY_SPACE);
}

bool EvdevListener::start(QString *warning)
{
    QDir input(QStringLiteral("/dev/input"));
    for (const QString &name : input.entryList({QStringLiteral("event*")}, QDir::System | QDir::Files)) {
        const int fd = ::open(QFile::encodeName(input.filePath(name)).constData(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) continue;
        libevdev *evdev = nullptr;
        if (libevdev_new_from_fd(fd, &evdev) < 0) { ::close(fd); continue; }
        const bool useful = isKeyboard(evdev)
            || libevdev_has_event_code(evdev, EV_KEY, BTN_LEFT)
            || libevdev_has_event_code(evdev, EV_KEY, BTN_RIGHT)
            || libevdev_has_event_code(evdev, EV_KEY, BTN_MIDDLE)
            || libevdev_has_event_code(evdev, EV_REL, REL_WHEEL)
            || libevdev_has_event_code(evdev, EV_REL, REL_HWHEEL)
            || libevdev_has_event_code(evdev, EV_REL, REL_WHEEL_HI_RES)
            || libevdev_has_event_code(evdev, EV_REL, REL_HWHEEL_HI_RES);
        if (useful) m_devices << new Device{fd, evdev, isKeyboard(evdev)};
        else { libevdev_free(evdev); ::close(fd); }
    }
    if (m_devices.isEmpty()) {
        if (warning) *warning = QStringLiteral("No readable keyboard or pointer evdev devices; recording without clicks/keys");
        return false;
    }
    m_running = true;
    m_thread = std::thread(&EvdevListener::run, this);
    if (warning) warning->clear();
    return true;
}

void EvdevListener::stop()
{
    m_running = false;
    if (m_thread.joinable()) m_thread.join();
    for (Device *device : m_devices) {
        libevdev_free(device->evdev);
        ::close(device->fd);
        delete device;
    }
    m_devices.clear();
}

QVector<DeviceEvent> EvdevListener::events() const
{
    std::lock_guard<std::mutex> lock(m_mutex);
    return m_events;
}

void EvdevListener::append(const DeviceEvent &event)
{
    std::lock_guard<std::mutex> lock(m_mutex);
    m_events << event;
}

static QString buttonName(unsigned code)
{
    if (code == BTN_LEFT) return QStringLiteral("left");
    if (code == BTN_RIGHT) return QStringLiteral("right");
    if (code == BTN_MIDDLE) return QStringLiteral("middle");
    return {};
}

static QString modifierName(unsigned code)
{
    if (code == KEY_LEFTCTRL || code == KEY_RIGHTCTRL) return QStringLiteral("ctrl");
    if (code == KEY_LEFTSHIFT || code == KEY_RIGHTSHIFT) return QStringLiteral("shift");
    if (code == KEY_LEFTALT || code == KEY_RIGHTALT) return QStringLiteral("alt");
    if (code == KEY_LEFTMETA || code == KEY_RIGHTMETA) return QStringLiteral("meta");
    return {};
}

void EvdevListener::run()
{
    QVector<pollfd> descriptors;
    for (Device *device : m_devices) descriptors << pollfd{device->fd, POLLIN, 0};
    QStringList modifiers;
    while (m_running) {
        const int ready = ::poll(descriptors.data(), nfds_t(descriptors.size()), 100);
        if (ready <= 0) continue;
        for (int i = 0; i < descriptors.size(); ++i) {
            if (!(descriptors[i].revents & POLLIN)) continue;
            input_event event{};
            int status = 0;
            while ((status = libevdev_next_event(m_devices[i]->evdev,
                                                  LIBEVDEV_READ_FLAG_NORMAL, &event)) == 0) {
                DeviceEvent output;
                output.monotonicUs = CursorSampler::monotonicUs();
                if (event.type == EV_REL && (event.code == REL_WHEEL || event.code == REL_HWHEEL)) {
                    output.kind = DeviceEventKind::Scroll;
                    if (event.code == REL_WHEEL) output.dy = -event.value;
                    else output.dx = event.value;
                    append(output);
                    continue;
                }
                if (event.type != EV_KEY || event.value == 2) continue;
                output.button = buttonName(event.code);
                if (!output.button.isEmpty()) {
                    output.kind = event.value ? DeviceEventKind::ButtonDown : DeviceEventKind::ButtonUp;
                    append(output);
                    continue;
                }
                if (!m_devices[i]->keyboard || event.code >= BTN_MISC) continue;
                const QString modifier = modifierName(event.code);
                if (!modifier.isEmpty()) {
                    if (event.value && !modifiers.contains(modifier)) modifiers << modifier;
                    if (!event.value) modifiers.removeAll(modifier);
                }
                output.kind = event.value ? DeviceEventKind::KeyDown : DeviceEventKind::KeyUp;
                output.keyCode = int(event.code);
                const char *name = libevdev_event_code_get_name(EV_KEY, event.code);
                if (name) output.keyName = QString::fromLatin1(name);
                if (event.value) output.modifiers = modifiers;
                append(output);
            }
        }
    }
}
