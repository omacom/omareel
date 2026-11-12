#include "Exporter.h"

#include "ExportRenderer.h"
#include "omareel_qt/src/theme.cxxqt.h"

#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QPainter>
#include <QPainterPath>
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
// goes; a null frame is a null image. `opaque` frames (the recording and the webcam: h264 through
// ffmpeg's RGBA, alpha 255 everywhere) are tagged premultiplied, which for them is the same
// bytes: the scene graph uploads that format as is, where a straight RGBA frame is converted
// first, a copy of the whole frame on Qt's thread pool every frame (8 threads busy at 4K).
QImage frameImage(::rust::Box<omareel::exporter::DecodedImage> frame, bool opaque = false)
{
    using Owner = ::rust::Box<omareel::exporter::DecodedImage>;
    if (frame->isNull()) return QImage();
    auto *owner = new Owner(std::move(frame));
    const ::rust::Slice<const std::uint8_t> pixels = (*owner)->pixels();
    return QImage(pixels.data(), (*owner)->width(), (*owner)->height(), qsizetype((*owner)->width()) * 4,
                  opaque ? QImage::Format_RGBA8888_Premultiplied : QImage::Format_RGBA8888, [](void *info) { delete static_cast<Owner *>(info); }, owner);
}

// A decoded YUV frame (the recording, see ExportJob::setYuvScreen) for FrameSource, which keeps
// the job's pixels alive until it has the next one.
YuvFrame yuvFrame(::rust::Box<omareel::exporter::DecodedImage> frame)
{
    using Owner = ::rust::Box<omareel::exporter::DecodedImage>;
    auto owner = std::shared_ptr<Owner>(new Owner(std::move(frame)));
    YuvFrame result;
    result.planes = (*owner)->pixels().data();
    result.width = (*owner)->width();
    result.height = (*owner)->height();
    result.bt709 = (*owner)->yuvBt709();
    result.fullRange = (*owner)->yuvFullRange();
    result.owner = std::shared_ptr<const void>(owner, owner.get());
    return result;
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

bool isSoftwareRendering()
{
    return QQuickWindow::graphicsApi() == QSGRendererInterface::Software
        || QGuiApplication::platformName() == QLatin1String("offscreen");
}

// Loads the composition against the prepared job's state, then starts the job.
bool startRendering(omareel::exporter::ExportJob &job, ExportRenderer &renderer, bool softwareRendering, QString *error)
{
    const omareel::exporter::Composition composition = job.composition();
    // Composition.qml reads the state as it loads.
    CompositionState &state = renderer.state();
    state.outputWidth = composition.output_width; state.outputHeight = composition.output_height;
    state.softwareRendering = softwareRendering;
    state.sourceWidth = composition.source_width; state.sourceHeight = composition.source_height;
    state.captureScale = composition.capture_scale;
    state.cameraAvailable = composition.camera_available;
    state.cameraSourceWidth = composition.camera_source_width;
    state.cameraSourceHeight = composition.camera_source_height;
    state.project = projectMap(job);
    state.zoom = job.zoomMap();
    state.layout = job.layoutMap();
    state.media = job.mediaMap();
    state.cursor = job.cursorMap();
    if (!renderer.init(composition.output_width, composition.output_height, softwareRendering, error)) return false;
    // On the GPU, the recording comes as YUV and FrameSource converts it: a third of the bytes
    // through the decoder's pipe and to the GPU, and no RGB conversion in ffmpeg (a 4K frame was
    // 33 MB of RGBA, which held the whole export to the decoder's pace).
    // OMAREEL_EXPORT_RGBA=1 keeps the RGBA path, for comparing the two.
    job.setYuvScreen(!softwareRendering && qgetenv("OMAREEL_EXPORT_RGBA") != "1");
    return job.start(softwareRendering, *error);
}

// Hands the job's current pictures to the renderer and renders the frame.
bool renderJobFrame(omareel::exporter::ExportJob &job, ExportRenderer &renderer, QByteArray *bytes, QString *error)
{
    ::rust::Box<omareel::exporter::DecodedImage> screen = job.screenImage();
    QImage screenImage;
    if (!screen->isNull() && screen->isYuv()) renderer.setScreenYuv(yuvFrame(std::move(screen)));
    else screenImage = frameImage(std::move(screen), true);
    return renderer.renderFrame(screenImage, frameImage(job.cameraImage(), true), frameImage(job.insertImage()), bytes, error);
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
    const int totalFrames = job->totalFrames();

    const bool softwareRendering = isSoftwareRendering();
    ExportRenderer renderer;
    CompositionState &state = renderer.state();
    if (!startRendering(*job, renderer, softwareRendering, error)) return false;

    QString pipelineError;
    const auto writeFrame = [&](QByteArray bytes) {
        // Export runs on the GUI thread. Keep Cancel and the status responsive while the
        // encoder catches up: the encoder's queue (ENCODE_QUEUE), one frame in the pipe and the
        // GPU readbacks bound how far rendering can run ahead.
        while (!job->waitEncodeSpace(20)) renderer.processEvents();
        const QString writeError = job->encode(std::move(bytes));
        if (!writeError.isEmpty()) pipelineError = writeError;
    };
    for (int frame = 0; frame < totalFrames; ++frame) {
        if (!job->nextFrame(frame, pipelineError)) break;
        applyFrame(state, *job);
        QByteArray bytes;
        if (!renderJobFrame(*job, renderer, &bytes, &pipelineError)) break;
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

bool Exporter::renderStills(const StillOptions &options, QList<Still> *stills, double *duration, QString *error)
{
    m_cancel->reset();
    stills->clear();
    // With nothing asked for, the job would not be in still mode: it would export.
    if (options.times.isEmpty() && options.count <= 0) {
        *error = QStringLiteral("No frames asked for");
        return false;
    }
    ::rust::Box<omareel::exporter::ExportJob> job = omareel::exporter::newExportJob();
    job->setStills(::rust::Slice<const double>(options.times.constData(), size_t(options.times.size())), options.count);
    // The output's name only tells an export MP4 from GIF; stills are neither.
    const omareel::exporter::ExportOptions jobOptions{options.bundlePath, QStringLiteral("still.png"), options.fps,
        options.width, QString(), 0, 0, false};
    if (!job->prepare(jobOptions, *m_cancel, *error)) return false;
    if (duration) *duration = job->outputDuration();
    const bool softwareRendering = isSoftwareRendering();
    ExportRenderer renderer;
    CompositionState &state = renderer.state();
    if (!startRendering(*job, renderer, softwareRendering, error)) return false;
    const omareel::exporter::Composition composition = job->composition();
    const int width = composition.output_width;
    const int height = composition.output_height;
    const ::rust::Vec<double> times = job->stillTimes();
    const int count = job->totalFrames();
    bool ok = true;
    for (int still = 0; still < count && ok; ++still) {
        if (!job->nextFrame(still, *error)) { ok = false; break; }
        applyFrame(state, *job);
        // The first still renders twice: the export's frame came after others, so anything the
        // scene graph sets up on its first frame (a gradient background's canvas) has settled.
        // Without new pictures the second render keeps the first's.
        QByteArray bytes;
        for (int pass = 0; pass < (still == 0 ? 2 : 1) && ok; ++pass) {
            QByteArray rendered;
            ok = pass == 0 ? renderJobFrame(*job, renderer, &rendered, error)
                           : renderer.renderFrame(QImage(), QImage(), QImage(), &rendered, error);
            // The software path returns the frame at once; OpenGL's is read back below.
            if (!rendered.isEmpty()) bytes = std::move(rendered);
            while (ok && renderer.takePendingFrame(&rendered, error)) {
                if (!error->isEmpty()) ok = false;
                else bytes = std::move(rendered);
            }
        }
        if (!ok) break;
        if (bytes.size() != qsizetype(width) * height * 4) {
            *error = QStringLiteral("Could not read the rendered frame");
            ok = false;
            break;
        }
        // As the encoder reads the frame: RGB with the alpha ignored, bottom-up from OpenGL.
        const QImage frame(reinterpret_cast<const uchar *>(bytes.constData()), width, height, qsizetype(width) * 4,
                           QImage::Format_RGBX8888);
        QImage image = softwareRendering ? frame.convertToFormat(QImage::Format_RGB888)
                                         : frame.flipped(Qt::Vertical).convertToFormat(QImage::Format_RGB888);
        Still rendered{std::move(image), times[size_t(still)]};
        if (options.take) {
            if (!options.take(still, rendered, error)) { ok = false; break; }
        } else {
            stills->append(std::move(rendered));
        }
        emit progress(still + 1, count);
    }
    job->finishDecoder();
    renderer.releaseReadbacks();
    if (ok) renderer.invalidate();
    return ok;
}

QImage Exporter::contactSheet(const QList<Still> &stills, int columns, int gap)
{
    if (stills.isEmpty() || columns <= 0) return QImage();
    const QSize tile = stills.first().image.size();
    const int rows = int((stills.size() + columns - 1) / columns);
    QImage sheet(columns * tile.width() + (columns + 1) * gap, rows * tile.height() + (rows + 1) * gap,
                 QImage::Format_RGB888);
    sheet.fill(QColor(0x16, 0x16, 0x16));
    QPainter painter(&sheet);
    painter.setRenderHint(QPainter::Antialiasing);
    painter.setRenderHint(QPainter::TextAntialiasing);
    QFont font = QGuiApplication::font();
    font.setPixelSize(qBound(11, tile.height() / 12, 28));
    font.setWeight(QFont::DemiBold);
    painter.setFont(font);
    const QFontMetrics metrics(font);
    for (qsizetype i = 0; i < stills.size(); ++i) {
        const QPoint origin(gap + int(i % columns) * (tile.width() + gap), gap + int(i / columns) * (tile.height() + gap));
        painter.drawImage(origin, stills[i].image);
        const QString label = QString(omareel::exporter::stillTimeLabel(stills[i].time));
        const int padding = qMax(3, metrics.height() / 4);
        const QRect box(origin.x() + padding, origin.y() + tile.height() - padding - metrics.height() - padding,
                        metrics.horizontalAdvance(label) + 2 * padding, metrics.height() + padding);
        painter.setPen(Qt::NoPen);
        painter.setBrush(QColor(0, 0, 0, 180));
        painter.drawRoundedRect(box, padding, padding);
        painter.setPen(Qt::white);
        painter.drawText(box, Qt::AlignCenter, label);
    }
    return sheet;
}
