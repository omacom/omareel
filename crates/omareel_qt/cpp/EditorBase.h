#pragma once

#include "PlaybackDriver.h"

#include <QMediaPlayer>
#include <QObject>
#include <QString>
#include <QTimer>
#include <QVariant>
#include <QVariantMap>
#include <cstdint>

class QUrl;

namespace omareel::exporter {
struct ExportOptions;
}

namespace Omareel {

class Exporter;

// The Qt side of the editor object, which is Rust (crates/omareel_qt/src/editor.rs): it owns the
// players, the timers and the exporter, and hands what they report to the Rust object through
// the pure virtuals below, which it overrides. Players are PlaybackDriver::Player values, player
// states and media statuses QMediaPlayer's. It is compiled into the omareel_qt archive beside
// Exporter, which it starts.
class EditorBase : public QObject
{
    Q_OBJECT
public:
    // The editor's timers: autosave (500 ms), motion (100 ms after the last change), scrub
    // (35 ms), preview (16 ms, precise, while playing) and preview stats (1 s).
    enum Timer { AutosaveTimer, MotionTimer, ScrubTimer, PreviewTimer, PreviewStatsTimer };
    // What the editor runs later from the event loop, as QTimer::singleShot did.
    enum Call { FinishLoadingCall, AdvanceClipCall };

    explicit EditorBase(QObject *parent = nullptr);
    ~EditorBase() override;

    Q_INVOKABLE void attachVideoOutput(QObject *output);
    Q_INVOKABLE void attachCameraVideoOutput(QObject *output);
    Q_INVOKABLE void attachMediaVideoOutput(QObject *output);
    Q_INVOKABLE void cancelExport();

    // Test hooks (no UI use).
    QMediaPlayer::PlaybackState playerPlaybackStateForTests() const { return m_playback.playbackState(PlaybackDriver::Screen); }
    QMediaPlayer::MediaStatus playerMediaStatusForTests() const { return m_playback.mediaStatus(PlaybackDriver::Screen); }
    qint64 playerPositionMsForTests() const { return m_playback.position(PlaybackDriver::Screen); }
    QMediaPlayer::PlaybackState cameraPlaybackStateForTests() const { return m_playback.playbackState(PlaybackDriver::Camera); }
    qint64 cameraPositionMsForTests() const { return m_playback.position(PlaybackDriver::Camera); }
    QMediaPlayer::PlaybackState mediaPlaybackStateForTests() const { return m_playback.playbackState(PlaybackDriver::Media); }

    // What the players, timers and exporter report, for the Rust object. 64-bit values are
    // std::int64_t, the type the Rust overrides have.
    virtual bool playerShouldPlay(int player) const = 0;
    virtual std::int64_t screenRestPosition() const = 0;
    virtual void handleFrameShown(int player, std::int64_t startTimeUs, bool live) = 0;
    virtual void handleMediaFrameReady() = 0;
    virtual void handleWarmingChanged() = 0;
    virtual void handleMediaStatus(int player, int status) = 0;
    virtual void handlePlaybackState(int player) = 0;
    virtual void cameraOutputAttached() = 0;
    virtual void mediaOutputAttached() = 0;
    virtual void timerFired(int timer) = 0;
    virtual void calledLater(int call) = 0;
    virtual void exportProgressed(int frame, int total) = 0;
    virtual void exportSucceeded(const QString &path) = 0;
    virtual void exportFailedWith(const QString &message) = 0;

protected:
    // PlaybackDriver, for the Rust object.
    void playbackLoadScreen(const QString &path) { m_playback.loadScreen(path); }
    void playbackLoadCamera(const QString &path) { m_playback.loadCamera(path); }
    void playbackLoadMedia(const QString &path) { m_playback.loadMedia(path); }
    void playbackWarmUp(int player) { m_playback.warmUp(PlaybackDriver::Player(player)); }
    void playbackCancelWarmUp(int player) { m_playback.cancelWarmUp(PlaybackDriver::Player(player)); }
    bool playbackWarming(int player) const { return m_playback.warming(PlaybackDriver::Player(player)); }
    void playbackSeek(int player, qint64 milliseconds, bool willPlay)
    { m_playback.seek(PlaybackDriver::Player(player), milliseconds, willPlay); }
    void playbackPlay(int player) { m_playback.play(PlaybackDriver::Player(player)); }
    void playbackPause(int player) { m_playback.pause(PlaybackDriver::Player(player)); }
    void playbackSetRate(int player, double rate) { m_playback.setPlaybackRate(PlaybackDriver::Player(player), rate); }
    void playbackSyncCamera(qint64 targetMs, qint64 toleranceMs, bool force, bool covered)
    { m_playback.syncCamera(targetMs, toleranceMs, force, covered); }
    void playbackSetAudio(int player, double volume, bool muted)
    { m_playback.setAudio(PlaybackDriver::Player(player), volume, muted); }
    qint64 playbackPosition(int player) const { return m_playback.position(PlaybackDriver::Player(player)); }
    bool playbackPositionReady(int player) const { return m_playback.positionReady(PlaybackDriver::Player(player)); }
    int playbackState(int player) const { return m_playback.playbackState(PlaybackDriver::Player(player)); }
    bool playbackMediaFrameReady() const { return m_playback.mediaFrameReady(); }

    void startEditorTimer(int timer) { this->timer(timer).start(); }
    void stopEditorTimer(int timer) { this->timer(timer).stop(); }
    bool editorTimerActive(int timer) const;
    // QTimer::singleShot(milliseconds, this, ...) of calledLater(call).
    void callLater(int call, int milliseconds);
    // Creates the exporter, connects it to the export virtuals and starts it from the event
    // loop.
    void startExport(const omareel::exporter::ExportOptions &options);

private:
    QTimer &timer(int timer);

    PlaybackDriver m_playback;
    QTimer m_previewTimer;
    QTimer m_scrubTimer;
    QTimer m_previewStatsTimer;
    QTimer m_autosaveTimer;
    QTimer m_motionTimer;
    Exporter *m_exporter = nullptr;
};

// Qt calls the editor makes, as free functions Rust can call.
namespace EditorShims {
// QJsonValue::fromVariant's JSON for a value from QML, wrapped in an array; with takeLocalFile,
// a value whose QUrl is a local file is that file's path first, as setProjectValue took
// background images.
QString projectValueJson(const QVariant &value, bool takeLocalFile);
// url.isLocalFile() ? url.toLocalFile() : url.toString()
QString urlPath(const QUrl &url);
// The same for a QUrl made from text, which is otherwise kept as it is.
QString textUrlPath(const QString &text);
// setZoomTarget's QVariant: true for "auto" (any case), otherwise the map's x and y (0.5 when
// missing).
bool zoomTarget(const QVariant &target, double &x, double &y);
// exportTo's settings, read with QVariant's conversions.
void exportSettings(const QVariantMap &settings, int &fps, int &height, QString &quality);
bool softwareRendering();
void copyToClipboard(const QString &text);
void openFolder(const QString &directory);
// OmarchyPaths::generateWallpaperThumbnail: a 400x240 JPEG of the wallpaper, unless one is there.
// Safe off the GUI thread.
bool generateWallpaperThumbnail(const QString &wallpaperPath, const QString &thumbnailPath);
} // namespace EditorShims

} // namespace Omareel
