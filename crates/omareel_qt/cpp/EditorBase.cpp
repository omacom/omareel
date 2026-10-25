#include "EditorBase.h"

#include <utility>

#include "Exporter.h"

#include <QClipboard>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QImageReader>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonValue>
#include <QProcess>
#include <QQuickWindow>
#include <QSize>
#include <QUrl>
#include <algorithm>

using namespace Omareel;

EditorBase::EditorBase(QObject *parent) : QObject(parent)
{
    m_autosaveTimer.setSingleShot(true);
    m_autosaveTimer.setInterval(500);
    m_motionTimer.setSingleShot(true);
    m_motionTimer.setInterval(100);
    m_scrubTimer.setSingleShot(true);
    m_scrubTimer.setInterval(35);
    m_previewTimer.setTimerType(Qt::PreciseTimer);
    m_previewTimer.setInterval(16);
    m_previewStatsTimer.setInterval(1000);
    connect(&m_autosaveTimer, &QTimer::timeout, this, [this] { timerFired(AutosaveTimer); });
    connect(&m_motionTimer, &QTimer::timeout, this, [this] { timerFired(MotionTimer); });
    connect(&m_scrubTimer, &QTimer::timeout, this, [this] { timerFired(ScrubTimer); });
    connect(&m_previewTimer, &QTimer::timeout, this, [this] { timerFired(PreviewTimer); });
    connect(&m_previewStatsTimer, &QTimer::timeout, this, [this] { timerFired(PreviewStatsTimer); });

    // The camera follows the transport; the screen and media players play while their clip is
    // the active one.
    m_playback.setPlayingQuery([this](PlaybackDriver::Player player) { return playerShouldPlay(player); });
    m_playback.setScreenRestQuery([this] { return screenRestPosition(); });
    connect(&m_playback, &PlaybackDriver::frameShown, this,
            [this](PlaybackDriver::Player player, qint64 startTimeUs, bool live) {
        handleFrameShown(player, startTimeUs, live);
    });
    connect(&m_playback, &PlaybackDriver::mediaFrameReadyChanged, this, [this] { handleMediaFrameReady(); });
    connect(&m_playback, &PlaybackDriver::warmingChanged, this, [this] { handleWarmingChanged(); });
    connect(&m_playback, &PlaybackDriver::mediaStatusChanged, this,
            [this](PlaybackDriver::Player player, QMediaPlayer::MediaStatus status) {
        handleMediaStatus(player, status);
    });
    connect(&m_playback, &PlaybackDriver::playbackStateChanged, this,
            [this](PlaybackDriver::Player player) { handlePlaybackState(player); });
}

EditorBase::~EditorBase()
{
    cancelExport();
    // The Rust object is gone by now: the players report nothing more as they are torn down.
    m_playback.disconnect(this);
    m_playback.setPlayingQuery({});
    m_playback.setScreenRestQuery({});
}

void EditorBase::attachVideoOutput(QObject *output)
{
    m_playback.attachVideoOutput(PlaybackDriver::Screen, output);
    m_playback.warmUp(PlaybackDriver::Screen);
}

void EditorBase::attachCameraVideoOutput(QObject *output)
{
    m_playback.attachVideoOutput(PlaybackDriver::Camera, output);
    cameraOutputAttached();
}

void EditorBase::attachMediaVideoOutput(QObject *output)
{
    if (!m_playback.attachVideoOutput(PlaybackDriver::Media, output)) return;
    mediaOutputAttached();
}

void EditorBase::cancelExport()
{
    if (m_exporter) m_exporter->cancel();
}

QTimer &EditorBase::timer(int timer)
{
    switch (timer) {
    case AutosaveTimer: return m_autosaveTimer;
    case MotionTimer: return m_motionTimer;
    case ScrubTimer: return m_scrubTimer;
    case PreviewTimer: return m_previewTimer;
    default: return m_previewStatsTimer;
    }
}

bool EditorBase::editorTimerActive(int timer) const
{
    return const_cast<EditorBase *>(this)->timer(timer).isActive();
}

void EditorBase::callLater(int call, int milliseconds)
{
    QTimer::singleShot(milliseconds, this, [this, call] { calledLater(call); });
}

void EditorBase::startExport(const omareel::exporter::ExportOptions &request)
{
    ExportOptions options;
    options.bundlePath = request.bundle_path;
    options.outputPath = request.output_path;
    options.fps = request.fps;
    options.width = request.width;
    options.quality = request.quality;
    options.gifFps = request.gif_fps;
    options.gifWidth = request.gif_width;
    options.timing = request.timing;
    m_exporter = new Exporter(this);
    connect(m_exporter, &Exporter::progress, this, [this](int frame, int total) { exportProgressed(frame, total); });
    // The finished exporter is released before the editor hears about it, so a handler that
    // starts another export keeps its new exporter.
    connect(m_exporter, &Exporter::finished, this, [this](const QString &result) {
        std::exchange(m_exporter, nullptr)->deleteLater();
        exportSucceeded(result);
    });
    connect(m_exporter, &Exporter::failed, this, [this](const QString &error) {
        std::exchange(m_exporter, nullptr)->deleteLater();
        exportFailedWith(error);
    });
    QTimer::singleShot(0, m_exporter, [exporter = m_exporter, options] { exporter->exportBundle(options); });
}

QString EditorShims::projectValueJson(const QVariant &value, bool takeLocalFile)
{
    QVariant normalized = value;
    if (takeLocalFile) {
        const QUrl url = value.toUrl();
        if (url.isLocalFile()) normalized = url.toLocalFile();
    }
    return QString::fromUtf8(
        QJsonDocument(QJsonArray{QJsonValue::fromVariant(normalized)}).toJson(QJsonDocument::Compact));
}

QString EditorShims::urlPath(const QUrl &url)
{
    return url.isLocalFile() ? url.toLocalFile() : url.toString();
}

QString EditorShims::textUrlPath(const QString &text)
{
    return QUrl(text).isLocalFile() ? QUrl(text).toLocalFile() : text;
}

bool EditorShims::zoomTarget(const QVariant &target, double &x, double &y)
{
    if (target.toString().compare(QStringLiteral("auto"), Qt::CaseInsensitive) == 0) return true;
    const QVariantMap point = target.toMap();
    x = point.value(QStringLiteral("x"), 0.5).toDouble();
    y = point.value(QStringLiteral("y"), 0.5).toDouble();
    return false;
}

void EditorShims::exportSettings(const QVariantMap &settings, int &fps, int &height, QString &quality)
{
    fps = settings.value(QStringLiteral("fps"), 0).toInt();
    quality = settings.value(QStringLiteral("quality")).toString();
    height = settings.value(QStringLiteral("height"), 0).toInt();
}

bool EditorShims::softwareRendering()
{
    return QQuickWindow::graphicsApi() == QSGRendererInterface::Software
        || QGuiApplication::platformName() == QLatin1String("offscreen");
}

void EditorShims::copyToClipboard(const QString &text)
{
    if (QGuiApplication::clipboard()) QGuiApplication::clipboard()->setText(text);
}

void EditorShims::openFolder(const QString &directory)
{
    QProcess::startDetached(QStringLiteral("xdg-open"), {directory});
}

bool EditorShims::generateWallpaperThumbnail(const QString &wallpaperPath, const QString &thumbnailPath)
{
    if (QFileInfo(thumbnailPath).isFile()) return true;
    QImageReader reader(wallpaperPath);
    reader.setAutoTransform(true);
    QImage image = reader.read();
    if (image.isNull()) return false;
    image = image.scaled(QSize(400, 240), Qt::KeepAspectRatioByExpanding,
                         Qt::SmoothTransformation);
    const int x = std::max(0, (image.width() - 400) / 2);
    const int y = std::max(0, (image.height() - 240) / 2);
    image = image.copy(x, y, std::min(400, image.width()), std::min(240, image.height()));
    if (!QDir().mkpath(QFileInfo(thumbnailPath).absolutePath())) return false;
    return image.save(thumbnailPath, "JPEG", 86);
}
