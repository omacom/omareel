#include "PlaybackDriver.h"

#include <QAudioOutput>
#include <QTimer>
#include <QUrl>
#include <QVariant>
#include <QVideoFrame>
#include <QVideoSink>
#include <cmath>
#include <utility>

using namespace Omareel;

PlaybackDriver::PlaybackDriver(QObject *parent): QObject(parent)
{
    connect(&m_player, &QMediaPlayer::mediaStatusChanged, this,
            [this](QMediaPlayer::MediaStatus status) { emit mediaStatusChanged(Screen, status); });
    connect(&m_cameraPlayer, &QMediaPlayer::mediaStatusChanged, this,
            [this](QMediaPlayer::MediaStatus status) { emit mediaStatusChanged(Camera, status); });
    connect(&m_mediaPlayer, &QMediaPlayer::mediaStatusChanged, this,
            [this](QMediaPlayer::MediaStatus status) {
        if ((status == QMediaPlayer::LoadedMedia || status == QMediaPlayer::BufferedMedia)
            && m_pendingMediaSeekMs >= 0) {
            const qint64 target = std::exchange(m_pendingMediaSeekMs, -1);
            seek(Media, target, m_pendingMediaWillPlay);
            if (playingNow(Media)) m_mediaPlayer.play();
        }
        emit mediaStatusChanged(Media, status);
    });
    connect(&m_player, &QMediaPlayer::playbackStateChanged, this,
            [this](QMediaPlayer::PlaybackState state) { emit playbackStateChanged(Screen, state); });
    connect(&m_cameraPlayer, &QMediaPlayer::playbackStateChanged, this,
            [this](QMediaPlayer::PlaybackState state) { emit playbackStateChanged(Camera, state); });
    connect(&m_mediaPlayer, &QMediaPlayer::playbackStateChanged, this,
            [this](QMediaPlayer::PlaybackState state) { emit playbackStateChanged(Media, state); });
}

PlaybackDriver::~PlaybackDriver() = default;

QMediaPlayer &PlaybackDriver::player(Player player)
{
    return player == Media ? m_mediaPlayer : player == Camera ? m_cameraPlayer : m_player;
}

const QMediaPlayer &PlaybackDriver::player(Player player) const
{
    return player == Media ? m_mediaPlayer : player == Camera ? m_cameraPlayer : m_player;
}

void PlaybackDriver::loadScreen(const QString &path)
{
    m_audioOutput = std::make_unique<QAudioOutput>();
    m_player.setAudioOutput(m_audioOutput.get());
    m_player.setSource(QUrl::fromLocalFile(path));
    m_mediaAudioOutput = std::make_unique<QAudioOutput>();
    m_mediaPlayer.setAudioOutput(m_mediaAudioOutput.get());
}

void PlaybackDriver::loadCamera(const QString &path)
{
    m_cameraPlayer.setSource(QUrl::fromLocalFile(path));
}

void PlaybackDriver::loadMedia(const QString &path)
{
    m_mediaFrameReady = false;
    m_pendingMediaSeekMs = -1;
    m_warmingMediaPreview = false;
    m_mediaPlayer.setSource(QUrl::fromLocalFile(path));
}

bool PlaybackDriver::attachVideoOutput(Player which, QObject *output)
{
    QVideoSink *sink = output ? output->property("videoSink").value<QVideoSink *>() : nullptr;
    if (which == Screen) {
        m_videoSink = sink;
        m_player.setVideoSink(m_videoSink);
        if (m_videoSink) connect(m_videoSink, &QVideoSink::videoFrameChanged, this,
            [this](const QVideoFrame &frame) {
                emit frameShown(Screen, frame.isValid() ? frame.startTime() : -1,
                                !m_warmingPreview && !m_internalSeek);
                if (m_warmingPreview) {
                    QTimer::singleShot(0, this, [this] {
                        // play() may have taken over since this frame arrived.
                        if (!m_warmingPreview) return;
                        m_warmingPreview = false;
                        m_player.pause();
                        m_cameraPlayer.pause();
                        // The decoder may have advanced a frame or two during warm-up; re-seek
                        // so the paused frame is the one the playhead points at.
                        m_internalSeek = true;
                        if (m_screenRestQuery) m_player.setPosition(m_screenRestQuery());
                        m_internalSeek = false;
                    });
                }
            });
        return sink != nullptr;
    }
    if (which == Camera) {
        m_cameraVideoSink = sink;
        m_cameraPlayer.setVideoSink(m_cameraVideoSink);
        if (m_cameraVideoSink) connect(m_cameraVideoSink, &QVideoSink::videoFrameChanged, this,
            [this](const QVideoFrame &frame) {
                emit frameShown(Camera, frame.isValid() ? frame.startTime() : -1,
                                !m_warmingCameraPreview && !m_internalSeek);
                if (!m_warmingCameraPreview) return;
                m_warmingCameraPreview = false;
                if (!playingNow(Camera)) m_cameraPlayer.pause();
            });
        return sink != nullptr;
    }
    m_mediaVideoSink = sink;
    m_mediaPlayer.setVideoSink(m_mediaVideoSink);
    if (!m_mediaVideoSink) return false;
    connect(m_mediaVideoSink, &QVideoSink::videoFrameChanged, this, [this](const QVideoFrame &frame) {
        if (!frame.isValid()) return;
        if (!m_mediaFrameReady) {
            m_mediaFrameReady = true;
            emit mediaFrameReadyChanged();
        }
        emit frameShown(Media, frame.startTime(), !m_warmingMediaPreview && !m_internalSeek);
        if (m_warmingMediaPreview) {
            QTimer::singleShot(0, this, [this] {
                if (!m_warmingMediaPreview) return;
                m_warmingMediaPreview = false;
                if (!playingNow(Media)) {
                    m_mediaPlayer.pause();
                    m_internalSeek = true;
                    m_mediaPlayer.setPosition(m_mediaWarmTargetMs);
                    m_internalSeek = false;
                }
                emit warmingChanged();
            });
        }
    });
    return true;
}

void PlaybackDriver::warmUp(Player which)
{
    // Only the screen and camera players are warmed this way; seek() warms an inserted video.
    Q_ASSERT(which != Media);
    if (which == Media) return;
    if (player(which).mediaStatus() == QMediaPlayer::InvalidMedia) return;
    if (which == Camera) m_warmingCameraPreview = true;
    else m_warmingPreview = true;
    player(which).play();
}

void PlaybackDriver::cancelWarmUp(Player which)
{
    if (which == Screen) m_warmingPreview = false;
    else if (which == Camera) m_warmingCameraPreview = false;
    else m_warmingMediaPreview = false;
}

bool PlaybackDriver::warming(Player which) const
{
    return which == Screen ? m_warmingPreview
        : which == Camera ? m_warmingCameraPreview : m_warmingMediaPreview;
}

void PlaybackDriver::seek(Player which, qint64 milliseconds, bool willPlay)
{
    // The camera follows the screen through syncCamera() instead.
    Q_ASSERT(which != Camera);
    if (which == Camera) return;
    QMediaPlayer &player = this->player(which);
    const bool insert = which == Media;
    // An inserted clip's file may still be opening; the seek waits for it.
    if (insert && (player.mediaStatus() == QMediaPlayer::NoMedia
                   || player.mediaStatus() == QMediaPlayer::LoadingMedia)) {
        m_pendingMediaSeekMs = milliseconds;
        m_pendingMediaWillPlay = willPlay;
        return;
    }
    // A player already paused on this exact frame (loaded ahead of its clip) keeps it; seeking
    // again would flush the frame it shows. Anything further off, a frame step included, seeks.
    if (player.playbackState() == QMediaPlayer::PausedState
        && player.mediaStatus() != QMediaPlayer::EndOfMedia
        && std::abs(player.position() - milliseconds) <= 1
        && (!insert || m_mediaFrameReady)) return;
    // After the player has run off the end it sits in EndOfMedia/Stopped, where a bare
    // setPosition() does not decode a frame, so scrubbing keeps showing the last (black)
    // frame. play() on a Stopped player restarts from 0 and drops any pending seek, so move to
    // Paused first (play+pause leaves the position alone), seek, then, unless it is about to
    // play anyway, let the decoder run until the sink reports one frame at the new position
    // and pause on that frame. (Calling play() and pause() back to back does not work: it
    // flushes the sink without decoding.)
    const bool stalled = player.mediaStatus() == QMediaPlayer::EndOfMedia
        || player.playbackState() == QMediaPlayer::StoppedState;
    if (!stalled) {
        player.setPosition(milliseconds);
        return;
    }
    player.play();
    player.pause();
    player.setPosition(milliseconds);
    if (willPlay) return;
    bool &warming = insert ? m_warmingMediaPreview : m_warmingPreview;
    if (insert) m_mediaWarmTargetMs = milliseconds;
    warming = true;
    emit warmingChanged();
    player.play();
    // Safety net in case the sink never reports a frame (no video output attached).
    QTimer::singleShot(250, this, [this, which, &player, &warming] {
        if (!warming) return;
        warming = false;
        if (!playingNow(which)) player.pause();
        emit warmingChanged();
    });
}

void PlaybackDriver::play(Player which) { player(which).play(); }
void PlaybackDriver::pause(Player which) { player(which).pause(); }
void PlaybackDriver::setPlaybackRate(Player which, qreal rate) { player(which).setPlaybackRate(rate); }

void PlaybackDriver::syncCamera(qint64 targetMs, qint64 toleranceMs, bool force, bool covered)
{
    m_cameraPlayer.setPlaybackRate(m_player.playbackRate());
    if (force || std::abs(m_cameraPlayer.position() - targetMs) > toleranceMs) {
        const bool cameraStalled = m_cameraPlayer.mediaStatus() == QMediaPlayer::EndOfMedia
            || m_cameraPlayer.playbackState() == QMediaPlayer::StoppedState;
        m_cameraPlayer.setPosition(targetMs);
        if (cameraStalled && m_player.playbackState() != QMediaPlayer::PlayingState) {
            m_cameraPlayer.play();
            QTimer::singleShot(250, this, [this] {
                if (m_player.playbackState() != QMediaPlayer::PlayingState) m_cameraPlayer.pause();
            });
        }
    }
    if (m_player.playbackState() == QMediaPlayer::PlayingState && covered) {
        if (m_cameraPlayer.playbackState() != QMediaPlayer::PlayingState) m_cameraPlayer.play();
    } else if (m_cameraPlayer.playbackState() != QMediaPlayer::PausedState) {
        m_cameraPlayer.pause();
    }
}

void PlaybackDriver::setAudio(Player which, double volume, bool muted)
{
    QAudioOutput *output = which == Screen ? m_audioOutput.get()
        : which == Media ? m_mediaAudioOutput.get() : nullptr;
    if (!output) return;
    output->setVolume(volume);
    output->setMuted(muted);
}

qint64 PlaybackDriver::position(Player which) const { return player(which).position(); }

bool PlaybackDriver::positionReady(Player which) const
{
    return which != Media || (m_pendingMediaSeekMs < 0
                              && m_mediaPlayer.mediaStatus() >= QMediaPlayer::LoadedMedia);
}

QMediaPlayer::PlaybackState PlaybackDriver::playbackState(Player which) const
{
    return player(which).playbackState();
}

QMediaPlayer::MediaStatus PlaybackDriver::mediaStatus(Player which) const
{
    return player(which).mediaStatus();
}
