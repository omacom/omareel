#include "Editor.h"

#include "core/ClipTimeline.h"
#include "core/CameraTimeline.h"
#include "core/ZoomTimeline.h"
#include "core/OmarchyPaths.h"
#include "core/Theme.h"
#include "render/Exporter.h"
#include "render/FrameSource.h"
#include "render/PreviewSink.h"

#include <QAudioOutput>
#include <QClipboard>
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
#include <QUrl>
#include <QVideoSink>
#include <QUuid>
#include <QtConcurrent>
#include <algorithm>
#include <cmath>

using namespace OmaRecord;

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

QVariantList buildWaveform(const QString &videoPath)
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-i"), videoPath, QStringLiteral("-ac"), QStringLiteral("1"),
        QStringLiteral("-ar"), QStringLiteral("8000"), QStringLiteral("-f"), QStringLiteral("s16le"),
        QStringLiteral("-")});
    if (!process.waitForFinished(120000) || process.exitCode() != 0) return {};
    const QByteArray pcm = process.readAllStandardOutput();
    const auto *samples = reinterpret_cast<const qint16 *>(pcm.constData());
    const qsizetype sampleCount = pcm.size() / qsizetype(sizeof(qint16));
    if (sampleCount == 0) return {};
    constexpr int bins = 360;
    QVariantList peaks;
    peaks.reserve(bins);
    for (int bin = 0; bin < bins; ++bin) {
        const qsizetype begin = bin * sampleCount / bins;
        const qsizetype end = std::max(begin + 1, (bin + 1) * sampleCount / bins);
        int peak = 0;
        for (qsizetype i = begin; i < std::min(end, sampleCount); ++i)
            peak = std::max(peak, std::abs(int(samples[i])));
        peaks << peak / 32768.0;
    }
    return peaks;
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
    connect(&m_autosaveTimer, &QTimer::timeout, this, &Editor::saveNow);
    connect(&m_motionTimer, &QTimer::timeout, this, &Editor::rebuildMotion);
    connect(&m_motionWatcher, &QFutureWatcher<MotionTrack>::finished, this, &Editor::applyMotionResult);
    connect(&m_waveformWatcher, &QFutureWatcher<QVariantList>::finished, this, [this] {
        m_waveform = m_waveformWatcher.result();
        emit waveformChanged();
    });

    QFile gradientsFile(QStringLiteral(":/omarecord/assets/gradients.json"));
    if (gradientsFile.open(QIODevice::ReadOnly))
        m_gradients = QJsonDocument::fromJson(gradientsFile.readAll()).array().toVariantList();

    if (!loadBundle()) return;
    m_audioOutput = std::make_unique<QAudioOutput>();
    m_player.setAudioOutput(m_audioOutput.get());
    m_videoSink = std::make_unique<QVideoSink>();
    m_previewSink = std::make_unique<PreviewSink>(m_videoSink.get(), this);
    m_player.setVideoSink(m_videoSink.get());
    connect(m_videoSink.get(), &QVideoSink::videoFrameChanged, this, [this](const QVideoFrame &) {
        if (m_warmingPreview) {
            m_warmingPreview = false;
            QTimer::singleShot(0, this, &Editor::pause);
        }
    });
    m_player.setSource(QUrl::fromLocalFile(m_videoPath));
    connect(&m_player, &QMediaPlayer::positionChanged, this, &Editor::handlePlayerPosition);
    connect(&m_player, &QMediaPlayer::playbackStateChanged, this, [this] { emit playingChanged(); });
    if (m_hasCamera) {
        m_cameraVideoSink = std::make_unique<QVideoSink>();
        m_cameraPreviewSink = std::make_unique<PreviewSink>(m_cameraVideoSink.get(), this);
        m_cameraPlayer.setVideoSink(m_cameraVideoSink.get());
        connect(m_cameraVideoSink.get(), &QVideoSink::videoFrameChanged, this,
                [this](const QVideoFrame &) {
            if (!m_warmingCameraPreview) return;
            m_warmingCameraPreview = false;
            if (!playing()) m_cameraPlayer.pause();
        });
        m_cameraPlayer.setSource(QUrl::fromLocalFile(m_cameraVideoPath));
    }
    m_valid = true;
    ++m_motionGeneration;
    rebuildMotion();
    startWaveformBuild();
}

Editor::~Editor()
{
    m_autosaveTimer.stop();
    if (m_dirty) saveNow();
    m_motionWatcher.waitForFinished();
    m_waveformWatcher.waitForFinished();
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
        QString name = QFileInfo(m_bundlePath).completeBaseName();
        if (name.endsWith(QLatin1String(".omarecord"))) name.chop(10);
        m_project = Project::defaults(name, m_sourceDuration);
        m_project.camera.enabled = m_hasCamera;
        const int rotation = cameraCapture.value(QStringLiteral("rotation")).toInt(0);
        if (rotation == 0 || rotation == 90 || rotation == 180 || rotation == 270)
            m_project.camera.rotation = rotation;
        m_project.camera.flipHorizontal = cameraCapture
            .value(QStringLiteral("flipHorizontal")).toBool(false);
    }
    if (m_project.clips.isEmpty()) m_project.clips = {Clip{QStringLiteral("c1"), 0.0, m_sourceDuration, 1.0}};
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
    m_selectedClipId = m_project.clips.first().id;
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
bool Editor::playing() const { return m_player.playbackState() == QMediaPlayer::PlayingState; }

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

QVariantMap Editor::previewCursor() const
{
    const MotionSample s = m_motion ? m_motion->sample(sourcePosition()) : MotionSample{};
    return {{QStringLiteral("x"), s.cursorX}, {QStringLiteral("y"), s.cursorY},
        {QStringLiteral("visible"), m_project.cursor.visible}, {QStringLiteral("scale"), s.cursorScale},
        {QStringLiteral("opacity"), s.cursorOpacity}, {QStringLiteral("rotation"), s.cursorRotation}};
}

QVariantList Editor::previewRipples() const
{
    QVariantList result;
    if (!m_motion || (m_project.cursor.clickEffect != QLatin1String("ripple")
                      && m_project.cursor.clickEffect != QLatin1String("circle"))) return result;
    for (const auto &r : m_motion->ripples(sourcePosition()))
        result << QVariantMap{{QStringLiteral("x"), r.x}, {QStringLiteral("y"), r.y}, {QStringLiteral("progress"), r.progress}};
    return result;
}

QVariantList Editor::previewKeystrokePills() const
{
    QVariantList result;
    for (const KeystrokePill &pill : m_keystrokeTrack.sample(sourcePosition()))
        result << QVariantMap{{QStringLiteral("keys"), pill.keys},
            {QStringLiteral("text"), pill.text}, {QStringLiteral("opacity"), pill.opacity}};
    return result;
}

bool Editor::cameraVisible() const
{
    if (!m_hasCamera) return false;
    const CameraTime mapped = mapCameraTime(sourcePosition(), m_cameraOffset, m_cameraDuration);
    return !mapped.beforeStart;
}

void Editor::attachFrameSource(QObject *source)
{
    if (m_previewSink) m_previewSink->setFrameSource(qobject_cast<FrameSource *>(source));
    if (m_player.mediaStatus() != QMediaPlayer::InvalidMedia) {
        m_warmingPreview = true;
        m_player.play();
    }
}

void Editor::attachCameraFrameSource(QObject *source)
{
    if (m_cameraPreviewSink)
        m_cameraPreviewSink->setFrameSource(qobject_cast<FrameSource *>(source));
    syncCamera(sourcePosition(), true);
    if (m_hasCamera && m_cameraPlayer.mediaStatus() != QMediaPlayer::InvalidMedia) {
        m_warmingCameraPreview = true;
        m_cameraPlayer.play();
    }
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
    QStringList retained;
    for (const QString &id : std::as_const(m_selectedZoomIds))
        if (zoomIndex(id) >= 0) retained << id;
    m_selectedZoomIds = retained;
    if (zoomIndex(m_selectedZoomId) < 0)
        m_selectedZoomId = retained.isEmpty() ? QString() : retained.first();
    changed();
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
    return ClipTimeline(m_project.clips).sourceTime(std::clamp(outputTime, 0.0, duration()));
}

double Editor::sourceToOutput(double sourceTime, int preferredClip) const
{
    if (preferredClip >= 0 && preferredClip < m_project.clips.size())
        return ClipTimeline(m_project.clips).outputTime(preferredClip, sourceTime);
    for (int i = 0; i < m_project.clips.size(); ++i)
        if (sourceTime >= m_project.clips[i].in && sourceTime <= m_project.clips[i].out)
            return ClipTimeline(m_project.clips).outputTime(i, sourceTime);
    return -1.0;
}

void Editor::seek(double outputTime)
{
    if (m_project.clips.isEmpty()) return;
    m_outputPosition = std::clamp(outputTime, 0.0, duration());
    double clipStart = 0.0;
    m_activeClip = clipForOutput(m_outputPosition, &clipStart);
    const auto &clip = m_project.clips[m_activeClip];
    const double source = clip.in + (m_outputPosition - clipStart) * clip.speed;
    m_internalSeek = true;
    m_player.setPlaybackRate(clip.speed);
    m_player.setPosition(qRound64(std::clamp(source, clip.in, clip.out) * 1000.0));
    syncCamera(source, true);
    m_internalSeek = false;
    emit positionChanged();
    updatePreview();
}

void Editor::playPause() { playing() ? pause() : play(); }
void Editor::play()
{
    if (m_outputPosition >= duration() - 0.0001) seek(0);
    m_player.play();
    syncCamera(sourcePosition(), true);
}
void Editor::pause()
{
    m_player.pause();
    m_cameraPlayer.pause();
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
    if (m_internalSeek || m_project.clips.isEmpty()) return;
    const double source = milliseconds / 1000.0;
    syncCamera(source);
    const Clip &clip = m_project.clips[std::clamp(m_activeClip, 0, int(m_project.clips.size()) - 1)];
    if (playing() && source >= clip.out - 0.004) {
        if (m_activeClip + 1 < m_project.clips.size()) {
            double nextOutput = ClipTimeline(m_project.clips).outputTime(m_activeClip + 1, m_project.clips[m_activeClip + 1].in);
            seek(nextOutput);
            m_player.play();
        } else { m_outputPosition = duration(); pause(); emit positionChanged(); updatePreview(); }
        return;
    }
    const double mapped = sourceToOutput(source, m_activeClip);
    if (mapped >= 0.0) {
        m_outputPosition = std::clamp(mapped, 0.0, duration());
        emit positionChanged();
        updatePreview();
    }
}

void Editor::syncCamera(double screenSourceTime, bool force)
{
    if (!m_hasCamera) return;
    const CameraTime mapped = mapCameraTime(screenSourceTime, m_cameraOffset, m_cameraDuration);
    const qint64 target = qRound64(mapped.seconds * 1000.0);
    m_cameraPlayer.setPlaybackRate(m_player.playbackRate());
    if (force || std::abs(m_cameraPlayer.position() - target) > 80)
        m_cameraPlayer.setPosition(target);
    if (m_player.playbackState() == QMediaPlayer::PlayingState
        && !mapped.beforeStart && !mapped.beyondEnd)
        m_cameraPlayer.play();
    else
        m_cameraPlayer.pause();
}

void Editor::updatePreview() { emit compositionChanged(); }

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

bool Editor::trimClip(const QString &id, double newIn, double newOut)
{
    const int index = clipIndex(id);
    if (index < 0) return false;
    const double lower = index > 0 ? m_project.clips[index - 1].out : 0.0;
    const double upper = index + 1 < m_project.clips.size() ? m_project.clips[index + 1].in : m_sourceDuration;
    newIn = std::clamp(newIn, lower, upper);
    newOut = std::clamp(newOut, lower, upper);
    if (newOut - newIn < 0.1) return false;
    snapshot(QStringLiteral("trim-") + id);
    m_project.clips[index].in = newIn;
    m_project.clips[index].out = newOut;
    changed(false);
    return true;
}

bool Editor::removeClip(const QString &id)
{
    const int index = clipIndex(id);
    if (index < 0 || m_project.clips.size() <= 1) return false;
    snapshot();
    m_project.clips.removeAt(index);
    m_selectedClipId = m_project.clips[std::min(index, int(m_project.clips.size()) - 1)].id;
    seek(std::min(m_outputPosition, duration()));
    changed(false);
    emit selectionChanged();
    return true;
}

bool Editor::mergeClip(const QString &id, int direction)
{
    const int index = clipIndex(id);
    const int other = index + (direction < 0 ? -1 : 1);
    if (index < 0 || other < 0 || other >= m_project.clips.size()) return false;
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

bool Editor::resetClipTrims(const QString &id)
{
    const int index = clipIndex(id);
    if (index < 0) return false;
    const double newIn = index > 0 ? m_project.clips[index - 1].out : 0.0;
    const double newOut = index + 1 < m_project.clips.size()
        ? m_project.clips[index + 1].in : m_sourceDuration;
    if (newOut - newIn < 0.1) return false;
    snapshot();
    m_project.clips[index].in = newIn;
    m_project.clips[index].out = newOut;
    changed(false);
    return true;
}

QString Editor::addZoomAt(double outputTime, double length)
{
    const double start = std::clamp(outputToSource(outputTime), 0.0, std::max(0.0, m_sourceDuration - 1.0));
    const double end = std::min(m_sourceDuration, start + std::max(1.0, length));
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

void Editor::startWaveformBuild()
{
    if (!m_hasAudio) return;
    m_waveformWatcher.setFuture(QtConcurrent::run(buildWaveform, m_videoPath));
}

QString Editor::presetsDirectory() const
{
    return QDir(QStandardPaths::writableLocation(QStandardPaths::ConfigLocation)).filePath(QStringLiteral("omarecord/presets"));
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
    return {{QStringLiteral("version"), 1}, {QStringLiteral("background"), json.value(QStringLiteral("background"))},
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
    const QJsonObject preset = QJsonDocument::fromJson(file.readAll()).object();
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
