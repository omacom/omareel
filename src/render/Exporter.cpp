#include "Exporter.h"

#include "FfmpegDecoder.h"
#include "FrameSource.h"
#include "core/ClipTimeline.h"
#include "core/InputLog.h"
#include "core/MotionTrack.h"
#include "core/Project.h"
#include "core/ZoomTimeline.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QOffscreenSurface>
#include <QOpenGLContext>
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
#include <cmath>

using namespace OmaRecord;

struct MediaInfo { int width = 0; int height = 0; double duration = 0.0; bool audio = false; };

static MediaInfo probe(const QString &path, QString *error)
{
    QProcess process;
    process.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-show_entries"), QStringLiteral("stream=codec_type,width,height:format=duration"),
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
        } else if (stream.value("codec_type") == QLatin1String("audio")) result.audio = true;
    }
    if (result.width <= 0 || result.height <= 0 || result.duration <= 0.0) {
        if (error) *error = QStringLiteral("ffprobe returned invalid video metadata");
    }
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

static QString wallpaperPath()
{
    QDir directory(QDir::homePath() + QStringLiteral("/.local/state/omarchy/current/theme/backgrounds"));
    const auto files = directory.entryInfoList({QStringLiteral("*.png"), QStringLiteral("*.jpg"),
        QStringLiteral("*.jpeg"), QStringLiteral("*.webp")}, QDir::Files, QDir::Name);
    return files.isEmpty() ? QString() : files.first().absoluteFilePath();
}

static QVariantMap projectMap(const Project &project)
{
    QVariantMap map = project.toJson().toVariantMap();
    QVariantMap background = map.value(QStringLiteral("background")).toMap();
    QString image;
    if (project.background.type == QLatin1String("image")) image = project.background.image;
    else if (project.background.type == QLatin1String("wallpaper"))
        image = project.background.wallpaper == QLatin1String("omarchy:current")
            ? wallpaperPath() : project.background.wallpaper;
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
        QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("color=size=64x64:rate=1"),
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
    const QDir bundle(options.bundlePath);
    const QString videoPath = bundle.filePath(QStringLiteral("screen.mp4"));
    const MediaInfo media = probe(videoPath, error);
    if (media.width <= 0) return false;

    Project project;
    const QString projectPath = bundle.filePath(QStringLiteral("project.json"));
    if (QFileInfo(projectPath).isFile()) {
        project = Project::load(projectPath, error);
        if (error && !error->isEmpty()) return false;
    } else {
        project = Project::defaults(bundle.dirName(), media.duration);
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
    QFile capture(bundle.filePath(QStringLiteral("capture.json")));
    if (capture.open(QIODevice::ReadOnly)) captureFps = QJsonDocument::fromJson(capture.readAll()).object().value("fps").toDouble(60.0);
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
    width += width % 2;
    height += height % 2;
    const int totalFrames = std::max(1, int(std::ceil(outputDuration * fps)));

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
        window->setRenderTarget(QQuickRenderTarget::fromOpenGLTexture(fbo->texture(), {width, height}));
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

    QString runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    if (runtime.isEmpty() || !QFileInfo(runtime).isWritable()) runtime = QDir::tempPath();
    QTemporaryFile rawFile(QDir(runtime).filePath(QStringLiteral("omarecord-export-XXXXXX.rgba")));
    QProcess encoder;
    bool useAudio = !gif && media.audio && (project.audio.desktop || project.audio.mic);
    if (gif) {
        if (!rawFile.open()) { if (error) *error = rawFile.errorString(); return false; }
    } else {
        QStringList args{QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("rawvideo"), QStringLiteral("-pix_fmt"), QStringLiteral("rgba"),
            QStringLiteral("-s:v"), QStringLiteral("%1x%2").arg(width).arg(height),
            QStringLiteral("-r"), QString::number(fps), QStringLiteral("-i"), QStringLiteral("-")};
        if (useAudio) args << QStringLiteral("-i") << videoPath
                           << QStringLiteral("-filter_complex") << audioFilter(project.clips, project.audio.volume)
                           << QStringLiteral("-map") << QStringLiteral("0:v:0")
                           << QStringLiteral("-map") << QStringLiteral("[aout]")
                           << QStringLiteral("-c:a") << QStringLiteral("aac");
        else args << QStringLiteral("-an");
        const QString quality = normalizedQuality(options.quality.isEmpty() ? project.exportSettings.quality : options.quality);
        const qint64 bitrate = qint64(std::floor(width * double(height) * fps * bitrateMultiplier(quality)));
        if (nvencWorks()) {
            args << QStringLiteral("-c:v") << QStringLiteral("h264_nvenc")
                 << QStringLiteral("-preset") << QStringLiteral("p5")
                 << QStringLiteral("-b:v") << QString::number(bitrate)
                 << QStringLiteral("-maxrate") << QString::number(qint64(bitrate * 1.2))
                 << QStringLiteral("-bufsize") << QString::number(bitrate * 2)
                 << QStringLiteral("-profile:v") << QStringLiteral("high");
        } else {
            args << QStringLiteral("-c:v") << QStringLiteral("libx264")
                 << QStringLiteral("-preset") << QStringLiteral("medium")
                 << QStringLiteral("-b:v") << QString::number(bitrate)
                 << QStringLiteral("-maxrate") << QString::number(qint64(bitrate * 1.2))
                 << QStringLiteral("-bufsize") << QString::number(bitrate * 2)
                 << QStringLiteral("-profile:v") << QStringLiteral("high");
        }
        args << QStringLiteral("-pix_fmt") << QStringLiteral("yuv420p")
             << QStringLiteral("-movflags") << QStringLiteral("+faststart") << options.outputPath;
        encoder.start(QStringLiteral("ffmpeg"), args, QIODevice::WriteOnly);
        if (!encoder.waitForStarted(10000)) { if (error) *error = encoder.errorString(); return false; }
    }

    FfmpegDecoder decoder;
    int activeClip = -1;
    QImage decoded;
    for (int frame = 0; frame < totalFrames; ++frame) {
        if (m_cancelled) { decoder.cancel(); encoder.kill(); if (error) *error = QStringLiteral("Export cancelled"); return false; }
        const double outputTime = frame / double(fps);
        double clipOutputStart = 0.0;
        int clipIndex = project.clips.size() - 1;
        for (int i = 0; i < project.clips.size(); ++i) {
            const double length = (project.clips[i].out - project.clips[i].in) / project.clips[i].speed;
            if (outputTime < clipOutputStart + length || i == project.clips.size() - 1) { clipIndex = i; break; }
            clipOutputStart += length;
        }
        const auto &clip = project.clips[clipIndex];
        if (clipIndex != activeClip) {
            if (!decoder.start(videoPath, clip.in, clip.out - clip.in, fps / clip.speed,
                               media.width, media.height, error)) return false;
            activeClip = clipIndex;
        }
        QImage next;
        QString decodeError;
        if (decoder.readFrame(&next, &decodeError)) decoded = next;
        else if (decoded.isNull()) { if (error) *error = decodeError.isEmpty() ? QStringLiteral("Decoder ended before the first frame") : decodeError; return false; }

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
        frameSource->setImage(decoded);
        QCoreApplication::processEvents();
        QImage rendered;
        if (softwareRendering) {
            window->show();
            window->requestUpdate();
            QCoreApplication::processEvents();
            rendered = window->grabWindow();
        } else {
            fbo->bind();
            context->functions()->glViewport(0, 0, width, height);
            context->functions()->glClearColor(0, 0, 0, 0);
            context->functions()->glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT);
            renderControl->beginFrame();
            renderControl->polishItems();
            renderControl->sync();
            renderControl->render();
            renderControl->endFrame();
            context->functions()->glFinish();
            rendered = fbo->toImage();
        }
        rendered = rendered.convertToFormat(QImage::Format_RGBA8888);
        if (rendered.size() != QSize(width, height)) {
            if (error) *error = QStringLiteral("Could not grab the rendered frame");
            return false;
        }
        const QByteArray bytes(reinterpret_cast<const char *>(rendered.constBits()), rendered.sizeInBytes());
        if (gif) {
            if (rawFile.write(bytes) != bytes.size()) { if (error) *error = rawFile.errorString(); return false; }
        } else {
            qint64 written = 0;
            while (written < bytes.size()) {
                const qint64 amount = encoder.write(bytes.constData() + written, bytes.size() - written);
                if (amount < 0 || !encoder.waitForBytesWritten(30000)) {
                    if (error) *error = QString::fromUtf8(encoder.readAllStandardError()).trimmed();
                    return false;
                }
                written += amount;
            }
        }
        emit progress(frame + 1, totalFrames);
    }
    decoder.cancel();
    if (renderControl) renderControl->invalidate();
    if (!gif) {
        encoder.closeWriteChannel();
        if (!encoder.waitForFinished(120000) || encoder.exitCode() != 0) {
            if (error) *error = QString::fromUtf8(encoder.readAllStandardError()).trimmed();
            return false;
        }
    } else {
        rawFile.flush();
        QStringList args{QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("rawvideo"), QStringLiteral("-pixel_format"), QStringLiteral("rgba"),
            QStringLiteral("-video_size"), QStringLiteral("%1x%2").arg(width).arg(height),
            QStringLiteral("-framerate"), QString::number(fps), QStringLiteral("-i"), rawFile.fileName()};
        const QString quality = normalizedQuality(options.quality.isEmpty() ? project.exportSettings.gif.quality : options.quality);
        if (quality == QLatin1String("studio"))
            args << QStringLiteral("-vf") << QStringLiteral("split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle");
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
    if (error) error->clear();
    return true;
}
