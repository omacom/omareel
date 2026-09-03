#include "Exporter.h"

#include "FfmpegDecoder.h"
#include "FrameSource.h"
#include "core/ClipTimeline.h"
#include "core/CameraTimeline.h"
#include "core/InputLog.h"
#include "core/MotionTrack.h"
#include "core/OmarchyPaths.h"
#include "core/Project.h"
#include "core/ZoomTimeline.h"

#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QOffscreenSurface>
#include <QOpenGLContext>
#include <QOpenGLExtraFunctions>
#include <QOpenGLFramebufferObject>
#include <QOpenGLFunctions>
#include <QProcess>
#include <QQuickItem>
#include <QQuickGraphicsDevice>
#include <QQuickRenderControl>
#include <QQuickRenderTarget>
#include <QQuickWindow>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QStandardPaths>
#include <QTemporaryFile>
#include <QTextStream>
#include <cmath>
#include <array>
#include <cstring>
#include <condition_variable>
#include <deque>
#include <mutex>
#include <thread>

using namespace OmaRecord;

QSize OmaRecord::paddedEvenSize(int width, int height)
{
    return {width + std::abs(width % 2), height + std::abs(height % 2)};
}

struct MediaInfo { int width = 0; int height = 0; double duration = 0.0; double fps = 30.0; bool audio = false; };

static double rateValue(const QString &value)
{
    const QStringList parts = value.split('/');
    return parts.size() == 2 && parts[1].toDouble() != 0.0
        ? parts[0].toDouble() / parts[1].toDouble() : value.toDouble();
}

static MediaInfo probe(const QString &path, QString *error)
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
    MediaInfo result;
    result.duration = root.value("format").toObject().value("duration").toString().toDouble();
    for (const auto &value : root.value("streams").toArray()) {
        const auto stream = value.toObject();
        if (stream.value("codec_type") == QLatin1String("video") && result.width == 0) {
            result.width = stream.value("width").toInt();
            result.height = stream.value("height").toInt();
            result.fps = rateValue(stream.value("r_frame_rate").toString());
        } else if (stream.value("codec_type") == QLatin1String("audio")) result.audio = true;
    }
    if (result.width <= 0 || result.height <= 0 || result.duration <= 0.0) {
        if (error) *error = QStringLiteral("ffprobe returned invalid video metadata");
    }
    if (result.fps <= 0.0) result.fps = 30.0;
    return result;
}

static double outputAspect(const QString &aspect, int sourceWidth, int sourceHeight, const QRectF &crop)
{
    if (aspect == QLatin1String("16:9")) return 16.0 / 9.0;
    if (aspect == QLatin1String("1:1")) return 1.0;
    if (aspect == QLatin1String("4:3")) return 4.0 / 3.0;
    if (aspect == QLatin1String("9:16")) return 9.0 / 16.0;
    if (aspect == QLatin1String("3:4")) return 3.0 / 4.0;
    if (aspect == QLatin1String("4:5")) return 4.0 / 5.0;
    return (sourceWidth * crop.width()) / std::max(1.0, sourceHeight * crop.height());
}

static QVariantMap projectMap(const Project &project)
{
    QVariantMap map = project.toJson().toVariantMap();
    QVariantMap background = map.value(QStringLiteral("background")).toMap();
    QString image;
    if (project.background.type == QLatin1String("image")) image = project.background.image;
    else if (project.background.type == QLatin1String("wallpaper"))
        image = project.background.wallpaper == QLatin1String("omarchy:current")
            ? OmarchyPaths::currentBackground() : project.background.wallpaper;
    background[QStringLiteral("resolvedImage")] = image.isEmpty() ? QString() : QUrl::fromLocalFile(image).toString();
    map[QStringLiteral("background")] = background;
    return map;
}

static QString atempo(double speed)
{
    QStringList filters;
    while (speed > 2.0) { filters << QStringLiteral("atempo=2"); speed /= 2.0; }
    while (speed < 0.5) { filters << QStringLiteral("atempo=0.5"); speed /= 0.5; }
    filters << QStringLiteral("atempo=%1").arg(speed, 0, 'g', 8);
    return filters.join(',');
}

static QString audioFilter(const QVector<Clip> &clips, double volume)
{
    QString filter;
    QStringList labels;
    for (int i = 0; i < clips.size(); ++i) {
        const auto &clip = clips[i];
        const QString label = QStringLiteral("a%1").arg(i);
        filter += QStringLiteral("[1:a]atrim=start=%1:end=%2,asetpts=PTS-STARTPTS,%3[%4];")
            .arg(clip.in, 0, 'f', 6).arg(clip.out, 0, 'f', 6).arg(atempo(clip.speed), label);
        labels << QStringLiteral("[%1]").arg(label);
    }
    filter += labels.join(QString()) + QStringLiteral("concat=n=%1:v=0:a=1,volume=%2[aout]")
        .arg(clips.size()).arg(volume, 0, 'g', 8);
    return filter;
}

static bool nvencWorks()
{
    QProcess process;
    process.start(QStringLiteral("ffmpeg"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("color=size=256x256:rate=1"),
        QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-c:v"), QStringLiteral("h264_nvenc"),
        QStringLiteral("-f"), QStringLiteral("null"), QStringLiteral("-")});
    return process.waitForFinished(15000) && process.exitCode() == 0;
}

static QString normalizedQuality(QString quality)
{
    if (quality == QLatin1String("low")) return QStringLiteral("web-low");
    if (quality == QLatin1String("medium")) return QStringLiteral("web-high");
    if (quality == QLatin1String("high")) return QStringLiteral("social");
    if (quality == QLatin1String("best")) return QStringLiteral("studio");
    return quality;
}

static double bitrateMultiplier(const QString &quality)
{
    if (quality == QLatin1String("web-low")) return 0.0075;
    if (quality == QLatin1String("web-high")) return 0.0175;
    if (quality == QLatin1String("studio")) return 0.3;
    return 0.05;
}

void Exporter::exportBundle(const ExportOptions &options)
{
    m_cancelled = false;
    QString error;
    if (run(options, &error)) emit finished(QFileInfo(options.outputPath).absoluteFilePath());
    else emit failed(error.isEmpty() ? QStringLiteral("Export failed") : error);
}

bool Exporter::run(const ExportOptions &options, QString *error)
{
    QElapsedTimer totalTimer;
    totalTimer.start();
    qint64 decodeNs = 0;
    qint64 uploadNs = 0;
    qint64 renderNs = 0;
    qint64 readbackNs = 0;
    qint64 encodeWriteNs = 0;
    const QDir bundle(options.bundlePath);
    const QString videoPath = bundle.filePath(QStringLiteral("screen.mp4"));
    const QString cameraPath = bundle.filePath(QStringLiteral("camera.mp4"));
    const MediaInfo media = probe(videoPath, error);
    if (media.width <= 0) return false;

    Project project;
    const QString projectPath = bundle.filePath(QStringLiteral("project.json"));
    if (QFileInfo(projectPath).isFile()) {
        project = Project::load(projectPath, error);
        if (error && !error->isEmpty()) return false;
    } else {
        project = Project::defaults(bundle.dirName(), media.duration);
        project.camera.enabled = QFileInfo(cameraPath).isFile();
        QString inputError;
        const auto input = InputLog::loadBundle(bundle.absolutePath(), media.duration, &inputError);
        if (!inputError.isEmpty()) { if (error) *error = inputError; return false; }
        QVector<double> clicks;
        for (const auto &event : input.clickDowns(true)) clicks << event.time;
        project.zooms = ZoomTimeline::generate(clicks, media.duration);
    }
    if (project.clips.isEmpty()) project.clips = {Clip{QStringLiteral("c1"), 0.0, media.duration, 1.0}};
    ClipTimeline timeline(project.clips);
    const double outputDuration = timeline.totalDuration();
    if (outputDuration <= 0.0) { if (error) *error = QStringLiteral("Project has no exportable clips"); return false; }

    QString inputError;
    const auto input = InputLog::loadBundle(bundle.absolutePath(), media.duration, &inputError);
    if (!inputError.isEmpty()) { if (error) *error = inputError; return false; }
    double captureFps = 60.0;
    double cameraOffset = 0.0;
    QJsonObject captureRoot;
    QFile capture(bundle.filePath(QStringLiteral("capture.json")));
    if (capture.open(QIODevice::ReadOnly)) {
        captureRoot = QJsonDocument::fromJson(capture.readAll()).object();
        captureFps = captureRoot.value("fps").toDouble(60.0);
        const qint64 screenFirst = captureRoot.value(QStringLiteral("first_frame_us")).toVariant().toLongLong();
        const qint64 cameraFirst = captureRoot.value(QStringLiteral("camera")).toObject()
                                       .value(QStringLiteral("first_frame_us")).toVariant().toLongLong();
        if (screenFirst > 0 && cameraFirst > 0)
            cameraOffset = (cameraFirst - screenFirst) / 1000000.0;
    }
    QString cameraProbeError;
    const MediaInfo cameraMedia = QFileInfo(cameraPath).isFile() ? probe(cameraPath, &cameraProbeError) : MediaInfo{};
    const bool cameraEnabled = project.camera.enabled && cameraMedia.width > 0;
    const MotionTrack motion = MotionTrack::build(media.duration, media.width, media.height,
                                                   input.events(), project, captureFps);

    const bool gif = options.outputPath.endsWith(QStringLiteral(".gif"), Qt::CaseInsensitive);
    const int fps = options.fps > 0 ? options.fps : (gif && options.gifFps > 0 ? options.gifFps
                                                    : gif ? project.exportSettings.gif.fps : project.exportSettings.fps);
    const double aspect = outputAspect(project.aspect, media.width, media.height, project.crop);
    int width = gif && options.gifWidth > 0 ? options.gifWidth : options.width;
    int height = 0;
    if (width > 0) height = qRound(width / aspect);
    else {
        height = gif ? project.exportSettings.gif.height : project.exportSettings.height;
        width = qRound(height * aspect);
    }
    const QSize evenSize = paddedEvenSize(width, height);
    width = evenSize.width();
    height = evenSize.height();
    const int totalFrames = std::max(1, int(std::ceil(outputDuration * fps)));

    double maximumZoom = 1.0;
    for (const auto &zoom : project.zooms) maximumZoom = std::max(maximumZoom, zoom.level);
    const double neededWidth = maximumZoom * width / std::max(0.0001, project.crop.width());
    const double neededHeight = maximumZoom * height / std::max(0.0001, project.crop.height());
    const double decodeScale = std::clamp(std::max(neededWidth / media.width,
                                                   neededHeight / media.height), 0.01, 1.0);
    const int decodeWidth = std::min(media.width, std::max(2, qRound(media.width * decodeScale / 2.0) * 2));
    const int decodeHeight = std::min(media.height, std::max(2, qRound(media.height * decodeScale / 2.0) * 2));
    const double cameraDecodeScale = cameraEnabled
        ? std::clamp(height * project.camera.size * 1.5 / cameraMedia.height, 0.01, 1.0) : 1.0;
    const int cameraDecodeWidth = cameraEnabled
        ? std::min(cameraMedia.width, std::max(2, qRound(cameraMedia.width * cameraDecodeScale / 2.0) * 2)) : 0;
    const int cameraDecodeHeight = cameraEnabled
        ? std::min(cameraMedia.height, std::max(2, qRound(cameraMedia.height * cameraDecodeScale / 2.0) * 2)) : 0;

    const bool softwareRendering = QQuickWindow::graphicsApi() == QSGRendererInterface::Software
                                || QGuiApplication::platformName() == QLatin1String("offscreen");
    std::unique_ptr<QOpenGLContext> context;
    std::unique_ptr<QOffscreenSurface> surface;
    std::unique_ptr<QOpenGLFramebufferObject> fbo;
    if (!softwareRendering) {
        context = std::make_unique<QOpenGLContext>();
        if (!context->create()) { if (error) *error = QStringLiteral("Could not create OpenGL context"); return false; }
        surface = std::make_unique<QOffscreenSurface>();
        surface->setFormat(context->format());
        surface->create();
        if (!surface->isValid() || !context->makeCurrent(surface.get())) {
            if (error) *error = QStringLiteral("Could not create an offscreen OpenGL surface");
            return false;
        }
        QOpenGLFramebufferObjectFormat fboFormat;
        fboFormat.setAttachment(QOpenGLFramebufferObject::CombinedDepthStencil);
        fbo = std::make_unique<QOpenGLFramebufferObject>(width, height, fboFormat);
        if (!fbo->isValid()) { if (error) *error = QStringLiteral("Could not create render framebuffer"); return false; }
    }
    std::unique_ptr<QQuickRenderControl> renderControl;
    std::unique_ptr<QQuickWindow> window;
    if (softwareRendering) {
        // Qt's software scene-graph adaptation rejects QQuickRenderControl. A hidden
        // offscreen QQuickWindow keeps headless export functional on hosts with no EGL/GL.
        window = std::make_unique<QQuickWindow>();
    } else {
        renderControl = std::make_unique<QQuickRenderControl>();
        window = std::make_unique<QQuickWindow>(renderControl.get());
        window->setGraphicsDevice(QQuickGraphicsDevice::fromOpenGLContext(context.get()));
        QQuickRenderTarget target = QQuickRenderTarget::fromOpenGLTexture(fbo->texture(), {width, height});
        window->setRenderTarget(target);
    }
    window->setGeometry(0, 0, width, height);
    window->setColor(Qt::transparent);
    if (!softwareRendering && !renderControl->initialize()) {
        if (error) *error = QStringLiteral("Could not initialize Qt Quick render control");
        return false;
    }

    CompositionState state;
    state.outputWidth = width; state.outputHeight = height;
    state.softwareRendering = softwareRendering;
    state.sourceWidth = media.width; state.sourceHeight = media.height;
    state.cameraAvailable = cameraMedia.width > 0;
    state.cameraSourceWidth = cameraMedia.width;
    state.cameraSourceHeight = cameraMedia.height;
    state.project = projectMap(project);
    state.zoom = {{QStringLiteral("scale"), 1.0}, {QStringLiteral("cx"), 0.5}, {QStringLiteral("cy"), 0.5}};
    state.cursor = {{QStringLiteral("x"), 0.5}, {QStringLiteral("y"), 0.5},
        {QStringLiteral("visible"), project.cursor.visible}, {QStringLiteral("scale"), 1.0},
        {QStringLiteral("opacity"), project.cursor.visible ? 1.0 : 0.0}, {QStringLiteral("rotation"), 0.0}};
    QQmlEngine engine;
    engine.rootContext()->setContextProperty(QStringLiteral("comp"), &state);
    QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/Omarecord/Composition.qml")));
    if (component.status() != QQmlComponent::Ready) {
        if (error) *error = component.errorString();
        return false;
    }
    std::unique_ptr<QObject> rootObject(component.create());
    auto *rootItem = qobject_cast<QQuickItem *>(rootObject.get());
    if (!rootItem) { if (error) *error = QStringLiteral("Composition.qml did not create a QQuickItem"); return false; }
    rootItem->setParentItem(window->contentItem());
    rootItem->setSize(QSizeF(width, height));
    auto *frameSource = rootItem->findChild<FrameSource *>(QStringLiteral("videoFrameSource"));
    if (!frameSource) { if (error) *error = QStringLiteral("Composition has no FrameSource"); return false; }
    auto *cameraFrameSource = rootItem->findChild<FrameSource *>(QStringLiteral("cameraFrameSource"));
    if (!cameraFrameSource) { if (error) *error = QStringLiteral("Composition has no camera FrameSource"); return false; }

    QString runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    if (runtime.isEmpty() || !QFileInfo(runtime).isWritable()) runtime = QDir::tempPath();
    QTemporaryFile rawFile(QDir(runtime).filePath(QStringLiteral("omarecord-export-XXXXXX.rgba")));
    QTemporaryFile encodedFile(QFileInfo(options.outputPath).absoluteDir().filePath(
        QStringLiteral(".omarecord-export-XXXXXX.mp4")));
    QString encodedPath = options.outputPath;
    if (!gif) {
        if (!encodedFile.open()) { if (error) *error = encodedFile.errorString(); return false; }
        encodedPath = encodedFile.fileName();
        encodedFile.close();
    }
    QStringList encoderArgs;
    bool useAudio = !gif && media.audio && (project.audio.desktop || project.audio.mic);
    if (gif) {
        if (!rawFile.open()) { if (error) *error = rawFile.errorString(); return false; }
    } else {
        encoderArgs = {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("rawvideo"), QStringLiteral("-pix_fmt"), QStringLiteral("rgba"),
            QStringLiteral("-s:v"), QStringLiteral("%1x%2").arg(width).arg(height),
            QStringLiteral("-r"), QString::number(fps), QStringLiteral("-i"), QStringLiteral("-")};
        if (!softwareRendering)
            encoderArgs << QStringLiteral("-vf") << QStringLiteral("vflip");
        if (useAudio) encoderArgs << QStringLiteral("-i") << videoPath
                           << QStringLiteral("-filter_complex") << audioFilter(project.clips, project.audio.volume)
                           << QStringLiteral("-map") << QStringLiteral("0:v:0")
                           << QStringLiteral("-map") << QStringLiteral("[aout]")
                           << QStringLiteral("-c:a") << QStringLiteral("aac");
        else encoderArgs << QStringLiteral("-an");
        const QString quality = normalizedQuality(options.quality.isEmpty() ? project.exportSettings.quality : options.quality);
        const qint64 bitrate = qint64(std::floor(width * double(height) * fps * bitrateMultiplier(quality)));
        if (!qEnvironmentVariableIsSet("OMARECORD_DISABLE_NVENC") && nvencWorks()) {
            encoderArgs << QStringLiteral("-c:v") << QStringLiteral("h264_nvenc")
                 << QStringLiteral("-preset") << QStringLiteral("p5")
                 << QStringLiteral("-b:v") << QString::number(bitrate)
                 << QStringLiteral("-maxrate") << QString::number(qint64(bitrate * 1.2))
                 << QStringLiteral("-bufsize") << QString::number(bitrate * 2)
                 << QStringLiteral("-profile:v") << QStringLiteral("high");
        } else {
            encoderArgs << QStringLiteral("-c:v") << QStringLiteral("libx264")
                 << QStringLiteral("-preset") << QStringLiteral("medium")
                 << QStringLiteral("-b:v") << QString::number(bitrate)
                 << QStringLiteral("-maxrate") << QString::number(qint64(bitrate * 1.2))
                 << QStringLiteral("-bufsize") << QString::number(bitrate * 2)
                 << QStringLiteral("-profile:v") << QStringLiteral("high");
        }
        encoderArgs << QStringLiteral("-pix_fmt") << QStringLiteral("yuv420p")
             << QStringLiteral("-movflags") << QStringLiteral("+faststart") << encodedPath;
    }

    std::mutex encodeMutex;
    std::condition_variable encodeReady;
    std::condition_variable encodeSpace;
    std::deque<QByteArray> encodeQueue;
    bool encodeStarted = gif;
    bool encodeStartOk = gif;
    bool encodeClosing = false;
    bool encodeAbort = false;
    QString encodeError;
    std::thread encodeThread;
    if (!gif) {
        encodeThread = std::thread([&] {
            QProcess process;
            process.start(QStringLiteral("ffmpeg"), encoderArgs, QIODevice::WriteOnly);
            const bool started = process.waitForStarted(10000);
            {
                std::lock_guard lock(encodeMutex);
                encodeStarted = true;
                encodeStartOk = started;
                if (!started) encodeError = process.errorString();
            }
            encodeReady.notify_all();
            if (!started) return;
            while (true) {
                QByteArray bytes;
                {
                    std::unique_lock lock(encodeMutex);
                    encodeReady.wait(lock, [&] {
                        return !encodeQueue.empty() || encodeClosing || encodeAbort || m_cancelled;
                    });
                    if (encodeAbort || m_cancelled) {
                        process.kill();
                        process.waitForFinished(2000);
                        return;
                    }
                    if (encodeQueue.empty() && encodeClosing) break;
                    bytes = std::move(encodeQueue.front());
                    encodeQueue.pop_front();
                }
                encodeSpace.notify_one();
                QElapsedTimer writeTimer;
                writeTimer.start();
                qint64 written = 0;
                while (written < bytes.size()) {
                    const qint64 amount = process.write(bytes.constData() + written,
                                                        bytes.size() - written);
                    if (amount < 0 || !process.waitForBytesWritten(30000)) {
                        std::lock_guard lock(encodeMutex);
                        encodeError = QString::fromUtf8(process.readAllStandardError()).trimmed();
                        encodeAbort = true;
                        encodeSpace.notify_all();
                        process.kill();
                        process.waitForFinished(2000);
                        return;
                    }
                    written += amount;
                }
                encodeWriteNs += writeTimer.nsecsElapsed();
            }
            process.closeWriteChannel();
            if (!process.waitForFinished(120000) || process.exitCode() != 0) {
                std::lock_guard lock(encodeMutex);
                encodeError = QString::fromUtf8(process.readAllStandardError()).trimmed();
            }
        });
        std::unique_lock lock(encodeMutex);
        encodeReady.wait(lock, [&] { return encodeStarted; });
        if (!encodeStartOk) {
            lock.unlock();
            encodeThread.join();
            if (error) *error = encodeError;
            return false;
        }
    }
    const auto finishEncoder = [&](bool abort) {
        if (gif) return;
        {
            std::lock_guard lock(encodeMutex);
            encodeAbort = abort;
            encodeClosing = !abort;
        }
        encodeReady.notify_all();
        encodeSpace.notify_all();
        if (encodeThread.joinable()) encodeThread.join();
    };

    struct ReadbackSlot { GLuint buffer = 0; int frame = -1; };
    std::array<ReadbackSlot, 8> readbacks;
    const qsizetype frameBytes = qsizetype(width) * height * 4;
    QOpenGLExtraFunctions *glExtra = nullptr;
    if (!softwareRendering) {
        glExtra = context->extraFunctions();
        glExtra->initializeOpenGLFunctions();
        for (auto &slot : readbacks) {
            glExtra->glGenBuffers(1, &slot.buffer);
            glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, slot.buffer);
            glExtra->glBufferData(GL_PIXEL_PACK_BUFFER, frameBytes, nullptr, GL_STREAM_READ);
        }
        glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
    }

    struct DecodedPacket { QImage image; QImage cameraImage; QString error; };
    std::mutex decodeMutex;
    std::condition_variable decodeReady;
    std::condition_variable decodeSpace;
    std::deque<DecodedPacket> decodeQueue;
    bool decodeDone = false;
    std::atomic_bool stopDecode{false};
    std::thread decodeThread([&] {
        FfmpegDecoder decoder;
        FfmpegDecoder cameraDecoder;
        int activeClip = -1;
        QImage decoded;
        QImage cameraDecoded;
        int cameraFrame = -1;
        bool cameraEof = false;
        if (cameraEnabled) {
            QString cameraError;
            if (!cameraDecoder.start(cameraPath, 0.0, cameraMedia.duration, cameraMedia.fps,
                                     cameraDecodeWidth, cameraDecodeHeight,
                                     cameraDecodeWidth != cameraMedia.width
                                         || cameraDecodeHeight != cameraMedia.height,
                                     &cameraError)) {
                std::lock_guard lock(decodeMutex);
                decodeQueue.push_back({{}, {}, cameraError});
                decodeDone = true;
                decodeReady.notify_all();
                return;
            }
        }
        for (int frame = 0; frame < totalFrames && !stopDecode && !m_cancelled; ++frame) {
            const double outputTime = frame / double(fps);
            double clipOutputStart = 0.0;
            int clipIndex = project.clips.size() - 1;
            for (int i = 0; i < project.clips.size(); ++i) {
                const double length = (project.clips[i].out - project.clips[i].in) / project.clips[i].speed;
                if (outputTime < clipOutputStart + length || i == project.clips.size() - 1) {
                    clipIndex = i;
                    break;
                }
                clipOutputStart += length;
            }
            const auto &clip = project.clips[clipIndex];
            QString decodeError;
            if (clipIndex != activeClip) {
                if (!decoder.start(videoPath, clip.in, clip.out - clip.in, fps / clip.speed,
                                   decodeWidth, decodeHeight,
                                   decodeWidth != media.width || decodeHeight != media.height,
                                   &decodeError)) {
                    std::lock_guard lock(decodeMutex);
                    decodeQueue.push_back({{}, {}, decodeError});
                    break;
                }
                activeClip = clipIndex;
            }
            QImage next;
            if (decoder.readFrame(&next, &decodeError)) decoded = std::move(next);
            else if (decoded.isNull()) {
                std::lock_guard lock(decodeMutex);
                decodeQueue.push_back({{}, {}, decodeError.isEmpty()
                    ? QStringLiteral("Decoder ended before the first frame") : decodeError});
                break;
            }
            if (cameraEnabled) {
                const double screenSourceTime = timeline.sourceTime(outputTime);
                const CameraTime mapped = mapCameraTime(screenSourceTime, cameraOffset,
                                                        cameraMedia.duration);
                const int wanted = std::max(0, int(std::floor(mapped.seconds * cameraMedia.fps + 1e-6)));
                if (wanted < cameraFrame) {
                    cameraDecoder.cancel();
                    cameraFrame = -1;
                    cameraEof = false;
                    if (!cameraDecoder.start(cameraPath, 0.0, cameraMedia.duration, cameraMedia.fps,
                                             cameraDecodeWidth, cameraDecodeHeight,
                                             cameraDecodeWidth != cameraMedia.width
                                                 || cameraDecodeHeight != cameraMedia.height,
                                             &decodeError)) {
                        std::lock_guard lock(decodeMutex);
                        decodeQueue.push_back({{}, {}, decodeError});
                        break;
                    }
                }
                while (!cameraEof && cameraFrame < wanted) {
                    QImage nextCamera;
                    if (cameraDecoder.readFrame(&nextCamera, &decodeError)) {
                        cameraDecoded = std::move(nextCamera);
                        ++cameraFrame;
                    } else {
                        cameraEof = true;
                        if (cameraDecoded.isNull()) {
                            std::lock_guard lock(decodeMutex);
                            decodeQueue.push_back({{}, {}, decodeError.isEmpty()
                                ? QStringLiteral("Camera decoder ended before the first frame") : decodeError});
                        }
                    }
                }
                if (cameraDecoded.isNull()) break;
            }
            std::unique_lock lock(decodeMutex);
            decodeSpace.wait(lock, [&] { return decodeQueue.size() < 3 || stopDecode || m_cancelled; });
            if (stopDecode || m_cancelled) break;
            decodeQueue.push_back({decoded, cameraDecoded, {}});
            lock.unlock();
            decodeReady.notify_one();
        }
        decoder.cancel();
        cameraDecoder.cancel();
        {
            std::lock_guard lock(decodeMutex);
            decodeDone = true;
        }
        decodeReady.notify_all();
    });
    const auto finishDecoder = [&] {
        stopDecode = true;
        decodeReady.notify_all();
        decodeSpace.notify_all();
        if (decodeThread.joinable()) decodeThread.join();
    };

    QString pipelineError;
    const auto writeFrame = [&](const QByteArray &bytes) {
        if (gif) {
            QElapsedTimer writeTimer;
            writeTimer.start();
            if (rawFile.write(bytes) != bytes.size()) pipelineError = rawFile.errorString();
            encodeWriteNs += writeTimer.nsecsElapsed();
        } else {
            std::unique_lock lock(encodeMutex);
            encodeSpace.wait(lock, [&] {
                return encodeQueue.size() < 4 || encodeAbort || !encodeError.isEmpty() || m_cancelled;
            });
            if (encodeAbort || !encodeError.isEmpty() || m_cancelled) {
                pipelineError = !encodeError.isEmpty() ? encodeError : QStringLiteral("Export cancelled");
            } else {
                encodeQueue.push_back(bytes);
                lock.unlock();
                encodeReady.notify_one();
            }
        }
    };
    const auto mapReadback = [&](ReadbackSlot &slot) {
        QByteArray bytes(frameBytes, Qt::Uninitialized);
        glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, slot.buffer);
        const void *mapped = glExtra->glMapBufferRange(GL_PIXEL_PACK_BUFFER, 0, frameBytes,
                                                       GL_MAP_READ_BIT);
        if (!mapped) {
            pipelineError = QStringLiteral("Could not map asynchronous frame readback");
            return QByteArray();
        }
        std::memcpy(bytes.data(), mapped, frameBytes);
        glExtra->glUnmapBuffer(GL_PIXEL_PACK_BUFFER);
        slot.frame = -1;
        return bytes;
    };
    for (int frame = 0; frame < totalFrames; ++frame) {
        if (m_cancelled) { pipelineError = QStringLiteral("Export cancelled"); break; }
        const double outputTime = frame / double(fps);
        QElapsedTimer stageTimer;
        stageTimer.start();
        DecodedPacket packet;
        {
            std::unique_lock lock(decodeMutex);
            decodeReady.wait(lock, [&] { return !decodeQueue.empty() || decodeDone || m_cancelled; });
            if (!decodeQueue.empty()) {
                packet = std::move(decodeQueue.front());
                decodeQueue.pop_front();
            }
        }
        decodeSpace.notify_one();
        decodeNs += stageTimer.nsecsElapsed();
        if (packet.image.isNull()) {
            pipelineError = !packet.error.isEmpty() ? packet.error
                          : m_cancelled ? QStringLiteral("Export cancelled")
                          : QStringLiteral("Decoder ended unexpectedly");
            break;
        }

        const double sourceTime = timeline.sourceTime(outputTime);
        const MotionSample sample = motion.sample(sourceTime);
        state.time = sourceTime;
        state.zoom = {{QStringLiteral("scale"), sample.zoomScale}, {QStringLiteral("cx"), sample.zoomCx}, {QStringLiteral("cy"), sample.zoomCy}};
        state.cursor = {{QStringLiteral("x"), sample.cursorX}, {QStringLiteral("y"), sample.cursorY},
            {QStringLiteral("visible"), project.cursor.visible}, {QStringLiteral("scale"), sample.cursorScale},
            {QStringLiteral("opacity"), sample.cursorOpacity}, {QStringLiteral("rotation"), sample.cursorRotation}};
        state.ripples.clear();
        if (project.cursor.clickEffect == QLatin1String("ripple") || project.cursor.clickEffect == QLatin1String("circle")) {
            for (const auto &ripple : motion.ripples(sourceTime))
                state.ripples << QVariantMap{{QStringLiteral("x"), ripple.x}, {QStringLiteral("y"), ripple.y},
                                             {QStringLiteral("progress"), ripple.progress}};
        }
        state.notifyChanged();
        stageTimer.restart();
        frameSource->setImage(packet.image);
        if (cameraEnabled && !packet.cameraImage.isNull())
            cameraFrameSource->setImage(packet.cameraImage);
        QCoreApplication::processEvents();
        uploadNs += stageTimer.nsecsElapsed();
        QImage rendered;
        QByteArray bytes;
        if (softwareRendering) {
            window->show();
            window->requestUpdate();
            QCoreApplication::processEvents();
            rendered = window->grabWindow();
            rendered = rendered.convertToFormat(QImage::Format_RGBA8888);
            if (rendered.size() != QSize(width, height)) {
                pipelineError = QStringLiteral("Could not grab the rendered frame");
                break;
            }
            bytes = QByteArray(reinterpret_cast<const char *>(rendered.constBits()),
                               rendered.sizeInBytes());
        } else {
            fbo->bind();
            context->functions()->glViewport(0, 0, width, height);
            context->functions()->glClearColor(0, 0, 0, 0);
            context->functions()->glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT);
            stageTimer.restart();
            renderControl->beginFrame();
            renderControl->polishItems();
            renderNs += stageTimer.nsecsElapsed();
            stageTimer.restart();
            renderControl->sync();
            uploadNs += stageTimer.nsecsElapsed();
            stageTimer.restart();
            renderControl->render();
            renderControl->endFrame();
            renderNs += stageTimer.nsecsElapsed();
            stageTimer.restart();
            auto &slot = readbacks[frame % int(readbacks.size())];
            if (slot.frame >= 0) bytes = mapReadback(slot);
            if (!pipelineError.isEmpty()) break;
            glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, slot.buffer);
            glExtra->glReadPixels(0, 0, width, height, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
            slot.frame = frame;
            glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
            readbackNs += stageTimer.nsecsElapsed();
        }
        if (!bytes.isEmpty()) writeFrame(bytes);
        if (!pipelineError.isEmpty()) break;
        emit progress(frame + 1, totalFrames);
    }
    finishDecoder();
    if (pipelineError.isEmpty() && !softwareRendering) {
        const int firstPending = std::max(0, totalFrames - int(readbacks.size()));
        for (int frame = firstPending; frame < totalFrames; ++frame) {
            auto &slot = readbacks[frame % int(readbacks.size())];
            if (slot.frame == frame) writeFrame(mapReadback(slot));
            if (!pipelineError.isEmpty()) break;
        }
    }
    if (!softwareRendering) {
        glExtra->glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
        for (auto &slot : readbacks)
            if (slot.buffer) glExtra->glDeleteBuffers(1, &slot.buffer);
    }
    if (!pipelineError.isEmpty()) {
        finishEncoder(true);
        if (error) *error = pipelineError;
        return false;
    }
    if (renderControl) renderControl->invalidate();
    if (!gif) {
        finishEncoder(false);
        if (!encodeError.isEmpty()) {
            if (error) *error = encodeError;
            return false;
        }
        if (QFileInfo::exists(options.outputPath) && !QFile::remove(options.outputPath)) {
            if (error) *error = QStringLiteral("Could not replace existing output %1").arg(options.outputPath);
            return false;
        }
        if (!QFile::rename(encodedPath, options.outputPath)) {
            if (error) *error = QStringLiteral("Could not finalize output %1").arg(options.outputPath);
            return false;
        }
        // QTemporaryFile creates 0600 files; exports are meant to be shared, so give
        // them the ordinary umask-style permissions a freshly written file would have.
        QFile::setPermissions(options.outputPath, QFile::ReadOwner | QFile::WriteOwner
                              | QFile::ReadGroup | QFile::ReadOther);
    } else {
        rawFile.flush();
        QStringList args{QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("rawvideo"), QStringLiteral("-pixel_format"), QStringLiteral("rgba"),
            QStringLiteral("-video_size"), QStringLiteral("%1x%2").arg(width).arg(height),
            QStringLiteral("-framerate"), QString::number(fps), QStringLiteral("-i"), rawFile.fileName()};
        const QString quality = normalizedQuality(options.quality.isEmpty() ? project.exportSettings.gif.quality : options.quality);
        const QString flip = softwareRendering ? QString() : QStringLiteral("vflip,");
        if (quality == QLatin1String("studio"))
            args << QStringLiteral("-vf") << flip + QStringLiteral("split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle");
        else if (!flip.isEmpty()) args << QStringLiteral("-vf") << QStringLiteral("vflip");
        args << QStringLiteral("-loop") << (project.exportSettings.gif.loop ? QStringLiteral("0") : QStringLiteral("-1"))
             << options.outputPath;
        QProcess gifProcess;
        gifProcess.start(QStringLiteral("ffmpeg"), args);
        if (!gifProcess.waitForFinished(120000) || gifProcess.exitCode() != 0) {
            if (error) *error = QString::fromUtf8(gifProcess.readAllStandardError()).trimmed();
            return false;
        }
        if (!QStandardPaths::findExecutable(QStringLiteral("gifsicle")).isEmpty()) {
            QProcess optimize;
            optimize.start(QStringLiteral("gifsicle"), {QStringLiteral("--batch"), QStringLiteral("--optimize=3"),
                QStringLiteral("--lossy=30"), options.outputPath});
            optimize.waitForFinished(120000);
        }
    }
    if (options.timing) {
        const double divisor = std::max(1, totalFrames) * 1000000.0;
        const double seconds = totalTimer.elapsed() / 1000.0;
        QTextStream(stderr)
            << "\nTiming (" << totalFrames << " frames):\n"
            << "  decode wait:  " << QString::number(decodeNs / divisor, 'f', 3) << " ms/frame\n"
            << "  upload:       " << QString::number(uploadNs / divisor, 'f', 3) << " ms/frame\n"
            << "  render:       " << QString::number(renderNs / divisor, 'f', 3) << " ms/frame\n"
            << "  readback:     " << QString::number(readbackNs / divisor, 'f', 3) << " ms/frame\n"
            << "  encode write: " << QString::number(encodeWriteNs / divisor, 'f', 3) << " ms/frame\n"
            << "  total:        " << QString::number(seconds, 'f', 3) << " s ("
            << QString::number(totalFrames / std::max(0.001, seconds), 'f', 1) << " frames/s)\n";
    }
    if (error) error->clear();
    return true;
}
