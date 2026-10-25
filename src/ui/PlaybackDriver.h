#pragma once

#include <QMediaPlayer>
#include <QObject>
#include <QPointer>
#include <functional>
#include <memory>

class QAudioOutput;
class QVideoSink;

namespace Omareel {

// Plays the editor's three files: the screen recording, the webcam recording, and one inserted
// video at a time. It knows nothing of clips or the timeline; it shows the positions it is told
// and reports what the players do. Behind its commands it works around QMediaPlayer: a player
// that is not about to play is run until its sink shows the frame asked for and paused there
// (warming up), a player stalled at the end of its file is revived before it seeks, and a seek
// into a file still opening waits for it.
class PlaybackDriver : public QObject
{
    Q_OBJECT
public:
    enum Player { Screen, Camera, Media };
    Q_ENUM(Player)

    explicit PlaybackDriver(QObject *parent = nullptr);
    ~PlaybackDriver() override;

    // Asked when a warm-up ends, since the transport may have moved on since it began.
    // playingQuery: whether the transport has the player running, so it keeps playing instead
    // of pausing on its frame; for the camera, whether the transport plays at all.
    // screenRestQuery: the screen position, in milliseconds, its warm-up comes to rest at.
    void setPlayingQuery(std::function<bool(Player)> query) { m_playingQuery = std::move(query); }
    void setScreenRestQuery(std::function<qint64()> query) { m_screenRestQuery = std::move(query); }

    // Opens the screen recording, and gives it and the media player their sound.
    void loadScreen(const QString &path);
    void loadCamera(const QString &path);
    // Opens a video in the media player. Seeks wait until it has opened, and its frame is not
    // ready until its sink shows one.
    void loadMedia(const QString &path);

    // Shows a player through a QML VideoOutput's videoSink; false when it has none.
    bool attachVideoOutput(Player player, QObject *output);
    // Runs the screen or camera player until its sink shows a frame, then pauses it there, the
    // screen exactly on the frame screenRestQuery names. A file that failed to open is left.
    void warmUp(Player player);
    // Lets a warming player run on: its next frame no longer pauses it.
    void cancelWarmUp(Player player);
    bool warming(Player player) const;

    // Shows the screen or media frame at milliseconds. A player that is not about to play
    // (willPlay) decodes that frame briefly and pauses on it.
    void seek(Player player, qint64 milliseconds, bool willPlay);
    void play(Player player);
    void pause(Player player);
    void setPlaybackRate(Player player, qreal rate);
    // Keeps the camera on the screen's rate and within tolerance of targetMs (on it when
    // force). It runs while the screen plays and covered says the camera recorded this moment,
    // and pauses otherwise.
    void syncCamera(qint64 targetMs, qint64 toleranceMs, bool force, bool covered);
    // The screen's sound is the recording's; the media player's is the inserted video's.
    void setAudio(Player player, double volume, bool muted);

    qint64 position(Player player) const;
    // False while the media player's file is still opening or a seek waits on it: it has no
    // position to report yet. The screen and camera always have one.
    bool positionReady(Player player) const;
    QMediaPlayer::PlaybackState playbackState(Player player) const;
    QMediaPlayer::MediaStatus mediaStatus(Player player) const;
    // The media sink shows a frame of the file now loaded.
    bool mediaFrameReady() const { return m_mediaFrameReady; }

signals:
    // A frame a sink shows; startTimeUs is its time in the file, -1 for an empty frame. live is
    // false while the driver runs or moves the player itself (a warm-up, or the seek that ends
    // one), when the frame says nothing about where playback is.
    void frameShown(PlaybackDriver::Player player, qint64 startTimeUs, bool live);
    // The media sink shows its first frame of the file now loaded.
    void mediaFrameReadyChanged();
    // A warm-up of the screen or media player began or ended.
    void warmingChanged();
    void mediaStatusChanged(PlaybackDriver::Player player, QMediaPlayer::MediaStatus status);
    void playbackStateChanged(PlaybackDriver::Player player, QMediaPlayer::PlaybackState state);

private:
    QMediaPlayer &player(Player player);
    const QMediaPlayer &player(Player player) const;
    bool playingNow(Player player) const { return m_playingQuery && m_playingQuery(player); }

    // Declared before the players so they outlive any frame a player delivers as it is torn down.
    std::function<bool(Player)> m_playingQuery;
    std::function<qint64()> m_screenRestQuery;
    QMediaPlayer m_player;
    QMediaPlayer m_cameraPlayer;
    // Holds one inserted video at a time.
    QMediaPlayer m_mediaPlayer;
    std::unique_ptr<QAudioOutput> m_audioOutput;
    std::unique_ptr<QAudioOutput> m_mediaAudioOutput;
    QPointer<QVideoSink> m_videoSink;
    QPointer<QVideoSink> m_cameraVideoSink;
    QPointer<QVideoSink> m_mediaVideoSink;
    qint64 m_pendingMediaSeekMs = -1;
    bool m_pendingMediaWillPlay = false;
    qint64 m_mediaWarmTargetMs = 0;
    bool m_warmingPreview = false;
    bool m_warmingCameraPreview = false;
    bool m_warmingMediaPreview = false;
    bool m_mediaFrameReady = false;
    // The driver moves a player itself; frames meanwhile are not live.
    bool m_internalSeek = false;
};

} // namespace Omareel
