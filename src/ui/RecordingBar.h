#pragma once

#include <QObject>
#include <QPointF>
#include <QSize>
#include <QTimer>
#include <memory>

namespace Omareel {

class CameraCapture;

inline QPoint clampedSelfViewDragPosition(const QPointF &pressPointer,
                                          const QPoint &pressBubble,
                                          const QPointF &pointer,
                                          const QSize &screenSize,
                                          int bubblePixels)
{
    const QPoint delta(qRound(pointer.x() - pressPointer.x()),
                       qRound(pointer.y() - pressPointer.y()));
    return QPoint(qBound(0, pressBubble.x() + delta.x(),
                         qMax(0, screenSize.width() - bubblePixels)),
                  qBound(0, pressBubble.y() + delta.y(),
                         qMax(0, screenSize.height() - bubblePixels)));
}

class RecordingBar : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString elapsed READ elapsed NOTIFY elapsedChanged)
    Q_PROPERTY(QString recordedMonitor READ recordedMonitor CONSTANT)
    Q_PROPERTY(bool webcam READ webcam CONSTANT)
    Q_PROPERTY(bool hidden READ hidden CONSTANT)
    Q_PROPERTY(bool captureStarted READ captureStarted NOTIFY captureStartedChanged)
    Q_PROPERTY(int cameraRotation READ cameraRotation NOTIFY cameraSettingsChanged)
    Q_PROPERTY(bool cameraFlipHorizontal READ cameraFlipHorizontal NOTIFY cameraSettingsChanged)
    Q_PROPERTY(QString cameraError READ cameraError NOTIFY cameraErrorChanged)
    Q_PROPERTY(bool selfViewVisible READ selfViewVisible NOTIFY selfViewVisibilityChanged)
    Q_PROPERTY(bool selfViewSafe READ selfViewSafe CONSTANT)
    Q_PROPERTY(QString selfViewMonitor READ selfViewMonitor CONSTANT)
    Q_PROPERTY(int selfViewPixels READ selfViewPixels CONSTANT)
    Q_PROPERTY(int selfViewX READ selfViewX NOTIFY selfViewPlacementChanged)
    Q_PROPERTY(int selfViewY READ selfViewY NOTIFY selfViewPlacementChanged)
public:
    explicit RecordingBar(bool hidden = false, QObject *parent = nullptr);
    ~RecordingBar() override;

    QString elapsed() const { return m_elapsed; }
    QString recordedMonitor() const { return m_recordedMonitor; }
    bool webcam() const { return m_webcam; }
    bool hidden() const { return m_hidden; }
    bool captureStarted() const { return m_captureStarted; }
    int cameraRotation() const { return m_cameraRotation; }
    bool cameraFlipHorizontal() const { return m_cameraFlipHorizontal; }
    QString cameraError() const { return m_cameraError; }
    bool selfViewVisible() const { return m_selfViewVisible; }
    bool selfViewSafe() const { return m_selfViewSafe; }
    QString selfViewMonitor() const { return m_selfViewMonitor; }
    int selfViewPixels() const { return m_selfViewPixels; }
    int selfViewX() const { return m_selfViewX; }
    int selfViewY() const { return m_selfViewY; }

    Q_INVOKABLE void attachCameraOutput(QObject *output);
    Q_INVOKABLE void rotateCamera();
    Q_INVOKABLE void flipCamera();
    Q_INVOKABLE QPointF globalCursorPos() const;
    Q_INVOKABLE void beginDrag(const QPointF &globalPosition);
    Q_INVOKABLE void dragTo(const QPointF &globalPosition);
    Q_INVOKABLE void endDrag();
    Q_INVOKABLE void moveSelfView(int deltaX, int deltaY);
    Q_INVOKABLE void setSelfViewPosition(int x, int y);
    Q_INVOKABLE void setSelfViewVisible(bool visible);
    void setSelfViewScreenSize(const QSize &size);
    Q_INVOKABLE void stop();
    Q_INVOKABLE void cancel();

signals:
    void elapsedChanged();
    void cameraSettingsChanged();
    void cameraErrorChanged();
    void selfViewVisibilityChanged();
    void selfViewPlacementChanged();
    void captureStartedChanged();
    void finished();

private:
    void poll();
    void scheduleSelfViewPlacementUpdate();
    QString m_elapsed = QStringLiteral("00:00");
    QString m_recordedMonitor;
    bool m_webcam = false;
    bool m_hidden = false;
    bool m_captureStarted = false;
    int m_cameraRotation = 0;
    bool m_cameraFlipHorizontal = false;
    QString m_cameraError;
    bool m_selfViewVisible = false;
    bool m_selfViewSafe = false;
    QString m_selfViewMonitor;
    int m_selfViewPixels = 160;
    int m_selfViewX = 16;
    int m_selfViewY = 16;
    QSize m_selfViewScreenSize;
    QTimer m_saveSelfViewTimer;
    QTimer m_selfViewPlacementTimer;
    QPointF m_dragPressPointer;
    QPoint m_dragPressBubble;
    bool m_dragActive = false;
    qint64 m_commandSequence = 0;
    std::unique_ptr<CameraCapture> m_cameraCapture;
    bool m_cameraStopRequested = false;
    bool m_cameraRecordRequested = false;
    bool m_cameraFailed = false;
    QTimer m_timer;
};

} // namespace Omareel
