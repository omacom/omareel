#pragma once

#include <QPointF>
#include <QVector>
#include <atomic>
#include <mutex>
#include <thread>

namespace Omareel {

struct RawCursorSample {
    qint64 monotonicUs = 0;
    QPointF logicalPosition;
};

class CursorSampler
{
public:
    CursorSampler() = default;
    ~CursorSampler();
    bool start(QString *error = nullptr);
    void stop();
    QVector<RawCursorSample> samples() const;
    static qint64 monotonicUs();

private:
    void run();
    QString m_socketPath;
    std::atomic_bool m_running{false};
    mutable std::mutex m_mutex;
    QVector<RawCursorSample> m_samples;
    std::thread m_thread;
};

} // namespace Omareel
