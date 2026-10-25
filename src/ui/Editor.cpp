#include "Editor.h"

#include "core/ClipTimeline.h"
#include "core/CameraTimeline.h"
#include "core/ZoomTimeline.h"
#include "core/OmarchyPaths.h"
#include "core/Theme.h"
#include "render/Exporter.h"
#include "render/FrameSource.h"

#include <QAudioOutput>
#include <QClipboard>
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QProcess>
#include <QRegularExpression>
#include <QSaveFile>
#include <QQuickWindow>
#include <QPointer>
#include <QStandardPaths>
#include <QTextStream>
#include <QUrl>
#include <QVideoSink>
#include <QUuid>
#include <QtConcurrent>
#include <algorithm>
#include <cmath>
#include <fcntl.h>
#include <limits>
#include <linux/fs.h>
#include <sys/ioctl.h>
#include <unistd.h>
#include <utility>

using namespace Omareel;

namespace {

struct MediaInfo { int width = 0; int height = 0; double duration = 0.0; double fps = 60.0; bool audio = false; };

double fraction(const QString &text)
{
    const auto parts = text.split('/');
    return parts.size() == 2 && parts[1].toDouble() != 0.0
        ? parts[0].toDouble() / parts[1].toDouble() : text.toDouble();
}

MediaInfo probe(const QString &path, QString *error)
{
    QProcess process;
    process.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-show_entries"), QStringLiteral("stream=codec_type,width,height,r_frame_rate:format=duration"),
        QStringLiteral("-of"), QStringLiteral("json"), path});
    if (!process.waitForFinished(15000) || process.exitCode() != 0) {
        if (error) *error = QString::fromUtf8(process.readAllStandardError()).trimmed();
        return {};
    }
    const auto root = QJsonDocument::fromJson(process.readAllStandardOutput()).object();
    MediaInfo info;
    info.duration = root.value(QStringLiteral("format")).toObject().value(QStringLiteral("duration")).toString().toDouble();
    for (const auto &entry : root.value(QStringLiteral("streams")).toArray()) {
        const auto stream = entry.toObject();
        if (stream.value(QStringLiteral("codec_type")) == QLatin1String("video") && info.width == 0) {
            info.width = stream.value(QStringLiteral("width")).toInt();
            info.height = stream.value(QStringLiteral("height")).toInt();
            info.fps = fraction(stream.value(QStringLiteral("r_frame_rate")).toString());
        } else if (stream.value(QStringLiteral("codec_type")) == QLatin1String("audio")) info.audio = true;
    }
    if (info.width <= 0 || info.height <= 0 || info.duration <= 0.0) {
        if (error) *error = QStringLiteral("screen.mp4 has invalid metadata");
        return {};
    }
    if (error) error->clear();
    return info;
}

double aspectRatio(const Project &project, int width, int height)
{
    if (project.aspect == QLatin1String("16:9")) return 16.0 / 9.0;
    if (project.aspect == QLatin1String("1:1")) return 1.0;
    if (project.aspect == QLatin1String("4:3")) return 4.0 / 3.0;
    if (project.aspect == QLatin1String("9:16")) return 9.0 / 16.0;
    if (project.aspect == QLatin1String("3:4")) return 3.0 / 4.0;
    if (project.aspect == QLatin1String("4:5")) return 4.0 / 5.0;
    return width * project.crop.width() / std::max(1.0, height * project.crop.height());
}

QJsonObject setNested(QJsonObject object, const QStringList &parts, int index, const QJsonValue &value)
{
    if (index == parts.size() - 1) object.insert(parts[index], value);
    else object.insert(parts[index], setNested(object.value(parts[index]).toObject(), parts, index + 1, value));
    return object;
}

QString cleanPresetName(QString name)
{
    name = name.trimmed();
    name.replace(QRegularExpression(QStringLiteral("[^A-Za-z0-9 _.-]")), QStringLiteral("_"));
    while (name.startsWith('.')) name.remove(0, 1);
    return name.left(80);
}

QVariantList buildCameraThumbnails(const QString &videoPath, double duration,
                                   const QString &cacheDirectory)
{
    constexpr int count = 8;
    QDir directory(cacheDirectory);
    if (!directory.exists() && !directory.mkpath(QStringLiteral("."))) return {};

    const auto thumbnailPath = [&directory](int index) {
        return directory.filePath(QStringLiteral("camera-%1.jpg").arg(index + 1, 2, 10,
                                                                     QLatin1Char('0')));
    };
    bool complete = true;
    for (int index = 0; index < count; ++index)
        complete = complete && QFileInfo(thumbnailPath(index)).size() > 0;

    if (!complete) {
        for (int index = 0; index < count; ++index) {
            if (QFileInfo(thumbnailPath(index)).size() > 0) continue;
            const double timestamp = duration * (index + 0.5) / count;
            QProcess process;
            process.start(QStringLiteral("ffmpeg"), {
                QStringLiteral("-v"), QStringLiteral("error"), QStringLiteral("-ss"),
                QString::number(timestamp, 'f', 3), QStringLiteral("-i"), videoPath,
                QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-vf"),
                QStringLiteral("scale=112:72:force_original_aspect_ratio=increase,crop=112:72"),
                QStringLiteral("-q:v"), QStringLiteral("5"), QStringLiteral("-y"),
                thumbnailPath(index)});
            if (!process.waitForFinished(15000) || process.exitCode() != 0) return {};
        }
    }

    QVariantList result;
    result.reserve(count);
    for (int index = 0; index < count; ++index)
        result << QUrl::fromLocalFile(thumbnailPath(index)).toString();
    return result;
}

// The 16-character fingerprint keys the cache to this exact file, so a re-recorded bundle never
// shows stale frames; suffix separates differently filtered frames of the same file.
QString thumbnailCacheDirectory(const QString &videoPath, const QString &suffix)
{
    const QFileInfo videoSource(videoPath);
    const QByteArray fingerprint = QCryptographicHash::hash(
        (videoSource.canonicalFilePath() + QLatin1Char('|') + QString::number(videoSource.size())
         + QLatin1Char('|') + QString::number(videoSource.lastModified().toMSecsSinceEpoch())).toUtf8(),
        QCryptographicHash::Sha256).toHex().left(16);
    return QDir(QStandardPaths::writableLocation(QStandardPaths::CacheLocation))
        .filePath(QStringLiteral("clip-filmstrips-v2/") + QString::fromLatin1(fingerprint) + suffix);
}

QSet<int> cachedThumbnails(const QString &cacheDirectory)
{
    QSet<int> ready;
    for (const QFileInfo &file : QDir(cacheDirectory).entryInfoList(
             {QStringLiteral("??????????.jpg")}, QDir::Files)) {
        bool valid = false;
        const int sample = file.completeBaseName().toInt(&valid);
        if (valid && file.size() > 0) ready.insert(sample);
    }
    return ready;
}

bool buildVideoThumbnail(const QString &videoPath, const QString &filter, const QString &path,
                         int milliseconds)
{
    const QFileInfo target(path);
    if (!QDir().mkpath(target.absolutePath())) return false;
    const QString temporary = path + QLatin1Char('.')
        + QString::number(QCoreApplication::applicationPid()) + QStringLiteral(".tmp.jpg");
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {
        QStringLiteral("-v"), QStringLiteral("error"), QStringLiteral("-ss"),
        QString::number(milliseconds / 1000.0, 'f', 3), QStringLiteral("-i"), videoPath,
        QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-an"),
        QStringLiteral("-vf"), (filter.isEmpty() ? QString() : filter + QLatin1Char(','))
            + QStringLiteral("scale=160:90:force_original_aspect_ratio=increase,crop=160:90"),
        QStringLiteral("-q:v"), QStringLiteral("4"), QStringLiteral("-y"), temporary});
    if (!process.waitForFinished(20000)) {
        process.kill();
        process.waitForFinished();
        QFile::remove(temporary);
        return false;
    }
    if (process.exitCode() != 0 || QFileInfo(temporary).size() == 0) {
        QFile::remove(temporary);
        return false;
    }
    if (QFileInfo(path).size() > 0) QFile::remove(temporary);
    else if (!QFile::rename(temporary, path)) return false;
    const QFileInfoList cached = target.dir().entryInfoList({QStringLiteral("??????????.jpg")},
        QDir::Files, QDir::Time);
    for (qsizetype index = 512; index < cached.size(); ++index)
        QFile::remove(cached[index].absoluteFilePath());
    return true;
}

// A reflink shares the original's blocks until either file changes: instant, and free on btrfs
// and xfs. Elsewhere the file is copied.
bool cloneOrCopy(const QString &from, const QString &to, QString *error)
{
    const int source = ::open(QFile::encodeName(from).constData(), O_RDONLY | O_CLOEXEC);
    if (source >= 0) {
        const int target = ::open(QFile::encodeName(to).constData(),
                                  O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
        const bool cloned = target >= 0 && ::ioctl(target, FICLONE, source) == 0;
        if (target >= 0) ::close(target);
        ::close(source);
        if (cloned) return true;
    }
    QFile::remove(to);
    if (QFile::copy(from, to)) return true;
    if (error) *error = QStringLiteral("Could not copy %1 into the recording").arg(QFileInfo(from).fileName());
    return false;
}

// Runs off the GUI thread: probes a video and brings it into the bundle's media/ directory.
Editor::ImportedInsert importInsert(const QString &source, const QString &bundlePath)
{
    Editor::ImportedInsert result;
    result.info = probeVideo(source, &result.error);
    if (!result.info.isValid()) return result;
    const QDir media(QDir(bundlePath).filePath(QStringLiteral("media")));
    const QFileInfo file(source);
    // A video already in this bundle is used where it is.
    if (media.exists() && QFileInfo(file.canonicalPath()) == QFileInfo(media.canonicalPath())
        && !sanitizedMediaPath(QStringLiteral("media/") + file.fileName()).isEmpty()) {
        result.path = QStringLiteral("media/") + file.fileName();
        return result;
    }
    if (!media.exists() && !QDir().mkpath(media.path())) {
        result.error = QStringLiteral("Could not create %1").arg(media.path());
        return result;
    }
    QString base = file.completeBaseName();
    base.replace(QRegularExpression(QStringLiteral("[^A-Za-z0-9 _.-]")), QStringLiteral("_"));
    while (base.startsWith(QLatin1Char('.'))) base.remove(0, 1);
    if (base.isEmpty()) base = QStringLiteral("clip");
    const QString suffix = file.suffix().isEmpty() ? QString()
        : QLatin1Char('.') + file.suffix().toLower();
    QString name = base + suffix;
    for (int n = 2; media.exists(name); ++n) name = QStringLiteral("%1-%2%3").arg(base).arg(n).arg(suffix);
    const QString partial = media.filePath(QLatin1Char('.') + name + QStringLiteral(".part"));
    if (!cloneOrCopy(source, partial, &result.error)) return result;
    if (!QFile::rename(partial, media.filePath(name))) {
        QFile::remove(partial);
        result.error = QStringLiteral("Could not add %1 to the recording").arg(name);
        return result;
    }
    result.path = QStringLiteral("media/") + name;
    return result;
}

} // namespace

Editor::Editor(const QString &bundlePath, QObject *parent)
    : QObject(parent), m_bundlePath(QDir(bundlePath).absolutePath())
{
    m_projectPath = QDir(m_bundlePath).filePath(QStringLiteral("project.json"));
    m_videoPath = QDir(m_bundlePath).filePath(QStringLiteral("screen.mp4"));
    m_cameraVideoPath = QDir(m_bundlePath).filePath(QStringLiteral("camera.mp4"));
    m_autosaveTimer.setSingleShot(true);
    m_autosaveTimer.setInterval(500);
    m_motionTimer.setSingleShot(true);
    m_motionTimer.setInterval(100);
    m_scrubTimer.setSingleShot(true);
    m_scrubTimer.setInterval(35);
    connect(&m_autosaveTimer, &QTimer::timeout, this, &Editor::saveNow);
    connect(&m_motionTimer, &QTimer::timeout, this, &Editor::rebuildMotion);
    connect(&m_scrubTimer, &QTimer::timeout, this, &Editor::flushScrubSeek);
    connect(&m_motionWatcher, &QFutureWatcher<MotionTrack>::finished, this, &Editor::applyMotionResult);
    connect(&m_cameraThumbnailWatcher, &QFutureWatcher<QVariantList>::finished, this, [this] {
        m_cameraThumbnails = m_cameraThumbnailWatcher.result();
        emit cameraThumbnailsChanged();
    });
    connect(&m_videoThumbnailWatcher, &QFutureWatcher<bool>::finished, this, [this] {
        const auto [key, finished] = m_videoThumbnailQueue.takeFirst();
        if (ThumbnailSource *source = thumbnailSource(key)) {
            source->pending.remove(finished);
            if (!m_videoThumbnailWatcher.result()) source->failed.insert(finished);
            else {
                source->ready.insert(finished);
                // buildVideoThumbnail keeps the newest 512 files; resync once more are known.
                if (source->ready.size() > 512)
                    source->ready = cachedThumbnails(source->cacheDirectory);
            }
        }
        ++m_videoThumbnailRevision;
        emit videoThumbnailRevisionChanged();
        startNextVideoThumbnail();
    });

    QFile gradientsFile(QStringLiteral(":/omareel/assets/gradients.json"));
    if (gradientsFile.open(QIODevice::ReadOnly))
        m_gradients = QJsonDocument::fromJson(gradientsFile.readAll()).array().toVariantList();

    if (!loadBundle()) return;
    thumbnailSource(QStringLiteral("screen"));
    m_audioOutput = std::make_unique<QAudioOutput>();
    m_player.setAudioOutput(m_audioOutput.get());
    connect(&m_player, &QMediaPlayer::mediaStatusChanged, this,
            [this](QMediaPlayer::MediaStatus status) {
        if (status == QMediaPlayer::LoadedMedia || status == QMediaPlayer::BufferedMedia
            || status == QMediaPlayer::EndOfMedia || status == QMediaPlayer::InvalidMedia)
            QTimer::singleShot(0, this, &Editor::finishLoading);
        // The video can end a little before the clip's out point; the clip is over either way.
        if (status == QMediaPlayer::EndOfMedia && playing() && !clipIsMedia(m_activeClip))
            QTimer::singleShot(0, this, &Editor::advanceClip);
    });
    m_player.setSource(QUrl::fromLocalFile(m_videoPath));
    m_mediaAudioOutput = std::make_unique<QAudioOutput>();
    m_mediaPlayer.setAudioOutput(m_mediaAudioOutput.get());
    connect(&m_mediaPlayer, &QMediaPlayer::mediaStatusChanged, this,
            [this](QMediaPlayer::MediaStatus status) {
        if ((status == QMediaPlayer::LoadedMedia || status == QMediaPlayer::BufferedMedia)
            && m_pendingMediaSeekMs >= 0) {
            const qint64 target = std::exchange(m_pendingMediaSeekMs, -1);
            seekPlayer(m_mediaPlayer, target, m_pendingMediaWillPlay);
            if (playing() && clipIsMedia(m_activeClip)) m_mediaPlayer.play();
        }
        if (status == QMediaPlayer::EndOfMedia && playing() && clipIsMedia(m_activeClip))
            QTimer::singleShot(0, this, &Editor::advanceClip);
    });
    connect(&m_importWatcher, &QFutureWatcher<ImportedInsert>::finished, this, &Editor::finishImport);
    m_previewTimer.setTimerType(Qt::PreciseTimer);
    m_previewTimer.setInterval(16);
    connect(&m_previewTimer, &QTimer::timeout, this, [this] {
        // While an inserted clip's file is still opening, its player has no position yet.
        const bool insert = clipIsMedia(m_activeClip);
        if (!insert || (m_pendingMediaSeekMs < 0
                        && m_mediaPlayer.mediaStatus() >= QMediaPlayer::LoadedMedia))
            handlePlayerPosition(playerFor(m_activeClip).position());
        updatePlayheadPosition();
    });
    m_previewStatsEnabled = qEnvironmentVariableIntValue("OMAREEL_PREVIEW_STATS") == 1;
    if (m_previewStatsEnabled) QTextStream(stderr) << "preview stats enabled\n";
    m_previewStatsTimer.setInterval(1000);
    connect(&m_previewStatsTimer, &QTimer::timeout, this, &Editor::handlePreviewStats);
    if (m_previewStatsEnabled)
        connect(&m_player, &QMediaPlayer::playbackStateChanged, this, [this] {
            QTextStream(stderr) << "preview state=" << int(m_player.playbackState()) << '\n';
        });
    if (m_hasCamera) {
        m_cameraPlayer.setSource(QUrl::fromLocalFile(m_cameraVideoPath));
    }
    m_valid = true;
    connect(&m_screenFrameTimesWatcher, &QFutureWatcher<QList<double>>::finished, this,
            [this] { m_screenFrameTimes = m_screenFrameTimesWatcher.result(); });
    m_screenFrameTimesWatcher.setFuture(QtConcurrent::run(probeFrameTimes, m_videoPath));
    ++m_motionGeneration;
    rebuildMotion();
    startCameraThumbnailBuild();
    QTimer::singleShot(1500, this, &Editor::finishLoading);
}

Editor::~Editor()
{
    m_autosaveTimer.stop();
    if (m_dirty) saveNow();
    m_motionWatcher.waitForFinished();
    m_cameraThumbnailWatcher.waitForFinished();
    m_videoThumbnailWatcher.waitForFinished();
    m_importWatcher.waitForFinished();
    m_screenFrameTimesWatcher.waitForFinished();
    cancelExport();
}

bool Editor::loadBundle()
{
    if (!QFileInfo(m_bundlePath).isDir()) { m_error = QStringLiteral("Bundle does not exist: %1").arg(m_bundlePath); return false; }
    const MediaInfo info = probe(m_videoPath, &m_error);
    if (info.width <= 0) return false;
    m_sourceWidth = info.width;
    m_sourceHeight = info.height;
    m_sourceDuration = info.duration;
    m_fps = info.fps > 0.0 ? info.fps : 60.0;
    m_hasAudio = info.audio;
    QJsonObject captureRoot;
    QFile captureFile(QDir(m_bundlePath).filePath(QStringLiteral("capture.json")));
    if (captureFile.open(QIODevice::ReadOnly)) {
        captureRoot = QJsonDocument::fromJson(captureFile.readAll()).object();
        const double scale = captureRoot.value(QStringLiteral("scale")).toDouble(1.0);
        if (scale > 0.0) m_captureScale = scale;
        const QJsonObject audio = captureRoot.value(QStringLiteral("audio")).toObject();
        m_hasDesktopAudio = info.audio && audio.value(QStringLiteral("desktop")).toBool(false);
        m_hasMicrophoneAudio = info.audio && audio.value(QStringLiteral("mic")).toBool(false);
    }
    const QJsonObject cameraCapture = captureRoot.value(QStringLiteral("camera")).toObject();
    if (QFileInfo(m_cameraVideoPath).isFile()) {
        QString cameraError;
        const MediaInfo camera = probe(m_cameraVideoPath, &cameraError);
        if (camera.width > 0) {
            m_hasCamera = true;
            m_cameraSourceWidth = camera.width;
            m_cameraSourceHeight = camera.height;
            m_cameraFps = camera.fps;
            m_cameraDuration = camera.duration;
            const qint64 screenFirst = captureRoot.value(QStringLiteral("first_frame_us")).toVariant().toLongLong();
            const qint64 cameraFirst = cameraCapture.value(QStringLiteral("first_frame_us")).toVariant().toLongLong();
            if (screenFirst > 0 && cameraFirst > 0)
                m_cameraOffset = (cameraFirst - screenFirst) / 1000000.0;
        }
    }
    if (QFileInfo(m_projectPath).isFile()) {
        m_project = Project::load(m_projectPath, &m_error);
        if (!m_error.isEmpty()) return false;
    } else {
        QString name = QFileInfo(m_bundlePath).fileName();
        if (name.endsWith(QLatin1String(".omareel"))) name.chop(8);
        else if (name.endsWith(QLatin1String(".omarecord"))) name.chop(10);
        m_project = Project::defaults(name, m_sourceDuration);
        m_project.camera.enabled = m_hasCamera;
        const int rotation = cameraCapture.value(QStringLiteral("rotation")).toInt(0);
        if (rotation == 0 || rotation == 90 || rotation == 180 || rotation == 270)
            m_project.camera.rotation = rotation;
        m_project.camera.flipHorizontal = cameraCapture
            .value(QStringLiteral("flipHorizontal")).toBool(false);
        const QString shape = cameraCapture.value(QStringLiteral("shape")).toString(QStringLiteral("round"));
        if (shape == QLatin1String("round") || shape == QLatin1String("square")
                || shape == QLatin1String("rectangle"))
            m_project.camera.shape = shape;
        m_project.camera.radius = cameraCapture.value(QStringLiteral("radius")).toDouble(16.0);
    }
    if (std::none_of(m_project.clips.cbegin(), m_project.clips.cend(),
                     [](const Clip &clip) { return !clip.isMedia(); }))
        m_project.clips.prepend(Clip{QStringLiteral("c1"), 0.0, m_sourceDuration, 1.0});
    // A missing inserted video keeps its clip; it shows as missing and blocks export.
    for (const Clip &clip : std::as_const(m_project.clips))
        if (clip.isMedia() && !m_inserts.contains(clip.media))
            m_inserts.insert(clip.media, probeVideo(QDir(m_bundlePath).filePath(clip.media)));
    m_project.audio.desktop = m_project.audio.desktop && m_hasDesktopAudio;
    m_project.audio.mic = m_project.audio.mic && m_hasMicrophoneAudio;
    m_input = InputLog::loadBundle(m_bundlePath, m_sourceDuration, &m_error);
    if (!m_error.isEmpty()) return false;
    m_keystrokeTrack = KeystrokeTrack::build(m_input.events(), m_project.keystrokes);
    if (!QFileInfo(m_projectPath).isFile()) {
        QVector<double> clicks;
        for (const auto &click : m_input.clickDowns(true)) clicks << click.time;
        m_project.zooms = ZoomTimeline::generate(clicks, m_sourceDuration);
        m_dirty = true;
        m_autosaveTimer.start();
    }
    m_selectedClipId.clear();
    return true;
}

QVariantMap Editor::projectMap() const
{
    if (m_projectMapCacheValid) return m_projectMapCache;
    QVariantMap result = m_project.toJson().toVariantMap();
    Theme renderTheme;
    result[QStringLiteral("renderTheme")] = QVariantMap{
        {QStringLiteral("background"), renderTheme.background()},
        {QStringLiteral("foreground"), renderTheme.foreground()}};
    QVariantMap background = result.value(QStringLiteral("background")).toMap();
    QString image;
    if (m_project.background.type == QLatin1String("image")) image = m_project.background.image;
    else if (m_project.background.type == QLatin1String("wallpaper")) {
        image = m_project.background.wallpaper;
        if (image == QLatin1String("omarchy:current")) image = OmarchyPaths::currentBackground();
    }
    background[QStringLiteral("resolvedImage")] = image.isEmpty() ? QString() : QUrl::fromLocalFile(image).toString();
    result[QStringLiteral("background")] = background;
    // project.json leaves default layouts out; QML reads every clip's layout. kind, lower and
    // upper (how far its in and out may move), mediaName and missing are for the timeline only.
    QVariantList clips = result.value(QStringLiteral("clips")).toList();
    const ClipTimeline timeline(m_project.clips);
    for (int i = 0; i < clips.size() && i < m_project.clips.size(); ++i) {
        const Clip &source = m_project.clips[i];
        QVariantMap clip = clips[i].toMap();
        if (!clip.contains(QStringLiteral("layout")))
            clip.insert(QStringLiteral("layout"), QStringLiteral("default"));
        if (source.isMedia()) {
            const VideoInfo info = m_inserts.value(source.media);
            clip.insert(QStringLiteral("kind"), QStringLiteral("media"));
            clip.insert(QStringLiteral("lower"), 0.0);
            clip.insert(QStringLiteral("upper"), info.isValid() ? info.duration : source.out);
            clip.insert(QStringLiteral("mediaName"), QFileInfo(source.media).fileName());
            clip.insert(QStringLiteral("missing"), !info.isValid());
        } else {
            const auto bounds = timeline.recordingBounds(i, m_sourceDuration);
            clip.insert(QStringLiteral("kind"), QStringLiteral("recording"));
            clip.insert(QStringLiteral("lower"), bounds.first);
            clip.insert(QStringLiteral("upper"), bounds.second);
            clip.insert(QStringLiteral("missing"), false);
        }
        clips[i] = clip;
    }
    result[QStringLiteral("clips")] = clips;
    m_projectMapCache = result;
    m_projectMapCacheValid = true;
    return m_projectMapCache;
}

QVariantList Editor::clips() const
{
    return projectMap().value(QStringLiteral("clips")).toList();
}

QVariantList Editor::zooms() const
{
    return projectMap().value(QStringLiteral("zooms")).toList();
}

double Editor::duration() const { return ClipTimeline(m_project.clips).totalDuration(); }
bool Editor::playing() const { return m_transportPlaying; }

int Editor::outputWidth() const
{
    int width = qRound(720.0 * aspectRatio(m_project, m_sourceWidth, m_sourceHeight));
    return std::max(2, width + width % 2);
}

int Editor::outputHeight() const { return 720; }
bool Editor::softwareRendering() const
{
    return QQuickWindow::graphicsApi() == QSGRendererInterface::Software
        || QGuiApplication::platformName() == QLatin1String("offscreen");
}
double Editor::sourcePosition() const { return outputToSource(m_outputPosition); }

QVariantMap Editor::previewZoom() const
{
    const MotionSample s = m_motion ? m_motion->sample(sourcePosition()) : MotionSample{};
    return {{QStringLiteral("scale"), s.zoomScale}, {QStringLiteral("cx"), s.zoomCx},
        {QStringLiteral("cy"), s.zoomCy}, {QStringLiteral("velocity"), s.zoomCenterVelocity}};
}

LayoutSample Editor::layoutAt(double outputTime) const
{
    return sampleLayout(m_project.clips, outputTime, m_hasCamera, m_project.camera.enabled);
}

QVariantMap Editor::previewLayout() const
{
    const LayoutSample layout = layoutAt(m_outputPosition);
    return {{QStringLiteral("cameraFull"), layout.cameraFull},
            {QStringLiteral("bubble"), layout.bubble}};
}

QVariantMap Editor::previewMedia() const
{
    const int index = clipForOutput(m_outputPosition);
    if (!clipIsMedia(index))
        return {{QStringLiteral("active"), false}, {QStringLiteral("ready"), true},
                {QStringLiteral("fit"), QStringLiteral("fit")}, {QStringLiteral("width"), 16},
                {QStringLiteral("height"), 9}, {QStringLiteral("missing"), false}};
    const Clip &clip = m_project.clips[index];
    const VideoInfo info = m_inserts.value(clip.media);
    return {{QStringLiteral("active"), true},
            {QStringLiteral("ready"), m_mediaFrameReady && m_mediaPlayerSource == clip.media},
            {QStringLiteral("fit"), clip.fit}, {QStringLiteral("width"), std::max(1, info.width)},
            {QStringLiteral("height"), std::max(1, info.height)},
            {QStringLiteral("missing"), !info.isValid()}};
}

QVariantMap Editor::previewCursor() const
{
    const bool insert = clipIsMedia(clipForOutput(m_outputPosition));
    const MotionSample s = m_motion && !insert ? m_motion->sample(sourcePosition()) : MotionSample{};
    return {{QStringLiteral("x"), s.cursorX}, {QStringLiteral("y"), s.cursorY},
        {QStringLiteral("visible"), m_project.cursor.visible && !insert}, {QStringLiteral("scale"), s.cursorScale},
        {QStringLiteral("opacity"), s.cursorOpacity}, {QStringLiteral("rotation"), s.cursorRotation}};
}

QVariantList Editor::previewRipples() const
{
    QVariantList result;
    if (!m_motion || clipIsMedia(clipForOutput(m_outputPosition))
        || (m_project.cursor.clickEffect != QLatin1String("ripple")
            && m_project.cursor.clickEffect != QLatin1String("circle"))) return result;
    for (const auto &r : m_motion->ripples(sourcePosition()))
        result << QVariantMap{{QStringLiteral("x"), r.x}, {QStringLiteral("y"), r.y}, {QStringLiteral("progress"), r.progress}};
    return result;
}

QVariantList Editor::previewKeystrokePills() const
{
    QVariantList result;
    if (clipIsMedia(clipForOutput(m_outputPosition))) return result;
    for (const KeystrokePill &pill : m_keystrokeTrack.sample(sourcePosition()))
        result << QVariantMap{{QStringLiteral("keys"), pill.keys},
            {QStringLiteral("text"), pill.text}, {QStringLiteral("opacity"), pill.opacity}};
    return result;
}

bool Editor::cameraVisible() const
{
    if (!m_hasCamera || clipIsMedia(clipForOutput(m_outputPosition))) return false;
    const CameraTime mapped = mapCameraTime(sourcePosition(), m_cameraOffset, m_cameraDuration);
    // A full-frame camera holds its first frame until the camera started instead of leaving
    // the frame empty.
    return !mapped.beforeStart || layoutAt(m_outputPosition).cameraFull > 0.0;
}

void Editor::attachVideoOutput(QObject *output)
{
    m_videoSink = output ? output->property("videoSink").value<QVideoSink *>() : nullptr;
    m_player.setVideoSink(m_videoSink);
    if (m_videoSink) connect(m_videoSink, &QVideoSink::videoFrameChanged, this,
        [this](const QVideoFrame &frame) {
            QElapsedTimer cost;
            cost.start();
            ++m_previewStatsFrames;
            if (playing() && !m_warmingPreview && !m_scrubbing && !m_internalSeek
                && frame.isValid() && frame.startTime() >= 0
                && m_activeClip >= 0 && m_activeClip < m_project.clips.size()
                && !clipIsMedia(m_activeClip)) {
                const Clip &clip = m_project.clips[m_activeClip];
                const double source = frame.startTime() / 1000000.0;
                const double output = sourceToOutput(source, m_activeClip);
                // A seek can deliver queued frames from the previous location. Only a frame
                // near the current clip and transport position may anchor the display clock.
                if (source >= clip.in - 0.01 && source < clip.out
                    && output >= m_outputPosition - 0.1
                    && output <= m_outputPosition + 0.5) {
                    m_playheadClockPosition = std::max(m_playheadPosition, output);
                    m_playheadClock.restart();
                }
            }
            if (m_warmingPreview) {
                QTimer::singleShot(0, this, [this] {
                    // play() may have taken over since this frame arrived.
                    if (!m_warmingPreview) return;
                    m_warmingPreview = false;
                    m_player.pause();
                    m_cameraPlayer.pause();
                    // The decoder may have advanced a frame or two during warm-up; re-seek so
                    // the paused frame is the one the playhead points at.
                    m_internalSeek = true;
                    m_player.setPosition(qRound64(sourcePosition() * 1000.0));
                    m_internalSeek = false;
                });
            }
            m_previewStatsCostNs += cost.nsecsElapsed();
        });
    if (m_player.mediaStatus() != QMediaPlayer::InvalidMedia) {
        m_warmingPreview = true;
        m_player.play();
    }
}

void Editor::attachCameraVideoOutput(QObject *output)
{
    m_cameraVideoSink = output ? output->property("videoSink").value<QVideoSink *>() : nullptr;
    m_cameraPlayer.setVideoSink(m_cameraVideoSink);
    if (m_cameraVideoSink) connect(m_cameraVideoSink, &QVideoSink::videoFrameChanged, this,
        [this](const QVideoFrame &) {
            if (!m_warmingCameraPreview) return;
            m_warmingCameraPreview = false;
            if (!playing()) m_cameraPlayer.pause();
        });
    syncCamera(sourcePosition(), true);
    if (m_hasCamera && m_cameraPlayer.mediaStatus() != QMediaPlayer::InvalidMedia) {
        m_warmingCameraPreview = true;
        m_cameraPlayer.play();
    }
}

void Editor::attachMediaVideoOutput(QObject *output)
{
    m_mediaVideoSink = output ? output->property("videoSink").value<QVideoSink *>() : nullptr;
    m_mediaPlayer.setVideoSink(m_mediaVideoSink);
    if (!m_mediaVideoSink) return;
    connect(m_mediaVideoSink, &QVideoSink::videoFrameChanged, this, [this](const QVideoFrame &frame) {
        if (!frame.isValid()) return;
        if (!m_mediaFrameReady) {
            m_mediaFrameReady = true;
            updatePreview();
        }
        if (playing() && clipIsMedia(m_activeClip) && !m_warmingMediaPreview && !m_scrubbing
            && !m_internalSeek && frame.startTime() >= 0) {
            const Clip &clip = m_project.clips[m_activeClip];
            const double source = frame.startTime() / 1000000.0;
            const double output = sourceToOutput(source, m_activeClip);
            if (source >= clip.in - 0.01 && source < clip.out
                && output >= m_outputPosition - 0.1 && output <= m_outputPosition + 0.5) {
                m_playheadClockPosition = std::max(m_playheadPosition, output);
                m_playheadClock.restart();
            }
        }
        if (m_warmingMediaPreview) {
            QTimer::singleShot(0, this, [this] {
                if (!m_warmingMediaPreview) return;
                m_warmingMediaPreview = false;
                if (!(playing() && clipIsMedia(m_activeClip))) {
                    m_mediaPlayer.pause();
                    m_internalSeek = true;
                    m_mediaPlayer.setPosition(m_mediaWarmTargetMs);
                    m_internalSeek = false;
                }
                updateInsertAudio();
            });
        }
    });
    if (clipIsMedia(m_activeClip)) seekMedia(m_outputPosition);
}

bool Editor::clipIsMedia(int index) const
{
    return index >= 0 && index < m_project.clips.size() && m_project.clips[index].isMedia();
}

VideoInfo Editor::insertInfo(const QString &path)
{
    if (!m_inserts.contains(path))
        m_inserts.insert(path, probeVideo(QDir(m_bundlePath).filePath(path)));
    return m_inserts.value(path);
}

QMediaPlayer &Editor::playerFor(int clipIndex)
{
    return clipIsMedia(clipIndex) ? m_mediaPlayer : m_player;
}

void Editor::setTransportPlaying(bool value)
{
    if (m_transportPlaying == value) return;
    m_transportPlaying = value;
    if (value) {
        resetPlayheadClock(m_outputPosition);
        m_previewTimer.start();
        if (m_previewStatsEnabled) {
            m_previewStatsFrames = 0;
            m_previewStatsCostNs = 0;
            m_previewStatsClock.restart();
            m_previewStatsTimer.start();
        }
    } else {
        m_previewTimer.stop();
        m_previewStatsTimer.stop();
        m_playheadClock.invalidate();
        setPlayheadPosition(m_outputPosition);
    }
    updateInsertAudio();
    emit playingChanged();
    if (!value) startNextVideoThumbnail();
}

void Editor::seekPlayer(QMediaPlayer &player, qint64 milliseconds, bool willPlay)
{
    const bool insert = &player == &m_mediaPlayer;
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
    updateInsertAudio();
    player.play();
    // Safety net in case the sink never reports a frame (no video output attached).
    QTimer::singleShot(250, this, [this, &player, &warming] {
        if (!warming) return;
        warming = false;
        if (!(playing() && &playerFor(m_activeClip) == &player)) player.pause();
        updateInsertAudio();
    });
}

void Editor::loadInsert(const QString &path)
{
    if (m_mediaPlayerSource == path) return;
    m_mediaPlayerSource = path;
    m_mediaFrameReady = false;
    m_pendingMediaSeekMs = -1;
    m_warmingMediaPreview = false;
    m_mediaPlayer.setSource(QUrl::fromLocalFile(QDir(m_bundlePath).filePath(path)));
    updatePreview();
}

void Editor::prepareNextClip()
{
    const int next = m_activeClip + 1;
    if (m_activeClip < 0 || next >= m_project.clips.size()) return;
    const Clip &clip = m_project.clips[next];
    const qint64 target = qRound64(clip.in * 1000.0);
    if (!clipIsMedia(m_activeClip) && clip.isMedia()) {
        // The inserted clip after this one opens and waits on its first frame, so the cut to it
        // is immediate.
        if (m_mediaPlayerSource == clip.media && m_mediaFrameReady
            && m_mediaPlayer.playbackState() == QMediaPlayer::PausedState
            && std::abs(m_mediaPlayer.position() - target) <= 1) return;
        loadInsert(clip.media);
        m_mediaPlayer.setPlaybackRate(clip.speed);
        seekPlayer(m_mediaPlayer, target, false);
    } else if (clipIsMedia(m_activeClip) && !clip.isMedia()
               && m_player.playbackState() != QMediaPlayer::PlayingState) {
        // Likewise the screen waits where the recording resumes.
        m_player.setPlaybackRate(clip.speed);
        seekPlayer(m_player, target, false);
    }
}

void Editor::advanceClip()
{
    if (!playing()) return;
    if (m_activeClip + 1 < m_project.clips.size()) {
        seek(ClipTimeline(m_project.clips).outputTime(m_activeClip + 1,
                                                      m_project.clips[m_activeClip + 1].in));
    } else {
        pause();
        setOutputPosition(duration());
    }
}

void Editor::updateInsertAudio()
{
    if (!m_mediaAudioOutput) return;
    m_mediaAudioOutput->setVolume(std::clamp(m_project.audio.volume, 0.0, 1.0));
    // Silent while it decodes ahead of its clip or warms up on a paused frame.
    m_mediaAudioOutput->setMuted(m_warmingMediaPreview || !clipIsMedia(m_activeClip));
}

QString Editor::cameraStatus() const
{
    if (!m_hasCamera) return QStringLiteral("No webcam in this bundle");
    return QStringLiteral("Webcam: %1×%2 @%3, offset %4%5 s")
        .arg(m_cameraSourceWidth).arg(m_cameraSourceHeight)
        .arg(m_cameraFps, 0, 'f', qFuzzyCompare(m_cameraFps, qRound(m_cameraFps)) ? 0 : 2)
        .arg(m_cameraOffset >= 0.0 ? QStringLiteral("+") : QString())
        .arg(m_cameraOffset, 0, 'f', 2);
}

void Editor::snapshot(const QString &coalesceKey)
{
    if (!coalesceKey.isEmpty() && m_coalesceKey == coalesceKey && m_coalesceSnapshotTaken) return;
    m_undo << m_project.toJson();
    if (m_undo.size() > 100) m_undo.removeFirst();
    m_redo.clear();
    if (!coalesceKey.isEmpty()) { m_coalesceKey = coalesceKey; m_coalesceSnapshotTaken = true; }
    else { m_coalesceKey.clear(); m_coalesceSnapshotTaken = false; }
    emit historyChanged();
}

void Editor::beginCoalescedEdit(const QString &key)
{
    m_coalesceKey = key;
    m_coalesceSnapshotTaken = false;
}

void Editor::endCoalescedEdit()
{
    m_coalesceKey.clear();
    m_coalesceSnapshotTaken = false;
}

void Editor::changed(bool motion)
{
    m_projectMapCacheValid = false;
    m_keystrokeTrack = KeystrokeTrack::build(m_input.events(), m_project.keystrokes);
    if (m_audioOutput) {
        m_audioOutput->setVolume(std::clamp(m_project.audio.volume, 0.0, 1.0));
        m_audioOutput->setMuted(!m_project.audio.desktop && !m_project.audio.mic);
    }
    updateInsertAudio();
    if (!m_dirty) { m_dirty = true; emit dirtyChanged(); }
    m_autosaveTimer.start();
    emit projectChanged();
    emit durationChanged();
    emit outputSizeChanged();
    if (motion) { ++m_motionGeneration; m_motionTimer.start(); }
    updatePreview();
}

void Editor::setProjectValue(const QString &path, const QVariant &value, bool coalesce)
{
    const QStringList parts = path.split('.', Qt::SkipEmptyParts);
    if (parts.isEmpty()) return;
    QVariant normalized = value;
    if (path == QLatin1String("background.image") || path == QLatin1String("background.wallpaper")) {
        const QUrl url = value.toUrl();
        if (url.isLocalFile()) normalized = url.toLocalFile();
    }
    const QJsonObject old = m_project.toJson();
    const QJsonObject next = setNested(old, parts, 0, QJsonValue::fromVariant(normalized));
    const Project parsed = Project::fromJson(next);
    if (parsed.toJson() == old) return;
    snapshot(coalesce ? (m_coalesceKey.isEmpty() ? path : m_coalesceKey) : QString());
    m_project = parsed;
    changed();
}

void Editor::applyGradientPreset(int index)
{
    if (index < 0 || index >= m_gradients.size()) return;
    const QVariantList colours = m_gradients.at(index).toList();
    if (colours.size() < 2) return;

    QJsonArray stops;
    for (qsizetype i = 0; i < colours.size(); ++i)
        stops << QJsonArray{colours.at(i).toString(),
                            i / double(std::max<qsizetype>(1, colours.size() - 1))};

    const QJsonObject old = m_project.toJson();
    QJsonObject next = setNested(old, {QStringLiteral("background"), QStringLiteral("type")},
                                 0, QStringLiteral("gradient"));
    next = setNested(next, {QStringLiteral("background"), QStringLiteral("gradient"),
                            QStringLiteral("stops")}, 0, stops);
    const Project parsed = Project::fromJson(next);
    if (parsed.toJson() == old) return;
    snapshot();
    m_project = parsed;
    changed();
}

void Editor::restore(const QJsonObject &json)
{
    m_project = Project::fromJson(json);
    if (!m_selectedClipId.isEmpty() && clipIndex(m_selectedClipId) < 0)
        m_selectedClipId = m_project.clips.isEmpty() ? QString()
            : m_project.clips[clipForOutput(std::min(m_outputPosition, duration()))].id;
    QStringList retained;
    for (const QString &id : std::as_const(m_selectedZoomIds))
        if (zoomIndex(id) >= 0) retained << id;
    m_selectedZoomIds = retained;
    if (zoomIndex(m_selectedZoomId) < 0)
        m_selectedZoomId = retained.isEmpty() ? QString() : retained.first();
    changed();
    if (!m_project.clips.isEmpty()) seek(std::min(m_outputPosition, duration()));
    emit selectionChanged();
}

void Editor::undo()
{
    if (m_undo.isEmpty()) return;
    m_redo << m_project.toJson();
    const QJsonObject value = m_undo.takeLast();
    endCoalescedEdit();
    restore(value);
    emit historyChanged();
}

void Editor::redo()
{
    if (m_redo.isEmpty()) return;
    m_undo << m_project.toJson();
    const QJsonObject value = m_redo.takeLast();
    restore(value);
    emit historyChanged();
}

bool Editor::saveNow()
{
    if (!m_valid && !QFileInfo(m_videoPath).isFile()) return false;
    QString error;
    if (!m_project.save(m_projectPath, &error)) { emit errorOccurred(error); return false; }
    if (m_dirty) { m_dirty = false; emit dirtyChanged(); }
    emit autosaved(m_projectPath);
    return true;
}

int Editor::clipIndex(const QString &id) const
{
    for (int i = 0; i < m_project.clips.size(); ++i) if (m_project.clips[i].id == id) return i;
    return -1;
}

int Editor::zoomIndex(const QString &id) const
{
    for (int i = 0; i < m_project.zooms.size(); ++i) if (m_project.zooms[i].id == id) return i;
    return -1;
}

int Editor::clipForOutput(double output, double *clipOutputStart) const
{
    double start = 0.0;
    for (int i = 0; i < m_project.clips.size(); ++i) {
        const double length = (m_project.clips[i].out - m_project.clips[i].in) / m_project.clips[i].speed;
        if (output < start + length || i == m_project.clips.size() - 1) {
            if (clipOutputStart) *clipOutputStart = start;
            return i;
        }
        start += length;
    }
    return -1;
}

double Editor::outputToSource(double outputTime) const
{
    return ClipTimeline(m_project.clips).recordingSourceTime(std::clamp(outputTime, 0.0, duration()));
}

double Editor::sourceToOutput(double sourceTime, int preferredClip) const
{
    // preferredClip maps the clip's own time, which for an inserted clip is the video's.
    if (preferredClip >= 0 && preferredClip < m_project.clips.size())
        return ClipTimeline(m_project.clips).outputTime(preferredClip, sourceTime);
    for (int i = 0; i < m_project.clips.size(); ++i)
        if (!m_project.clips[i].isMedia()
            && sourceTime >= m_project.clips[i].in && sourceTime <= m_project.clips[i].out)
            return ClipTimeline(m_project.clips).outputTime(i, sourceTime);
    return -1.0;
}

QPointF Editor::sourceRangeToOutput(double sourceStart, double sourceEnd) const
{
    const auto range = ClipTimeline(m_project.clips).outputRange(sourceStart, sourceEnd);
    return {range.first, range.second};
}

void Editor::seek(double outputTime)
{
    if (m_project.clips.isEmpty()) return;
    m_scrubTimer.stop();
    m_pendingScrubPosition = -1.0;
    m_scrubbing = false;
    setOutputPosition(clampPlaybackPosition(outputTime));
    seekMedia(m_outputPosition);
    startNextVideoThumbnail();
}

void Editor::setPlaybackRange(double from, double to)
{
    if (!std::isfinite(from) || !std::isfinite(to)) return;
    if (from > to) std::swap(from, to);
    from = std::clamp(from, 0.0, duration());
    to = std::clamp(to, 0.0, duration());
    if (to - from <= 0.001) { clearPlaybackRange(); return; }
    if (m_playbackRangeStart == from && m_playbackRangeEnd == to) return;
    m_playbackRangeStart = from;
    m_playbackRangeEnd = to;
    emit playbackRangeChanged();
}

void Editor::clearPlaybackRange()
{
    if (!hasPlaybackRange()) return;
    m_playbackRangeStart = -1.0;
    m_playbackRangeEnd = -1.0;
    emit playbackRangeChanged();
}

QString Editor::videoThumbnailUrl(double sourceTime)
{
    return videoThumbnailUrlForClip(sourceTime, 0.0, m_sourceDuration);
}

QString Editor::videoThumbnailUrlForClip(double sourceTime, double sourceStart,
                                         double sourceEnd)
{
    return thumbnailUrl(QStringLiteral("screen"), sourceTime, sourceStart, sourceEnd);
}

QString Editor::clipThumbnailUrl(const QString &clipId, double sourceTime, double sourceStart,
                                 double sourceEnd)
{
    const int index = clipIndex(clipId);
    if (index >= 0 && m_project.clips[index].isMedia())
        return thumbnailUrl(QStringLiteral("media:") + m_project.clips[index].media, sourceTime,
                            sourceStart, sourceEnd);
    if (index < 0 || !m_hasCamera || m_project.clips[index].layout != QLatin1String("camera"))
        return videoThumbnailUrlForClip(sourceTime, sourceStart, sourceEnd);
    // The camera runs m_cameraOffset behind the screen. Before it started, the preview holds its
    // first frame, and so does the filmstrip.
    const auto cameraTime = [this](double screenTime) {
        return std::clamp(screenTime - m_cameraOffset, 0.0, m_cameraDuration);
    };
    double from = cameraTime(sourceStart);
    const double to = std::max(cameraTime(sourceEnd), std::min(m_cameraDuration, from + 0.1));
    from = std::min(from, std::max(0.0, to - 0.1));
    return thumbnailUrl(cameraThumbnailKey(), cameraTime(sourceTime), from, to);
}

QString Editor::cameraThumbnailKey() const
{
    return QStringLiteral("camera-r%1%2").arg(m_project.camera.rotation)
        .arg(m_project.camera.flipHorizontal ? QStringLiteral("f") : QString());
}

Editor::ThumbnailSource *Editor::thumbnailSource(const QString &key)
{
    const auto found = m_thumbnailSources.find(key);
    if (found != m_thumbnailSources.end()) return &found->second;
    ThumbnailSource source;
    if (key == QLatin1String("screen")) {
        source.videoPath = m_videoPath;
        source.duration = m_sourceDuration;
        source.fps = m_fps;
        source.cacheDirectory = thumbnailCacheDirectory(m_videoPath, {});
    } else if (m_hasCamera && key.startsWith(QLatin1String("camera-r"))) {
        // Thumbnails show the camera the way the composition turns it: flipped first, then
        // rotated clockwise, matching CameraOverlay's transform order.
        const bool flipped = key.endsWith(QLatin1Char('f'));
        const int rotation = key.mid(8, key.size() - 8 - (flipped ? 1 : 0)).toInt();
        QStringList filters;
        if (flipped) filters << QStringLiteral("hflip");
        if (rotation == 90) filters << QStringLiteral("transpose=clock");
        else if (rotation == 180) filters << QStringLiteral("hflip,vflip");
        else if (rotation == 270) filters << QStringLiteral("transpose=cclock");
        source.videoPath = m_cameraVideoPath;
        source.filter = filters.join(QLatin1Char(','));
        source.duration = m_cameraDuration;
        source.fps = m_cameraFps > 0.0 ? m_cameraFps : 30.0;
        source.cacheDirectory = thumbnailCacheDirectory(m_cameraVideoPath,
                                                        QLatin1Char('-') + key);
    } else if (key.startsWith(QLatin1String("media:"))) {
        const QString path = key.mid(6);
        const VideoInfo info = insertInfo(path);
        if (!info.isValid()) return nullptr;
        source.videoPath = QDir(m_bundlePath).filePath(path);
        source.duration = info.duration;
        source.fps = info.fps;
        source.cacheDirectory = thumbnailCacheDirectory(source.videoPath, {});
    } else {
        return nullptr;
    }
    source.ready = cachedThumbnails(source.cacheDirectory);
    return &m_thumbnailSources.emplace(key, std::move(source)).first->second;
}

QString Editor::thumbnailUrl(const QString &key, double time, double start, double end)
{
    if (!m_valid || !std::isfinite(time) || !std::isfinite(start) || !std::isfinite(end))
        return {};
    ThumbnailSource *source = thumbnailSource(key);
    if (!source) return {};
    start = std::clamp(start, 0.0, source->duration);
    end = std::clamp(end, start, source->duration);
    if (end - start <= 0.001) return {};
    const int firstSample = qCeil(start * 1000.0);
    const double tailMargin = std::min((end - start) / 2.0, 1.5 / std::max(1.0, source->fps));
    const int lastSample = std::max(firstSample, qFloor((end - tailMargin) * 1000.0));
    const int milliseconds = std::clamp(qRound(time * 10.0) * 100, firstSample, lastSample);
    const QDir cache(source->cacheDirectory);
    const QString path = cache.filePath(
        QStringLiteral("%1.jpg").arg(milliseconds, 10, 10, QLatin1Char('0')));
    if (QFileInfo(path).size() > 0) {
        source->ready.insert(milliseconds);
        return QUrl::fromLocalFile(path).toString();
    }
    if (!source->pending.contains(milliseconds) && !source->failed.contains(milliseconds)
        && m_videoThumbnailQueue.size() < 96) {
        source->pending.insert(milliseconds);
        m_videoThumbnailQueue << qMakePair(key, milliseconds);
        startNextVideoThumbnail();
    }
    int nearest = -1;
    for (const int ready : std::as_const(source->ready))
        if (ready >= firstSample && ready <= lastSample
            && (nearest < 0 || std::abs(ready - milliseconds) < std::abs(nearest - milliseconds)))
            nearest = ready;
    if (nearest < 0) return {};
    return QUrl::fromLocalFile(cache.filePath(
        QStringLiteral("%1.jpg").arg(nearest, 10, 10, QLatin1Char('0')))).toString();
}

void Editor::startNextVideoThumbnail()
{
    if (playing() || m_scrubbing || m_videoThumbnailWatcher.isRunning()
        || m_videoThumbnailQueue.isEmpty()) return;
    const auto [key, milliseconds] = m_videoThumbnailQueue.first();
    const ThumbnailSource *source = thumbnailSource(key);
    if (!source) {
        m_videoThumbnailQueue.removeFirst();
        startNextVideoThumbnail();
        return;
    }
    const QString path = QDir(source->cacheDirectory)
        .filePath(QStringLiteral("%1.jpg").arg(milliseconds, 10, 10, QLatin1Char('0')));
    m_videoThumbnailWatcher.setFuture(QtConcurrent::run(
        buildVideoThumbnail, source->videoPath, source->filter, path, milliseconds));
}

double Editor::clampPlaybackPosition(double outputTime) const
{
    outputTime = std::clamp(outputTime, 0.0, duration());
    if (!hasPlaybackRange()) return outputTime;
    return std::clamp(outputTime, m_playbackRangeStart,
                      std::max(m_playbackRangeStart, m_playbackRangeEnd - 0.001));
}

void Editor::beginScrub()
{
    if (m_project.clips.isEmpty()) return;
    pause();
    m_scrubbing = true;
}

void Editor::scrubTo(double outputTime)
{
    if (m_project.clips.isEmpty()) return;
    if (!m_scrubbing) beginScrub();
    setOutputPosition(clampPlaybackPosition(outputTime));
    m_pendingScrubPosition = m_outputPosition;
    if (!m_scrubTimer.isActive()) m_scrubTimer.start();
}

void Editor::endScrub()
{
    if (!m_scrubbing) return;
    m_scrubbing = false;
    m_scrubTimer.stop();
    flushScrubSeek();
    startNextVideoThumbnail();
}

void Editor::setOutputPosition(double outputTime)
{
    m_outputPosition = std::clamp(outputTime, 0.0, duration());
    m_activeClip = clipForOutput(m_outputPosition);
    resetPlayheadClock(m_outputPosition);
    emit positionChanged();
    updatePreview();
}

void Editor::setPlayheadPosition(double outputTime)
{
    outputTime = std::clamp(outputTime, 0.0, duration());
    if (std::abs(m_playheadPosition - outputTime) < 0.0005) return;
    m_playheadPosition = outputTime;
    emit playheadPositionChanged();
}

void Editor::resetPlayheadClock(double outputTime)
{
    m_playheadClockPosition = outputTime;
    m_playheadClock.restart();
    setPlayheadPosition(outputTime);
}

void Editor::updatePlayheadPosition()
{
    if (!playing() || m_scrubbing || m_project.clips.isEmpty()
        || !m_playheadClock.isValid()) return;
    const Clip &clip = m_project.clips[std::clamp(m_activeClip, 0, int(m_project.clips.size()) - 1)];
    const ClipTimeline timeline(m_project.clips);
    const double clipEnd = timeline.outputTime(m_activeClip, clip.out);
    const double limit = std::min(clipEnd, hasPlaybackRange()
        ? m_playbackRangeEnd - 0.001 : duration());
    // A decoded frame supplies the clock origin. Interpolate between frames, but stop
    // within two frame intervals if decoding stalls instead of racing ahead of the video.
    // Where the screen stood still the recording has no frames, and the clock runs on to the
    // next one.
    double maxLead = std::max(0.05, 2.0 / std::max(1.0, m_fps));
    if (!clip.isMedia()) {
        const double anchor = clip.in
            + (m_playheadClockPosition - timeline.outputTime(m_activeClip, clip.in)) * clip.speed;
        maxLead += std::max(0.0, nextScreenFrameTime(anchor) - anchor) / clip.speed;
    }
    const double elapsed = std::min(m_playheadClock.elapsed() / 1000.0, maxLead);
    const double target = std::max({m_playheadPosition, m_outputPosition,
                                    m_playheadClockPosition + elapsed});
    setPlayheadPosition(std::min(target, limit));
}

// The recording's next frame after sourceTime; its last frame lasts to the end. Until the frame
// times are known, every frame is taken to be the last before a change.
double Editor::nextScreenFrameTime(double sourceTime) const
{
    if (m_screenFrameTimes.isEmpty()) return sourceTime;
    const auto next = std::upper_bound(m_screenFrameTimes.cbegin(), m_screenFrameTimes.cend(),
                                       sourceTime + 0.0005);
    return next == m_screenFrameTimes.cend() ? std::max(sourceTime, m_sourceDuration) : *next;
}

void Editor::flushScrubSeek()
{
    if (m_pendingScrubPosition < 0.0) return;
    const double target = m_pendingScrubPosition;
    m_pendingScrubPosition = -1.0;
    seekMedia(target);
}

void Editor::seekMedia(double outputTime)
{
    double clipStart = 0.0;
    const int clipIndex = clipForOutput(outputTime, &clipStart);
    const auto &clip = m_project.clips[clipIndex];
    const double source = std::clamp(clip.in + (outputTime - clipStart) * clip.speed, clip.in, clip.out);
    const qint64 targetMs = qRound64(source * 1000.0);
    m_internalSeek = true;
    // While playing, the clip's player starts before the other stops, so the picture never
    // waits on a paused pair.
    if (clip.isMedia()) {
        loadInsert(clip.media);
        m_mediaPlayer.setPlaybackRate(clip.speed);
        seekPlayer(m_mediaPlayer, targetMs, playing());
        if (playing()) m_mediaPlayer.play();
        m_player.pause();
        m_cameraPlayer.pause();
    } else {
        m_player.setPlaybackRate(clip.speed);
        seekPlayer(m_player, targetMs, playing());
        if (playing()) m_player.play();
        m_mediaPlayer.pause();
        syncCamera(source, true);
    }
    updateInsertAudio();
    m_internalSeek = false;
    prepareNextClip();
}

void Editor::playPause() { playing() ? pause() : play(); }
void Editor::play()
{
    endScrub();
    if (hasPlaybackRange()) {
        if (m_outputPosition < m_playbackRangeStart
            || m_outputPosition >= m_playbackRangeEnd - 0.001)
            seek(m_playbackRangeStart);
    } else if (m_outputPosition >= duration() - 0.0001) seek(0);
    // The opening preview, or a seek from a stalled player, may still be warming up; its pause
    // on the first decoded frame must not stop playback asked for in the meantime.
    m_warmingPreview = false;
    m_warmingMediaPreview = false;
    m_activeClip = clipForOutput(m_outputPosition);
    setTransportPlaying(true);
    if (clipIsMedia(m_activeClip)) {
        m_mediaPlayer.play();
        m_player.pause();
    } else {
        m_player.play();
        m_mediaPlayer.pause();
        syncCamera(sourcePosition(), true);
    }
    updateInsertAudio();
    prepareNextClip();
}
void Editor::pause()
{
    setTransportPlaying(false);
    m_player.pause();
    m_cameraPlayer.pause();
    m_mediaPlayer.pause();
}

void Editor::stepFrames(int frames)
{
    pause();
    seek(m_outputPosition + frames / std::max(1.0, m_fps));
}

void Editor::seekBoundary(int direction)
{
    if (direction < 0) seek(0.0); else seek(duration());
}

void Editor::handlePlayerPosition(qint64 milliseconds)
{
    if (m_internalSeek || m_scrubbing || m_project.clips.isEmpty()) return;
    const Clip &clip = m_project.clips[std::clamp(m_activeClip, 0, int(m_project.clips.size()) - 1)];
    double source = milliseconds / 1000.0;
    // Through a still stretch of the recording the player holds its last frame's time while the
    // playhead runs on; the camera, the clip's end and the position follow the playhead.
    if (playing() && !clip.isMedia()) {
        const double playheadSource = clip.in + (m_playheadPosition
            - ClipTimeline(m_project.clips).outputTime(m_activeClip, clip.in)) * clip.speed;
        if (playheadSource >= clip.in - 0.001 && playheadSource <= clip.out + 0.001)
            source = std::max(source, std::min(playheadSource, clip.out));
    }
    const double mapped = sourceToOutput(source, m_activeClip);
    if (hasPlaybackRange() && playing() && mapped >= m_playbackRangeEnd - 0.001) {
        pause();
        seek(m_playbackRangeEnd);
        return;
    }
    if (!clip.isMedia()) syncCamera(source);
    if (playing() && source >= clip.out - 0.004) {
        advanceClip();
        return;
    }
    if (mapped >= 0.0) {
        const double output = clampPlaybackPosition(mapped);
        if (std::abs(m_outputPosition - output) >= 0.0005) {
            m_outputPosition = output;
            emit positionChanged();
            updatePreview();
        }
    }
}

void Editor::syncCamera(double screenSourceTime, bool force)
{
    if (!m_hasCamera) return;
    const CameraTime mapped = mapCameraTime(screenSourceTime, m_cameraOffset, m_cameraDuration);
    const qint64 target = qRound64(mapped.seconds * 1000.0);
    m_cameraPlayer.setPlaybackRate(m_player.playbackRate());
    // A bubble tolerates some drift; a full-frame talking head needs its lips on the audio.
    const qint64 tolerance = layoutAt(m_outputPosition).cameraFull > 0.0 ? 80 : 250;
    if (force || std::abs(m_cameraPlayer.position() - target) > tolerance) {
        const bool cameraStalled = m_cameraPlayer.mediaStatus() == QMediaPlayer::EndOfMedia
            || m_cameraPlayer.playbackState() == QMediaPlayer::StoppedState;
        m_cameraPlayer.setPosition(target);
        if (cameraStalled && m_player.playbackState() != QMediaPlayer::PlayingState) {
            m_cameraPlayer.play();
            QTimer::singleShot(250, this, [this] {
                if (m_player.playbackState() != QMediaPlayer::PlayingState) m_cameraPlayer.pause();
            });
        }
    }
    if (m_player.playbackState() == QMediaPlayer::PlayingState
        && !mapped.beforeStart && !mapped.beyondEnd) {
        if (m_cameraPlayer.playbackState() != QMediaPlayer::PlayingState) m_cameraPlayer.play();
    } else if (m_cameraPlayer.playbackState() != QMediaPlayer::PausedState) {
        m_cameraPlayer.pause();
    }
}

void Editor::updatePreview() { emit compositionChanged(); }

void Editor::handlePreviewStats()
{
    const qint64 elapsed = m_previewStatsClock.elapsed();
    if (elapsed <= 0) return;
    const double fps = m_previewStatsFrames * 1000.0 / elapsed;
    const double averageMs = m_previewStatsFrames > 0
        ? m_previewStatsCostNs / 1000000.0 / m_previewStatsFrames : 0.0;
    const int expected = qRound(m_fps * elapsed / 1000.0);
    const int drops = std::max(0, expected - m_previewStatsFrames);
    QTextStream(stderr) << QStringLiteral("preview fps=%1 average_frame_cost_ms=%2 drops=%3\n")
        .arg(fps, 0, 'f', 1).arg(averageMs, 0, 'f', 3).arg(drops);
    m_previewStatsFrames = 0;
    m_previewStatsCostNs = 0;
    m_previewStatsClock.restart();
}

bool Editor::splitAtPlayhead()
{
    double start = 0.0;
    const int index = clipForOutput(m_outputPosition, &start);
    if (index < 0) return false;
    const Clip clip = m_project.clips[index];
    const double source = clip.in + (m_outputPosition - start) * clip.speed;
    if (source - clip.in < 0.1 || clip.out - source < 0.1) return false;
    snapshot();
    Clip left = clip, right = clip;
    left.out = source;
    right.in = source;
    right.id = QStringLiteral("c-%1").arg(QUuid::createUuid().toString(QUuid::WithoutBraces));
    m_project.clips[index] = left;
    m_project.clips.insert(index + 1, right);
    m_selectedClipId = right.id;
    changed(false);
    emit selectionChanged();
    return true;
}

QPair<double, double> Editor::clipBounds(int index)
{
    const Clip &clip = m_project.clips[index];
    if (!clip.isMedia()) return ClipTimeline(m_project.clips).recordingBounds(index, m_sourceDuration);
    const VideoInfo info = insertInfo(clip.media);
    return {0.0, info.isValid() ? info.duration : clip.out};
}

bool Editor::moveClip(const QString &id, double sourceIn)
{
    const int index = clipIndex(id);
    if (index < 0) return false;
    const double length = m_project.clips[index].out - m_project.clips[index].in;
    const auto bounds = clipBounds(index);
    const double lower = bounds.first;
    const double upper = bounds.second - length;
    const double nextIn = std::clamp(sourceIn, lower, std::max(lower, upper));
    if (qFuzzyCompare(nextIn + 1.0, m_project.clips[index].in + 1.0)) return true;
    snapshot(m_coalesceKey.isEmpty() ? QStringLiteral("move-clip-") + id : m_coalesceKey);
    m_project.clips[index].in = nextIn;
    m_project.clips[index].out = nextIn + length;
    changed(false);
    return true;
}

bool Editor::trimClip(const QString &id, double newIn, double newOut)
{
    const int index = clipIndex(id);
    if (index < 0) return false;
    const auto [lower, upper] = clipBounds(index);
    newIn = std::clamp(newIn, lower, upper);
    newOut = std::clamp(newOut, lower, upper);
    if (newOut - newIn < 0.1) return false;
    snapshot(QStringLiteral("trim-") + id);
    m_project.clips[index].in = newIn;
    m_project.clips[index].out = newOut;
    changed(false);
    return true;
}

void Editor::traceInput(const QString &objectName, const QString &phase,
                        double x, double y) const
{
    if (qEnvironmentVariable("OMAREEL_INPUT_TRACE") != QLatin1String("1")) return;
    QTextStream(stderr) << QStringLiteral("OMAREEL_INPUT object=%1 phase=%2 local=(%3,%4)\n")
        .arg(objectName, phase).arg(x, 0, 'f', 1).arg(y, 0, 'f', 1);
}

bool Editor::removeClip(const QString &id)
{
    const int index = clipIndex(id);
    if (index < 0 || m_project.clips.size() <= 1) return false;
    // The recording always keeps at least one clip.
    if (!m_project.clips[index].isMedia()
        && std::count_if(m_project.clips.cbegin(), m_project.clips.cend(),
                         [](const Clip &clip) { return !clip.isMedia(); }) <= 1) return false;
    snapshot();
    m_project.clips.removeAt(index);
    m_selectedClipId = m_project.clips[std::min(index, int(m_project.clips.size()) - 1)].id;
    seek(std::min(m_outputPosition, duration()));
    changed(false);
    emit selectionChanged();
    return true;
}

bool Editor::deleteOutputRange(double from, double to)
{
    if (!std::isfinite(from) || !std::isfinite(to)) return false;
    if (from > to) std::swap(from, to);
    from = std::clamp(from, 0.0, duration());
    to = std::clamp(to, 0.0, duration());
    ClipTimeline edited(m_project.clips);
    const QString remainderId = QStringLiteral("c-%1").arg(
        QUuid::createUuid().toString(QUuid::WithoutBraces));
    if (!edited.deleteRange(from, to, remainderId)) return false;
    if (std::none_of(edited.clips().cbegin(), edited.clips().cend(),
                     [](const Clip &clip) { return !clip.isMedia(); })) return false;

    pause();
    snapshot();
    const auto visibleSourceSpan = [](const ZoomSegment &zoom, const QVector<Clip> &clips) {
        double first = std::numeric_limits<double>::infinity();
        double last = -std::numeric_limits<double>::infinity();
        for (const Clip &clip : clips) {
            if (clip.isMedia()) continue;
            const double start = std::max(zoom.start, clip.in);
            const double end = std::min(zoom.end, clip.out);
            if (end - start <= 1e-9) continue;
            first = std::min(first, start);
            last = std::max(last, end);
        }
        return std::pair{first, last};
    };
    QVector<ZoomSegment> survivingZooms;
    bool zoomsChanged = false;
    for (ZoomSegment zoom : std::as_const(m_project.zooms)) {
        const auto before = visibleSourceSpan(zoom, m_project.clips);
        const auto after = visibleSourceSpan(zoom, edited.clips());
        if (std::isfinite(before.first) && !std::isfinite(after.first)) {
            zoomsChanged = true;
            continue;
        }
        if (std::isfinite(before.first) && before != after) {
            zoom.start = after.first;
            zoom.end = after.second;
            zoomsChanged = true;
        }
        survivingZooms << zoom;
    }
    m_project.clips = edited.clips();
    if (zoomsChanged) m_project.zooms = std::move(survivingZooms);
    m_selectedClipId = m_project.clips[clipForOutput(std::min(from, duration()))].id;
    m_selectedZoomId.clear();
    m_selectedZoomIds.clear();
    changed(zoomsChanged);
    seek(std::min(from, duration()));
    emit selectionChanged();
    return true;
}

bool Editor::mergeClip(const QString &id, int direction)
{
    const int index = clipIndex(id);
    const int other = index + (direction < 0 ? -1 : 1);
    if (index < 0 || other < 0 || other >= m_project.clips.size()) return false;
    // Only pieces of the recording join; an inserted clip has nothing to join across.
    if (m_project.clips[index].isMedia() || m_project.clips[other].isMedia()) return false;
    snapshot();
    const int first = std::min(index, other), second = std::max(index, other);
    m_project.clips[first].out = m_project.clips[second].out;
    m_project.clips.removeAt(second);
    m_selectedClipId = m_project.clips[first].id;
    changed(false);
    emit selectionChanged();
    return true;
}

bool Editor::setClipSpeed(const QString &id, double speed)
{
    const int index = clipIndex(id);
    if (index < 0 || !allowedClipSpeeds().contains(speed)) return false;
    snapshot();
    m_project.clips[index].speed = speed;
    changed(false);
    seek(std::min(m_outputPosition, duration()));
    return true;
}

bool Editor::setClipLayout(const QString &id, const QString &layout)
{
    const int index = clipIndex(id);
    if (index < 0 || !allowedClipLayouts().contains(layout) || m_project.clips[index].isMedia())
        return false;
    if (m_project.clips[index].layout == layout) return true;
    snapshot();
    m_project.clips[index].layout = layout;
    changed(false);
    // The camera now has to keep up with a full-frame layout, or may fall back to a bubble.
    syncCamera(sourcePosition(), true);
    return true;
}

bool Editor::setClipFit(const QString &id, const QString &fit)
{
    const int index = clipIndex(id);
    if (index < 0 || !m_project.clips[index].isMedia() || !allowedClipFits().contains(fit))
        return false;
    if (m_project.clips[index].fit == fit) return true;
    snapshot();
    m_project.clips[index].fit = fit;
    changed(false);
    return true;
}

bool Editor::reorderClip(const QString &id, int direction)
{
    const int index = clipIndex(id);
    const int other = index + (direction < 0 ? -1 : 1);
    // Recording clips keep their order; only an inserted clip moves past its neighbour.
    if (index < 0 || other < 0 || other >= m_project.clips.size()
        || !m_project.clips[index].isMedia()) return false;
    pause();
    snapshot();
    m_project.clips.swapItemsAt(index, other);
    m_selectedClipId = id;
    changed(false);
    seek(ClipTimeline(m_project.clips).outputTime(other, m_project.clips[other].in));
    emit selectionChanged();
    return true;
}

void Editor::insertMedia(const QUrl &url)
{
    if (!m_valid || m_importing) return;
    const QString source = url.isLocalFile() ? url.toLocalFile() : url.toString();
    if (source.isEmpty()) return;
    pause();
    m_importing = true;
    m_importError.clear();
    emit importingChanged();
    m_importWatcher.setFuture(QtConcurrent::run(importInsert, source, m_bundlePath));
}

void Editor::clearImportError()
{
    if (m_importError.isEmpty()) return;
    m_importError.clear();
    emit importingChanged();
}

void Editor::finishImport()
{
    const ImportedInsert imported = m_importWatcher.result();
    m_importing = false;
    if (!imported.error.isEmpty()) {
        m_importError = imported.error;
        emit importingChanged();
        return;
    }
    m_inserts.insert(imported.path, imported.info);
    // The new clip goes at the playhead: before a clip it starts, after one it ends, or between
    // the two halves of the clip it splits.
    const double at = std::clamp(m_outputPosition, 0.0, duration());
    double clipStart = 0.0;
    const int index = clipForOutput(at, &clipStart);
    snapshot();
    int insertAt = m_project.clips.size();
    if (index >= 0) {
        const Clip clip = m_project.clips[index];
        const double source = clip.in + (at - clipStart) * clip.speed;
        if (source - clip.in < 0.1) {
            insertAt = index;
        } else {
            insertAt = index + 1;
            if (clip.out - source >= 0.1) {
                Clip left = clip, right = clip;
                left.out = source;
                right.in = source;
                right.id = QStringLiteral("c-%1").arg(QUuid::createUuid().toString(QUuid::WithoutBraces));
                m_project.clips[index] = left;
                m_project.clips.insert(index + 1, right);
            }
        }
    }
    Clip insert{QStringLiteral("m-%1").arg(QUuid::createUuid().toString(QUuid::WithoutBraces)),
                0.0, imported.info.duration, 1.0};
    insert.media = imported.path;
    m_project.clips.insert(insertAt, insert);
    m_selectedClipId = insert.id;
    m_selectedZoomId.clear();
    m_selectedZoomIds.clear();
    changed(false);
    seek(ClipTimeline(m_project.clips).outputTime(insertAt, 0.0));
    emit selectionChanged();
    emit importingChanged();
}

bool Editor::resetClipTrims(const QString &id)
{
    const int index = clipIndex(id);
    if (index < 0) return false;
    const auto [newIn, newOut] = clipBounds(index);
    if (newOut - newIn < 0.1) return false;
    snapshot();
    m_project.clips[index].in = newIn;
    m_project.clips[index].out = newOut;
    changed(false);
    return true;
}

QString Editor::addZoomAt(double outputTime, double length)
{
    // Zooms never overlap. A new one starts where asked inside the open stretch around that
    // point, ends early at the next zoom, and backs up when less than a second remains.
    // Inserted clips have no recording to zoom into.
    if (clipIsMedia(clipForOutput(std::clamp(outputTime, 0.0, duration())))) return {};
    const double at = std::clamp(outputToSource(outputTime), 0.0, m_sourceDuration);
    double lower = 0.0;
    double upper = m_sourceDuration;
    for (const ZoomSegment &zoom : std::as_const(m_project.zooms)) {
        if (zoom.start <= at && at < zoom.end) return {};
        if (zoom.end <= at) lower = std::max(lower, zoom.end);
        else upper = std::min(upper, zoom.start);
    }
    const double end = std::min(upper, at + std::max(1.0, length));
    const double start = end - at < 1.0 ? std::max(lower, end - 1.0) : at;
    if (end - start < 1.0) return {};
    snapshot();
    ZoomSegment zoom{QStringLiteral("z-%1").arg(QUuid::createUuid().toString(QUuid::WithoutBraces)), start, end, 2.0, true, {0.5, 0.5}};
    m_project.zooms << zoom;
    std::sort(m_project.zooms.begin(), m_project.zooms.end(), [](const auto &a, const auto &b) { return a.start < b.start; });
    m_selectedZoomId = zoom.id;
    m_selectedZoomIds = {zoom.id};
    changed();
    emit selectionChanged();
    return zoom.id;
}

bool Editor::moveZoom(const QString &id, double sourceStart)
{
    const int index = zoomIndex(id);
    if (index < 0) return false;
    const double length = m_project.zooms[index].end - m_project.zooms[index].start;
    const double lower = index > 0 ? m_project.zooms[index - 1].end : 0.0;
    const double upper = index + 1 < m_project.zooms.size()
        ? m_project.zooms[index + 1].start - length : m_sourceDuration - length;
    sourceStart = std::clamp(sourceStart, lower, std::max(lower, upper));
    snapshot(QStringLiteral("move-") + id);
    m_project.zooms[index].start = sourceStart;
    m_project.zooms[index].end = sourceStart + length;
    changed();
    return true;
}

bool Editor::resizeZoom(const QString &id, double sourceStart, double sourceEnd)
{
    const int index = zoomIndex(id);
    if (index < 0) return false;
    const double lower = index > 0 ? m_project.zooms[index - 1].end : 0.0;
    const double upper = index + 1 < m_project.zooms.size()
        ? m_project.zooms[index + 1].start : m_sourceDuration;
    sourceStart = std::clamp(sourceStart, lower, upper);
    sourceEnd = std::clamp(sourceEnd, lower, upper);
    if (sourceEnd - sourceStart < 1.0) return false;
    snapshot(QStringLiteral("resize-") + id);
    m_project.zooms[index].start = sourceStart;
    m_project.zooms[index].end = sourceEnd;
    changed();
    return true;
}

bool Editor::setZoomLevel(const QString &id, double level, bool coalesce)
{
    const int index = zoomIndex(id);
    if (index < 0) return false;
    snapshot(coalesce ? QStringLiteral("level-") + id : QString());
    m_project.zooms[index].level = std::clamp(level, 1.0, 4.0);
    changed();
    return true;
}

bool Editor::setZoomTarget(const QString &id, const QVariant &target)
{
    const int index = zoomIndex(id);
    if (index < 0) return false;
    snapshot();
    if (target.toString().compare(QStringLiteral("auto"), Qt::CaseInsensitive) == 0) {
        m_project.zooms[index].automaticTarget = true;
    } else {
        const QVariantMap point = target.toMap();
        m_project.zooms[index].automaticTarget = false;
        m_project.zooms[index].target = QPointF(std::clamp(point.value(QStringLiteral("x"), 0.5).toDouble(), 0.0, 1.0),
                                                std::clamp(point.value(QStringLiteral("y"), 0.5).toDouble(), 0.0, 1.0));
    }
    changed();
    return true;
}

bool Editor::setZoomTargetFromPreview(double x, double y)
{
    if (m_selectedZoomId.isEmpty()) return false;
    const bool result = setZoomTarget(m_selectedZoomId, QVariantMap{{QStringLiteral("x"), x}, {QStringLiteral("y"), y}});
    setPickingZoomTarget(false);
    return result;
}

bool Editor::removeZoom(const QString &id)
{
    const int index = zoomIndex(id);
    if (index < 0) return false;
    snapshot();
    m_project.zooms.removeAt(index);
    m_selectedZoomIds.removeAll(id);
    m_selectedZoomId = m_project.zooms.isEmpty() ? QString() : m_project.zooms[std::min(index, int(m_project.zooms.size()) - 1)].id;
    if (!m_selectedZoomId.isEmpty()) m_selectedZoomIds = {m_selectedZoomId};
    changed();
    emit selectionChanged();
    return true;
}

void Editor::regenerateZooms()
{
    snapshot();
    QVector<double> clicks;
    for (const auto &click : m_input.clickDowns(true)) clicks << click.time;
    m_project.zooms = ZoomTimeline::generate(clicks, m_sourceDuration);
    m_selectedZoomId = m_project.zooms.isEmpty() ? QString() : m_project.zooms.first().id;
    m_selectedZoomIds = m_selectedZoomId.isEmpty() ? QStringList{} : QStringList{m_selectedZoomId};
    changed();
    emit selectionChanged();
}

void Editor::setSelectedClipId(const QString &id)
{
    if (id != m_selectedClipId || !m_selectedZoomId.isEmpty()) {
        setPickingZoomTarget(false);
        m_selectedClipId = id;
        m_selectedZoomId.clear();
        m_selectedZoomIds.clear();
        emit selectionChanged();
    }
}

void Editor::setSelectedZoomId(const QString &id)
{
    if (id != m_selectedZoomId || !m_selectedClipId.isEmpty()) {
        setPickingZoomTarget(false);
        m_selectedZoomId = id;
        m_selectedZoomIds = id.isEmpty() ? QStringList{} : QStringList{id};
        m_selectedClipId.clear();
        emit selectionChanged();
    }
}

void Editor::setSelectedZoomIds(const QStringList &ids)
{
    QStringList valid;
    for (const QString &id : ids)
        if (zoomIndex(id) >= 0 && !valid.contains(id)) valid << id;
    const QString primary = valid.isEmpty() ? QString() : valid.first();
    if (valid == m_selectedZoomIds && primary == m_selectedZoomId
        && (valid.isEmpty() || m_selectedClipId.isEmpty())) return;
    setPickingZoomTarget(false);
    m_selectedZoomIds = valid;
    m_selectedZoomId = primary;
    if (!valid.isEmpty()) m_selectedClipId.clear();
    emit selectionChanged();
}

void Editor::selectZoomsInOutputRange(double outputStart, double outputEnd)
{
    if (outputStart > outputEnd) std::swap(outputStart, outputEnd);
    QStringList ids;
    for (const ZoomSegment &zoom : std::as_const(m_project.zooms)) {
        const double start = sourceToOutput(zoom.start);
        const double end = sourceToOutput(zoom.end);
        if (start >= 0.0 && end >= 0.0 && end >= outputStart && start <= outputEnd)
            ids << zoom.id;
    }
    setSelectedZoomIds(ids);
}

bool Editor::moveSelectedZooms(const QString &anchorId, double sourceStart)
{
    const int anchor = zoomIndex(anchorId);
    if (anchor < 0) return false;
    if (!m_selectedZoomIds.contains(anchorId)) setSelectedZoomId(anchorId);
    QSet<QString> selected(m_selectedZoomIds.begin(), m_selectedZoomIds.end());
    const double requested = sourceStart - m_project.zooms[anchor].start;
    double minimum = -m_sourceDuration;
    double maximum = m_sourceDuration;
    for (const ZoomSegment &zoom : std::as_const(m_project.zooms)) {
        if (!selected.contains(zoom.id)) continue;
        minimum = std::max(minimum, -zoom.start);
        maximum = std::min(maximum, m_sourceDuration - zoom.end);
        for (const ZoomSegment &other : std::as_const(m_project.zooms)) {
            if (selected.contains(other.id)) continue;
            if (other.end <= zoom.start)
                minimum = std::max(minimum, other.end - zoom.start);
            else if (other.start >= zoom.end)
                maximum = std::min(maximum, other.start - zoom.end);
        }
    }
    const double delta = std::clamp(requested, minimum, maximum);
    if (qFuzzyIsNull(delta)) return true;
    snapshot(m_coalesceKey.isEmpty() ? QStringLiteral("move-selection") : m_coalesceKey);
    for (ZoomSegment &zoom : m_project.zooms) {
        if (!selected.contains(zoom.id)) continue;
        zoom.start += delta;
        zoom.end += delta;
    }
    changed();
    return true;
}

int Editor::removeSelectedZooms()
{
    if (m_selectedZoomIds.isEmpty()) return 0;
    const QSet<QString> selected(m_selectedZoomIds.begin(), m_selectedZoomIds.end());
    const int before = m_project.zooms.size();
    snapshot();
    m_project.zooms.erase(std::remove_if(m_project.zooms.begin(), m_project.zooms.end(),
        [&selected](const ZoomSegment &zoom) { return selected.contains(zoom.id); }),
        m_project.zooms.end());
    m_selectedZoomId.clear();
    m_selectedZoomIds.clear();
    changed();
    emit selectionChanged();
    return before - m_project.zooms.size();
}
void Editor::setPickingZoomTarget(bool value)
{
    value = value && !m_selectedZoomId.isEmpty();
    if (value != m_pickingZoomTarget) {
        m_pickingZoomTarget = value;
        emit pickingZoomTargetChanged();
    }
}

void Editor::rebuildMotion()
{
    if (m_motionWatcher.isRunning()) return;
    m_runningMotionGeneration = m_motionGeneration;
    const auto events = m_input.events();
    const Project project = m_project;
    const double sourceDuration = m_sourceDuration, captureFps = m_fps;
    const int width = m_sourceWidth, height = m_sourceHeight;
    m_motionWatcher.setFuture(QtConcurrent::run([=] {
        return MotionTrack::build(sourceDuration, width, height, events, project, captureFps);
    }));
}

void Editor::applyMotionResult()
{
    m_motion = std::make_shared<MotionTrack>(m_motionWatcher.result());
    updatePreview();
    if (m_runningMotionGeneration != m_motionGeneration) m_motionTimer.start();
}

void Editor::startCameraThumbnailBuild()
{
    if (!m_hasCamera) return;
    const QFileInfo source(m_cameraVideoPath);
    const QByteArray fingerprint = QCryptographicHash::hash(
        (source.canonicalFilePath() + QLatin1Char('|') + QString::number(source.size())
         + QLatin1Char('|') + QString::number(source.lastModified().toMSecsSinceEpoch())).toUtf8(),
        QCryptographicHash::Sha256).toHex().left(16);
    const QString cacheDirectory = QDir(
        QStandardPaths::writableLocation(QStandardPaths::CacheLocation))
        .filePath(QStringLiteral("camera-filmstrips/") + QString::fromLatin1(fingerprint));
    m_cameraThumbnailWatcher.setFuture(QtConcurrent::run(
        buildCameraThumbnails, m_cameraVideoPath, m_cameraDuration, cacheDirectory));
}

void Editor::finishLoading()
{
    if (qEnvironmentVariableIntValue("OMAREEL_SCREENSHOT_LOADING") == 1) return;
    if (!m_loading) return;
    m_loading = false;
    emit loadingChanged();
}

QString Editor::presetsDirectory() const
{
    return QDir(QStandardPaths::writableLocation(QStandardPaths::ConfigLocation)).filePath(QStringLiteral("omareel/presets"));
}

QStringList Editor::presetNames() const
{
    QDir directory(presetsDirectory());
    QStringList names;
    for (const auto &file : directory.entryList({QStringLiteral("*.json")}, QDir::Files, QDir::Name))
        names << QFileInfo(file).completeBaseName();
    return names;
}

QJsonObject Editor::stylePreset() const
{
    const auto json = m_project.toJson();
    return {{QStringLiteral("version"), 1},
        {QStringLiteral("defaultsVersion"), Project::CurrentDefaultsVersion},
        {QStringLiteral("background"), json.value(QStringLiteral("background"))},
        {QStringLiteral("frame"), json.value(QStringLiteral("frame"))}, {QStringLiteral("cursor"), json.value(QStringLiteral("cursor"))},
        {QStringLiteral("zoomStyle"), json.value(QStringLiteral("zoomStyle"))}, {QStringLiteral("camera"), json.value(QStringLiteral("camera"))},
        {QStringLiteral("audio"), json.value(QStringLiteral("audio"))},
        {QStringLiteral("export"), json.value(QStringLiteral("export"))}};
}

void Editor::savePreset(const QString &rawName)
{
    const QString name = cleanPresetName(rawName);
    if (name.isEmpty()) return;
    QDir().mkpath(presetsDirectory());
    QSaveFile file(QDir(presetsDirectory()).filePath(name + QStringLiteral(".json")));
    if (!file.open(QIODevice::WriteOnly) || file.write(QJsonDocument(stylePreset()).toJson(QJsonDocument::Indented)) < 0 || !file.commit())
        emit errorOccurred(file.errorString());
    emit presetsChanged();
}

void Editor::loadPreset(const QString &rawName)
{
    QFile file(QDir(presetsDirectory()).filePath(cleanPresetName(rawName) + QStringLiteral(".json")));
    if (!file.open(QIODevice::ReadOnly)) { emit errorOccurred(file.errorString()); return; }
    const QJsonObject preset = Project::migrateDefaults(
        QJsonDocument::fromJson(file.readAll()).object());
    QJsonObject project = m_project.toJson();
    for (const QString &key : {QStringLiteral("background"), QStringLiteral("frame"), QStringLiteral("cursor"),
                               QStringLiteral("zoomStyle"), QStringLiteral("camera"), QStringLiteral("audio"),
                               QStringLiteral("export")})
        if (preset.contains(key)) project.insert(key, preset.value(key));
    snapshot();
    m_project = Project::fromJson(project);
    changed();
}

void Editor::deletePreset(const QString &rawName)
{
    QFile::remove(QDir(presetsDirectory()).filePath(cleanPresetName(rawName) + QStringLiteral(".json")));
    emit presetsChanged();
}

QVariantList Editor::wallpaperGroups() const
{
    if (m_wallpapersCacheValid) return m_wallpapersCache;
    QVariantList result;
    for (const QVariant &groupValue : OmarchyPaths::themeBackgroundGroups()) {
        QVariantMap group = groupValue.toMap();
        QVariantList wallpapers;
        for (const QVariant &pathValue : group.value(QStringLiteral("paths")).toList()) {
            const QString path = pathValue.toString();
            const QFileInfo file(path);
            const QString thumbnail = OmarchyPaths::wallpaperThumbnailPath(path);
            const bool ready = QFileInfo(thumbnail).isFile();
            wallpapers << QVariantMap{{QStringLiteral("name"), file.completeBaseName()},
                {QStringLiteral("file"), file.fileName()},
                {QStringLiteral("path"), file.absoluteFilePath()},
                {QStringLiteral("url"), QUrl::fromLocalFile(file.absoluteFilePath()).toString()},
                {QStringLiteral("thumbnailUrl"), QUrl::fromLocalFile(ready ? thumbnail : path).toString()}};
            if (!ready && !m_pendingThumbnails.contains(thumbnail)) {
                m_pendingThumbnails.insert(thumbnail);
                const QPointer<Editor> self(const_cast<Editor *>(this));
                (void) QtConcurrent::run([self, path, thumbnail] {
                    OmarchyPaths::generateWallpaperThumbnail(path, thumbnail);
                    if (!self) return;
                    QMetaObject::invokeMethod(self, [self, thumbnail] {
                        if (!self) return;
                        self->m_pendingThumbnails.remove(thumbnail);
                        self->m_wallpapersCacheValid = false;
                        emit self->wallpapersChanged();
                    }, Qt::QueuedConnection);
                });
            }
        }
        group.remove(QStringLiteral("paths"));
        group.insert(QStringLiteral("wallpapers"), wallpapers);
        result << group;
    }
    m_wallpapersCache = result;
    m_wallpapersCacheValid = true;
    return m_wallpapersCache;
}

void Editor::refreshOmarchyTheme()
{
    m_wallpapersCacheValid = false;
    m_projectMapCacheValid = false;
    emit wallpapersChanged();
    emit projectChanged();
}

void Editor::exportTo(const QString &pathValue, const QVariantMap &settings)
{
    if (m_exporting) return;
    saveNow();
    QString path = QUrl(pathValue).isLocalFile() ? QUrl(pathValue).toLocalFile() : pathValue;
    if (path.isEmpty()) return;
    if (!QDir().mkpath(QFileInfo(path).absolutePath())) {
        m_exportError = QStringLiteral("Could not create the output directory");
        emit exportErrorChanged();
        return;
    }
    ExportOptions options;
    options.bundlePath = m_bundlePath;
    options.outputPath = QFileInfo(path).absoluteFilePath();
    options.fps = settings.value(QStringLiteral("fps"), 0).toInt();
    options.quality = settings.value(QStringLiteral("quality")).toString();
    const int requestedHeight = settings.value(QStringLiteral("height"), 0).toInt();
    if (requestedHeight > 0) options.width = qRound(requestedHeight * aspectRatio(m_project, m_sourceWidth, m_sourceHeight));
    if (path.endsWith(QLatin1String(".gif"), Qt::CaseInsensitive)) {
        options.gifFps = options.fps;
        options.gifWidth = options.width;
        options.fps = 0;
        options.width = 0;
    }
    m_exporting = true;
    m_exportProgress = 0.0;
    m_exportFps = 0.0;
    m_exportEtaSeconds = 0;
    m_exportedPath.clear();
    m_exportTimer.start();
    m_exportError.clear();
    emit exportingChanged(); emit exportProgressChanged(); emit exportErrorChanged();
    m_exporter = new Exporter(this);
    connect(m_exporter, &Exporter::progress, this, [this](int frame, int total) {
        m_exportProgress = total > 0 ? frame / double(total) : 0.0;
        const double elapsed = m_exportTimer.elapsed() / 1000.0;
        m_exportFps = elapsed > 0.0 ? frame / elapsed : 0.0;
        m_exportEtaSeconds = m_exportFps > 0.0
            ? std::max(0, qRound((total - frame) / m_exportFps)) : 0;
        emit exportProgressChanged();
    });
    connect(m_exporter, &Exporter::finished, this, [this](const QString &result) {
        m_exporting = false; m_exportProgress = 1.0;
        m_exportEtaSeconds = 0;
        m_exportedPath = result;
        emit exportingChanged(); emit exportProgressChanged(); emit exportFinished(result);
        m_exporter->deleteLater();
        m_exporter = nullptr;
    });
    connect(m_exporter, &Exporter::failed, this, [this](const QString &error) {
        m_exporting = false; m_exportError = error;
        m_exportEtaSeconds = 0;
        emit exportingChanged(); emit exportErrorChanged();
        m_exporter->deleteLater();
        m_exporter = nullptr;
    });
    QTimer::singleShot(0, m_exporter, [exporter = m_exporter, options] { exporter->exportBundle(options); });
}

void Editor::cancelExport()
{
    if (m_exporter) m_exporter->cancel();
}

QString Editor::defaultExportPath(const QString &format) const
{
    return QDir(OmarchyPaths::recordingsDirectory()).filePath(
        m_project.name + (format == QLatin1String("gif") ? QStringLiteral(".gif") : QStringLiteral(".mp4")));
}

QString Editor::formatTime(double seconds) const
{
    const int total = std::max(0, qRound(seconds));
    return QStringLiteral("%1:%2").arg(total / 60, 2, 10, QLatin1Char('0')).arg(total % 60, 2, 10, QLatin1Char('0'));
}

void Editor::openContainingFolder(const QString &path) const
{
    const QFileInfo file(path);
    const QString directory = file.isDir() ? file.absoluteFilePath() : file.absolutePath();
    if (!directory.isEmpty()) QProcess::startDetached(QStringLiteral("xdg-open"), {directory});
}

void Editor::copyPath(const QString &path) const
{
    if (QGuiApplication::clipboard()) QGuiApplication::clipboard()->setText(path);
}
