#include "Exporter.h"

#include "ExportRenderer.h"
#include "omareel_qt/src/theme.cxxqt.h"

#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QQuickWindow>
#include <QTextStream>
#include <QUrl>
#include <cstdint>
#include <utility>

using namespace Omareel;

QSize Omareel::paddedEvenSize(int width, int height)
{
    const omareel::exporter::EvenSize size = omareel::exporter::paddedEvenSize(width, height);
    return {size.width, size.height};
}

Exporter::Exporter(QObject *parent): QObject(parent), m_cancel(omareel::exporter::newCancelFlag()) {}

Exporter::~Exporter() = default;

void Exporter::cancel()
{
    m_cancel->cancel();
}

namespace {

// The project as Composition.qml reads it: its JSON as a map, the colours keystroke pills are
// drawn with, and the background image the job resolved, as a URL.
QVariantMap projectMap(const omareel::exporter::ExportJob &job)
{
    QVariantMap map = job.projectMap();
    // A Theme built here used to set the application font as it loaded; the composition's
    // text without a theme of its own falls back to that font.
    ThemeFonts::setApplicationFont(ThemeFonts::interfaceFamily(), omareel::theme::bodyTextSize());
    map[QStringLiteral("renderTheme")] = QVariantMap{
        {QStringLiteral("background"), omareel::theme::renderBackground()},
        {QStringLiteral("foreground"), omareel::theme::renderForeground()}};
    QVariantMap background = map.value(QStringLiteral("background")).toMap();
    const QString image = job.backgroundImage();
    background[QStringLiteral("resolvedImage")] = image.isEmpty() ? QString() : QUrl::fromLocalFile(image).toString();
    map[QStringLiteral("background")] = background;
    return map;
}

// A decoded frame as a QImage over the job's pixels, which it keeps alive until its last copy
// goes; a null frame is a null image.
QImage frameImage(::rust::Box<omareel::exporter::DecodedImage> frame)
{
    using Owner = ::rust::Box<omareel::exporter::DecodedImage>;
    if (frame->isNull()) return QImage();
    auto *owner = new Owner(std::move(frame));
    const ::rust::Slice<const std::uint8_t> pixels = (*owner)->pixels();
    return QImage(pixels.data(), (*owner)->width(), (*owner)->height(), qsizetype((*owner)->width()) * 4,
                  QImage::Format_RGBA8888, [](void *info) { delete static_cast<Owner *>(info); }, owner);
}

// The members the frame loop changes, from the job's state after nextFrame.
void applyFrame(CompositionState &state, const omareel::exporter::ExportJob &job)
{
    const omareel::exporter::Composition composition = job.composition();
    state.time = composition.time;
    state.zoom = job.zoomMap();
    state.cursor = job.cursorMap();
    state.ripples = job.ripples();
    state.keystrokePills = job.keystrokePills();
    state.layout = job.layoutMap();
    state.cameraVisible = composition.camera_visible;
    state.media = job.mediaMap();
}

} // namespace

void Exporter::exportBundle(const ExportOptions &options)
{
    m_cancel->reset();
    QString error;
    if (run(options, &error)) emit finished(QFileInfo(options.outputPath).absoluteFilePath());
    else emit failed(error.isEmpty() ? QStringLiteral("Export failed") : error);
}

bool Exporter::run(const ExportOptions &options, QString *error)
{
    ::rust::Box<omareel::exporter::ExportJob> job = omareel::exporter::newExportJob();
    const omareel::exporter::ExportOptions jobOptions{options.bundlePath, options.outputPath, options.fps,
        options.width, options.quality, options.gifFps, options.gifWidth, options.timing};
    if (!job->prepare(jobOptions, *m_cancel, *error)) return false;
    const omareel::exporter::Composition composition = job->composition();
    const int totalFrames = job->totalFrames();

    const bool softwareRendering = QQuickWindow::graphicsApi() == QSGRendererInterface::Software
                                || QGuiApplication::platformName() == QLatin1String("offscreen");
    ExportRenderer renderer;
    // Composition.qml reads the state as it loads.
    CompositionState &state = renderer.state();
    state.outputWidth = composition.output_width; state.outputHeight = composition.output_height;
    state.softwareRendering = softwareRendering;
    state.sourceWidth = composition.source_width; state.sourceHeight = composition.source_height;
    state.captureScale = composition.capture_scale;
    state.cameraAvailable = composition.camera_available;
    state.cameraSourceWidth = composition.camera_source_width;
    state.cameraSourceHeight = composition.camera_source_height;
    state.project = projectMap(*job);
    state.zoom = job->zoomMap();
    state.layout = job->layoutMap();
    state.media = job->mediaMap();
    state.cursor = job->cursorMap();
    if (!renderer.init(composition.output_width, composition.output_height, softwareRendering, error)) return false;
    if (!job->start(softwareRendering, *error)) return false;

    QString pipelineError;
    const auto writeFrame = [&](QByteArray bytes) {
        // Export runs on the GUI thread. Keep Cancel and the status responsive while the
        // encoder catches up: two queued frames, one in the pipe, and two GPU readbacks bound
        // how far rendering can run ahead.
        while (!job->waitEncodeSpace(20)) renderer.processEvents();
        const QString writeError = job->encode(std::move(bytes));
        if (!writeError.isEmpty()) pipelineError = writeError;
    };
    for (int frame = 0; frame < totalFrames; ++frame) {
        if (!job->nextFrame(frame, pipelineError)) break;
        applyFrame(state, *job);
        QByteArray bytes;
        if (!renderer.renderFrame(frameImage(job->screenImage()), frameImage(job->cameraImage()),
                                  frameImage(job->insertImage()), &bytes, &pipelineError))
            break;
        if (!bytes.isEmpty()) writeFrame(std::move(bytes));
        if (!pipelineError.isEmpty()) break;
        emit progress(frame + 1, totalFrames);
    }
    job->finishDecoder();
    if (pipelineError.isEmpty()) {
        QByteArray bytes;
        while (renderer.takePendingFrame(&bytes, &pipelineError)) {
            writeFrame(std::move(bytes));
            if (!pipelineError.isEmpty()) break;
        }
    }
    renderer.releaseReadbacks();
    const auto finishEncoder = [&](bool abort) {
        job->closeEncoder(abort);
        // Keep Cancel and the finishing status responsive while the worker drains the last
        // frames and finalizes MP4.
        while (!job->encoderDone()) {
            renderer.processEvents();
            job->waitEncoderDone(20);
        }
        job->joinEncoder();
    };
    if (!pipelineError.isEmpty()) {
        finishEncoder(true);
        *error = pipelineError;
        return false;
    }
    renderer.invalidate();
    finishEncoder(false);
    if (!job->finish(*error)) return false;
    if (options.timing) {
        const ExportRenderer::Timing &timing = renderer.timing();
        QTextStream(stderr) << job->timingReport(timing.uploadNs, timing.renderNs, timing.readbackNs);
    }
    error->clear();
    return true;
}
