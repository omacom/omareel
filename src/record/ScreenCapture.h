#pragma once

#include <QRect>
#include <QSize>
#include <QString>
#include <QVector>
#include <memory>

namespace Omareel {

struct ScreenCaptureConfig {
    QString monitor;
    QString outputPath;
    QString timestampPath;
    QRect crop;
    int fps = 60;
};

struct CaptureRowCopy {
    qsizetype sourceOffset = 0;
    qsizetype destinationOffset = 0;
    qsizetype bytes = 0;
};

QVector<CaptureRowCopy> captureCropRows(const QSize &sourceSize, int sourceStride,
                                        const QRect &requestedCrop);

class CaptureRingBookkeeping
{
public:
    enum class State { Available, Capturing, Queued, Writing };

    explicit CaptureRingBookkeeping(int slotCount = 4);
    int acquire();
    bool markQueued(int slot);
    int takeQueued();
    bool release(int slot);
    State state(int slot) const;
    int count(State state) const;
    int drops() const { return m_drops; }

private:
    QVector<State> m_states;
    QVector<int> m_queue;
    int m_drops = 0;
};

class ScreenCapture
{
public:
    struct Private;
    ScreenCapture();
    ~ScreenCapture();

    static bool isSupported();
    bool start(const ScreenCaptureConfig &config, QString *error = nullptr);
    bool captureFrame(QString *error = nullptr);
    bool finish(QString *error = nullptr);
    void abort();
    qint64 firstFrameUs() const;
    qint64 lastFrameUs() const;
    QSize outputSize() const;
    int encodedFrames() const;
    int droppedFrames() const;
    double measuredFps() const;
    QString conversionMode() const;

private:
    std::unique_ptr<Private> d;
};

} // namespace Omareel
