#pragma once

#include <QObject>
#include <QPointF>
#include <QSize>
#include <QTimer>
#include <QFileSystemWatcher>
#include <QLockFile>
#include <QJsonObject>
#include <QPointer>
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
    Q_PROPERTY(bool bubbleMapped READ bubbleMapped NOTIFY selfViewVisibilityChanged)
    Q_PROPERTY(QString elapsed READ elapsed NOTIFY elapsedChanged)
    Q_PROPERTY(QString recordedMonitor READ recordedMonitor NOTIFY captureStartedChanged)
    Q_PROPERTY(bool webcam READ webcam CONSTANT)
    Q_PROPERTY(bool hidden READ hidden NOTIFY captureStartedChanged)
    Q_PROPERTY(bool captureStarted READ captureStarted NOTIFY captureStartedChanged)
    Q_PROPERTY(bool countdownActive READ countdownActive NOTIFY countdownChanged)
    Q_PROPERTY(int countdownValue READ countdownValue NOTIFY countdownChanged)
    Q_PROPERTY(int cameraRotation READ cameraRotation NOTIFY cameraSettingsChanged)
    Q_PROPERTY(bool cameraFlipHorizontal READ cameraFlipHorizontal NOTIFY cameraSettingsChanged)
    Q_PROPERTY(QString cameraError READ cameraError NOTIFY cameraErrorChanged)
    Q_PROPERTY(bool selfViewVisible READ selfViewVisible NOTIFY selfViewVisibilityChanged)
    Q_PROPERTY(bool selfViewSafe READ selfViewSafe CONSTANT)
    Q_PROPERTY(QString selfViewMonitor READ selfViewMonitor CONSTANT)
    Q_PROPERTY(int selfViewPixels READ selfViewPixels NOTIFY selfViewPlacementChanged)
    Q_PROPERTY(int selfViewX READ selfViewX NOTIFY selfViewPlacementChanged)
    Q_PROPERTY(int selfViewY READ selfViewY NOTIFY selfViewPlacementChanged)
public:
    explicit RecordingBar(bool hidden = false, bool standby = false, qint64 owner = 0, QObject *parent = nullptr);
    ~RecordingBar() override;

    bool bubbleMapped() const { return m_selfViewVisible && !m_suppressed && !m_seedHidden && (m_standby || m_captureStarted); }
    QString elapsed() const { return m_elapsed; }
    QString recordedMonitor() const { return m_recordedMonitor; }
    bool webcam() const { return m_webcam; }
    bool hidden() const { return m_hidden; }
    bool captureStarted() const { return m_captureStarted; }
    bool countdownActive() const { return m_countdownActive; }
    int countdownValue() const { return m_countdownValue; }
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
    void countdownChanged();
    void finished();

private:
    void poll();
    void publishGeometry();
    void saveSelfViewPosition();
    void createCamera(const QJsonObject &state);
    QPointer<QObject> m_cameraOutput;
    void publishHost();
    void cameraState(const QJsonObject &values);
    void hostCommand(const QString &command);
    bool m_standby = false;
    bool m_adopted = false;
    bool m_suppressed = false;
    bool m_seedHidden = false;
    bool m_quit = false;
    qint64 m_owner = 0;
    qint64 m_hostSequence = 0;
    QTimer m_positionWriteTimer;
    QFileSystemWatcher m_watcher;
    std::unique_ptr<QLockFile> m_hostLock;
    void scheduleSelfViewPlacementUpdate();
    QString m_elapsed = QStringLiteral("00:00");
    QString m_recordedMonitor;
    bool m_webcam = false;
    bool m_hidden = false;
    bool m_captureStarted = false;
    bool m_countdownActive = false;
    int m_countdownValue = 3;
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
