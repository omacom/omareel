// screencapture: the parts of src/record/ScreenCapture.cpp that need no compositor. The tag's file
// is included whole, so CaptureRingBookkeeping, captureCropRows, the Matroska headers, the encoder
// probe and its cache (encoderProbeKey, encoderProbeResults, cachedPreferredEncoderAvailable, with
// ffmpeg answered by the replay shim) run as the tag has them, and ScreenCapture::Private's
// writerLoop is driven directly: frames queued into its ring as captureFrame queues them, its
// output written to a file instead of the encoder's pipe. The crop trim and the encoder arguments
// inside ScreenCapture::start, which needs a Wayland session, are copied below with their lines.
// tests/golden/README.md, screencapture.

#include "record/ScreenCapture.cpp"

#include "common.h"
#include "recording.h"
#include "replay.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonObject>

#include <chrono>
#include <fcntl.h>
#include <sys/stat.h>
#include <thread>

namespace Oracle {

namespace {

using Omareel::CaptureRingBookkeeping;
using Omareel::CaptureRowCopy;
using Omareel::ScreenCapture;
using Omareel::ScreenCaptureConfig;
using Private = ScreenCapture::Private;

QByteArray ringStates(const CaptureRingBookkeeping &ring, int slotTotal)
{
    QByteArray text;
    for (int slot = 0; slot < slotTotal; ++slot) {
        switch (ring.state(slot)) {
        case CaptureRingBookkeeping::State::Available: text += 'A'; break;
        case CaptureRingBookkeeping::State::Capturing: text += 'C'; break;
        case CaptureRingBookkeeping::State::Queued: text += 'Q'; break;
        case CaptureRingBookkeeping::State::Writing: text += 'W'; break;
        }
    }
    return text;
}

QRect rectFrom(const QJsonValue &value)
{
    const QJsonArray r = value.toArray();
    return QRect(r.at(0).toInt(), r.at(1).toInt(), r.at(2).toInt(), r.at(3).toInt());
}

QSize sizeFrom(const QJsonValue &value)
{
    return QSize(value.toArray().at(0).toInt(), value.toArray().at(1).toInt());
}

QByteArray rectText(const QRect &rect)
{
    return QByteArray::number(rect.x()) + ',' + QByteArray::number(rect.y()) + ' ' + QByteArray::number(rect.width())
        + 'x' + QByteArray::number(rect.height());
}

QByteArray sizeText(const QSize &size)
{
    return QByteArray::number(size.width()) + 'x' + QByteArray::number(size.height());
}

QByteArray rowText(const CaptureRowCopy &row)
{
    return QByteArray::number(row.sourceOffset) + ':' + QByteArray::number(row.destinationOffset) + ':'
        + QByteArray::number(row.bytes);
}

void captureRings(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject rings = cases.value(QStringLiteral("rings")).toObject();
    for (auto it = rings.begin(); it != rings.end(); ++it) {
        const int slotTotal = it.value().toArray().at(0).toInt();
        CaptureRingBookkeeping ring(slotTotal);
        const int shown = std::max(1, slotTotal) + 1;
        out << "== " << it.key().toUtf8() << " slotCount=" << slotTotal << "\n";
        for (const QJsonValue &opValue : it.value().toArray().at(1).toArray()) {
            const QJsonArray op = opValue.toArray();
            const QString name = op.at(0).toString();
            const int slot = op.at(1).toInt();
            QByteArray result;
            if (name == QLatin1String("acquire")) result = QByteArray::number(ring.acquire());
            else if (name == QLatin1String("markQueued")) result = boolText(ring.markQueued(slot));
            else if (name == QLatin1String("takeQueued")) result = QByteArray::number(ring.takeQueued());
            else if (name == QLatin1String("release")) result = boolText(ring.release(slot));
            else if (name == QLatin1String("state")) result = ringStates(ring, slot + 1).right(1);
            else fail(QStringLiteral("unknown ring operation %1").arg(name));
            out.row({name.toUtf8(), op.size() > 1 ? QByteArray::number(slot) : QByteArray("-"), result,
                     ringStates(ring, shown), QByteArray::number(ring.drops()),
                     QByteArray::number(ring.count(CaptureRingBookkeeping::State::Available)) + ' '
                         + QByteArray::number(ring.count(CaptureRingBookkeeping::State::Capturing)) + ' '
                         + QByteArray::number(ring.count(CaptureRingBookkeeping::State::Queued)) + ' '
                         + QByteArray::number(ring.count(CaptureRingBookkeeping::State::Writing))});
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("capture-ring.txt")), out.data());
}

void captureCrops(const Context &context, const QJsonObject &cases)
{
    Text out;
    for (const QJsonValue &value : cases.value(QStringLiteral("crops")).toArray()) {
        const QJsonObject input = value.toObject();
        const QSize size = sizeFrom(input.value(QStringLiteral("size")));
        const int stride = input.value(QStringLiteral("stride")).toInt();
        const QRect crop = rectFrom(input.value(QStringLiteral("crop")));
        const QVector<CaptureRowCopy> rows = Omareel::captureCropRows(size, stride, crop);
        QList<QByteArray> cells{sizeText(size), QByteArray::number(stride), rectText(crop),
                                QByteArray::number(rows.size())};
        qint64 sources = 0, destinations = 0, bytes = 0;
        for (const CaptureRowCopy &row : rows) {
            sources += row.sourceOffset;
            destinations += row.destinationOffset;
            bytes += row.bytes;
        }
        cells << "sums=" + QByteArray::number(sources) + ':' + QByteArray::number(destinations) + ':'
                     + QByteArray::number(bytes);
        for (qsizetype index = 0; index < rows.size(); ++index) {
            if (rows.size() > 8 && index >= 3 && index < rows.size() - 3) continue;
            cells << QByteArray::number(index) + '=' + rowText(rows.at(index));
        }
        out.row(cells);
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("capture-crop-rows.txt")), out.data());
}

void captureHeaders(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject streams = cases.value(QStringLiteral("streamHeaders")).toObject();
    for (const QJsonValue &size : streams.value(QStringLiteral("sizes")).toArray())
        for (const QJsonValue &fps : streams.value(QStringLiteral("fps")).toArray())
            out.row({"stream", sizeText(sizeFrom(size)), QByteArray::number(fps.toInt()),
                     Omareel::captureStreamHeader(sizeFrom(size), fps.toInt()).toHex()});
    const QJsonObject frames = cases.value(QStringLiteral("frameHeaders")).toObject();
    for (const QJsonValue &timestamp : frames.value(QStringLiteral("timestamps")).toArray())
        for (const QJsonValue &bytes : frames.value(QStringLiteral("bytes")).toArray())
            out.row({"frame", QByteArray::number(timestamp.toInteger()), QByteArray::number(bytes.toInteger()),
                     Omareel::captureFrameHeader(timestamp.toInteger(), qsizetype(bytes.toInteger())).toHex()});
    writeFile(QDir(context.output).filePath(QStringLiteral("matroska-headers.txt")), out.data());
}

// src/record/ScreenCapture.cpp:855-870 (in ScreenCapture::start, after the capture session
// reported the output's size), verbatim; the caller sets d->width, d->height and d->stride.
bool startCrop(Private *d, const ScreenCaptureConfig &config, QString *error)
{
    d->crop = config.crop.isValid() ? config.crop.intersected(QRect(0, 0, d->width, d->height))
                                    : QRect(0, 0, d->width, d->height);
    // The encoders reject odd frame sizes; a crop that lost a pixel to the output edge (or a whole
    // output of odd size) is trimmed rather than failing at the first frame.
    d->crop.setWidth(d->crop.width() - d->crop.width() % 2);
    d->crop.setHeight(d->crop.height() - d->crop.height() % 2);
    if (!d->crop.isValid()) {
        if (error) *error = QStringLiteral("The capture region is outside the monitor");
        return false;
    }
    d->encodedSize = d->crop.size();
    d->cropRows = captureCropRows(QSize(d->width, d->height), d->stride, d->crop);
    if (d->cropRows.isEmpty()) {
        if (error) *error = QStringLiteral("The capture crop could not be prepared");
        return false;
    }
    return true;
}

// src/record/ScreenCapture.cpp:811-812 (d->config), 898 and 903-962 (in ScreenCapture::start),
// verbatim: the encoder's arguments once the probes have answered.
QStringList startArguments(Private *d, const ScreenCaptureConfig &config, const EncoderProbeResults &probeResults)
{
    d->config = config;
    d->config.fps = std::max(1, config.fps);
    const bool preferred = probeResults.preferred;
    const QString requestedConversion = qEnvironmentVariable("OMAREEL_NATIVE_CONVERSION").toLower();
    const bool useGpuConversion = preferred && requestedConversion != QLatin1String("cpu")
        && probeResults.gpuConversion;
    d->conversion = useGpuConversion ? QStringLiteral("gpu") : QStringLiteral("cpu");
    QStringList arguments{QStringLiteral("-y"), QStringLiteral("-loglevel"), QStringLiteral("error"),
        QStringLiteral("-f"), QStringLiteral("matroska"), QStringLiteral("-i"),
        QStringLiteral("pipe:0"), QStringLiteral("-an")};
    if (useGpuConversion)
        arguments << QStringLiteral("-vf") << QStringLiteral("hwupload_cuda,scale_cuda=format=nv12");
    arguments << QStringLiteral("-c:v");
    // Quality: the capture is an intermediate that gets re-encoded on export, so keep it near
    // visually lossless. A plain -cq without a rate mode made NVENC starve small regions (a
    // 440x234 capture came out at ~110 kbit/s with visible macroblocks), so pin VBR with a
    // generous ceiling scaled to the frame size, plus a 1-second GOP for clean seeking.
    const double megapixels = d->encodedSize.width() * double(d->encodedSize.height()) / 1e6;
    const int maxrateKbps = int(std::clamp(megapixels * 12000.0 * (d->config.fps / 60.0), 8000.0, 120000.0));
    // Debug aid: OMAREEL_NATIVE_ENCODER=lossless stores the raw capture losslessly so capture
    // problems can be told apart from encoder artefacts.
    const bool lossless = qEnvironmentVariable("OMAREEL_NATIVE_ENCODER") == QLatin1String("lossless");
    // Small captures (regions, windows) are cheap enough to store losslessly, and NVENC's lossy
    // modes smear high-contrast edges into visible macroblocks at these sizes. Larger frames use
    // a high-quality VBR tier instead.
    const bool smallFrame = megapixels < 1.5;
    if (lossless)
        arguments << QStringLiteral("libx264rgb") << QStringLiteral("-preset") << QStringLiteral("ultrafast")
                  << QStringLiteral("-qp") << QStringLiteral("0");
    else if (preferred && smallFrame)
        arguments << QStringLiteral("h264_nvenc") << QStringLiteral("-preset") << QStringLiteral("p4")
                  << QStringLiteral("-tune") << QStringLiteral("lossless")
                  << QStringLiteral("-zerolatency") << QStringLiteral("1")
                  << QStringLiteral("-g") << QString::number(d->config.fps)
                  << QStringLiteral("-bf") << QStringLiteral("0");
    else if (preferred)
        arguments << QStringLiteral("h264_nvenc") << QStringLiteral("-preset") << QStringLiteral("p4")
                  << QStringLiteral("-tune") << QStringLiteral("hq")
                  << QStringLiteral("-zerolatency") << QStringLiteral("1")
                  << QStringLiteral("-rc") << QStringLiteral("vbr")
                  << QStringLiteral("-cq") << QStringLiteral("16")
                  << QStringLiteral("-b:v") << QStringLiteral("0")
                  << QStringLiteral("-maxrate") << QStringLiteral("%1k").arg(maxrateKbps)
                  << QStringLiteral("-bufsize") << QStringLiteral("%1k").arg(maxrateKbps * 2)
                  << QStringLiteral("-g") << QString::number(d->config.fps)
                  << QStringLiteral("-bf") << QStringLiteral("0");
    // Without NVENC, libx264 it is. h264_vulkan looks like the hardware alternative, but uploading
    // BGRA frames through hwupload held it below libx264 veryfast at every capture size on the N1X.
    else
        // -g is one second. Without zerolatency the encoder may hold that GOP and a
        // stop never writes it, so the raw recording ends about a second early.
        arguments << QStringLiteral("libx264") << QStringLiteral("-preset") << QStringLiteral("veryfast")
                  << QStringLiteral("-tune") << QStringLiteral("zerolatency")
                  << QStringLiteral("-crf") << (smallFrame ? QStringLiteral("10") : QStringLiteral("16"))
                  << QStringLiteral("-g") << QString::number(d->config.fps)
                  << QStringLiteral("-bf") << QStringLiteral("0");
    if (!useGpuConversion && !lossless) arguments << QStringLiteral("-pix_fmt") << QStringLiteral("yuv420p");
    // Frames keep the times they were captured at, on a 1/fps time base. Gaps stay gaps: ffmpeg
    // neither fills them with copies nor closes them up. (B-frames would also break the MP4
    // durations of such a stream, so none of the encoders above use them.)
    arguments << QStringLiteral("-fps_mode") << QStringLiteral("vfr")
              << QStringLiteral("-enc_time_base") << QStringLiteral("1/%1").arg(d->config.fps);
    arguments << QStringLiteral("-movflags") << QStringLiteral("+faststart") << config.outputPath;
    return arguments;
}

// src/record/ScreenCapture.cpp:1100-1101 (ScreenCapture::videoDurationUs), verbatim.
qint64 videoDuration(const Private *d)
{
    const qint64 index = d->lastFrameIndex.load(std::memory_order_relaxed);
    return index < 0 ? 0 : std::llround((index + 1) * 1e6 / d->config.fps);
}

void captureStart(const Context &context, const QJsonObject &cases)
{
    Text out;
    for (const QJsonValue &value : cases.value(QStringLiteral("trims")).toArray()) {
        const QJsonObject input = value.toObject();
        Private d;
        d.width = sizeFrom(input.value(QStringLiteral("size"))).width();
        d.height = sizeFrom(input.value(QStringLiteral("size"))).height();
        d.stride = d.width * 4;
        ScreenCaptureConfig config;
        config.crop = rectFrom(input.value(QStringLiteral("crop")));
        QString error = QStringLiteral("<untouched>");
        const bool ok = startCrop(&d, config, &error);
        out.row({"crop", sizeText(QSize(d.width, d.height)), rectText(config.crop), boolText(ok), quote(error),
                 rectText(d.crop), sizeText(d.encodedSize), QByteArray::number(d.cropRows.size())});
    }
    for (const QJsonValue &value : cases.value(QStringLiteral("encoders")).toArray()) {
        const QJsonObject input = value.toObject();
        ScopedEnvironment environment;
        environment.unset("OMAREEL_NATIVE_CONVERSION");
        environment.unset("OMAREEL_NATIVE_ENCODER");
        if (input.contains(QStringLiteral("conversion")))
            environment.set("OMAREEL_NATIVE_CONVERSION", input.value(QStringLiteral("conversion")).toString().toUtf8());
        if (input.contains(QStringLiteral("encoder")))
            environment.set("OMAREEL_NATIVE_ENCODER", input.value(QStringLiteral("encoder")).toString().toUtf8());
        Private d;
        d.encodedSize = sizeFrom(input.value(QStringLiteral("size")));
        ScreenCaptureConfig config;
        config.fps = input.value(QStringLiteral("fps")).toInt();
        config.outputPath = QStringLiteral("/recordings/Recording.omareel/screen-video.mp4");
        EncoderProbeResults probes;
        probes.preferred = input.value(QStringLiteral("preferred")).toBool();
        probes.gpuConversion = input.value(QStringLiteral("gpuConversion")).toBool();
        const QStringList arguments = startArguments(&d, config, probes);
        out.row({"encoder", sizeText(d.encodedSize), QByteArray::number(config.fps), boolText(probes.preferred),
                 boolText(probes.gpuConversion),
                 input.contains(QStringLiteral("conversion")) ? quote(input.value(QStringLiteral("conversion")).toString()) : QByteArray("<unset>"),
                 input.contains(QStringLiteral("encoder")) ? quote(input.value(QStringLiteral("encoder")).toString()) : QByteArray("<unset>"),
                 quote(d.conversion), quoteList(arguments)});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("start.txt")), out.data());
}

// The probe key holds the NVIDIA driver version from /sys/module/nvidia/version, a fact of the
// machine: it is shown as {driver}.
QByteArray maskDriver(QByteArray text, const QString &driver)
{
    const QByteArray value = QJsonDocument(QJsonArray{driver}).toJson(QJsonDocument::Compact).mid(1);
    const QByteArray quoted = value.left(value.size() - 1);
    text.replace("\"driver\": " + quoted, "\"driver\": \"{driver}\"");
    text.replace("\"driver\":" + quoted, "\"driver\":\"{driver}\"");
    return sanitize(QString::fromUtf8(text)).toUtf8();
}

void captureProbes(const Context &context)
{
    Text out;
    const QString shims = QFileInfo(shimDirectory(context.scratch, QStringLiteral("probe-links"), {QStringLiteral("ffmpeg")})
                                        + QStringLiteral("/ffmpeg")).symLinkTarget();
    const QString bin = shimDirectory(context.scratch, QStringLiteral("ffmpeg"), {});
    // A copy of the ffmpeg shim with a fixed modification time stands for the installed ffmpeg,
    // whose path, time and size the key records.
    QFile::copy(shims, QDir(bin).filePath(QStringLiteral("ffmpeg")));
    QFile::copy(QFileInfo(shims).absolutePath() + QStringLiteral("/replay.sh"), QDir(bin).filePath(QStringLiteral("replay.sh")));
    const auto setTime = [](const QString &path, qint64 seconds) {
        const timespec times[2] = {{seconds, 0}, {seconds, 0}};
        ::utimensat(AT_FDCWD, QFile::encodeName(path).constData(), times, 0);
    };
    const QString ffmpeg = QDir(bin).filePath(QStringLiteral("ffmpeg"));
    setTime(ffmpeg, 1700000000);
    const QString cache = QDir(context.scratch).filePath(QStringLiteral("cache"));
    ScopedEnvironment environment;
    environment.set("XDG_CACHE_HOME", QFile::encodeName(cache));
    environment.set("PATH", QFile::encodeName(bin));
    const QString driver = encoderProbeKey().value(QStringLiteral("driver")).toString();
    const QStringList preferredProbe{QStringLiteral("-hide_banner"), QStringLiteral("-loglevel"), QStringLiteral("error"),
        QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("color=size=256x256:rate=1"),
        QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-c:v"), QStringLiteral("h264_nvenc"),
        QStringLiteral("-f"), QStringLiteral("null"), QStringLiteral("-")};
    const QStringList gpuProbe{QStringLiteral("-hide_banner"), QStringLiteral("-loglevel"), QStringLiteral("error"),
        QStringLiteral("-f"), QStringLiteral("rawvideo"), QStringLiteral("-pix_fmt"), QStringLiteral("bgra"),
        QStringLiteral("-s"), QStringLiteral("256x256"), QStringLiteral("-r"), QStringLiteral("1"), QStringLiteral("-i"),
        QStringLiteral("pipe:0"), QStringLiteral("-frames:v"), QStringLiteral("1"), QStringLiteral("-vf"),
        QStringLiteral("hwupload_cuda,scale_cuda=format=nv12"), QStringLiteral("-c:v"), QStringLiteral("h264_nvenc"),
        QStringLiteral("-f"), QStringLiteral("null"), QStringLiteral("-")};
    const QString cacheFile = QDir(cache).filePath(QStringLiteral("omareel/encoder-probe.json"));
    const auto dumpCache = [&] {
        bool readable = false;
        const QByteArray bytes = QFileInfo(cacheFile).isFile() ? readFile(cacheFile, &readable) : QByteArray();
        out.row({"  cache", readable ? quoteBytes(maskDriver(bytes, driver)) : QByteArray("-"), octalMode(cacheFile)});
    };
    out.row({"encoderProbeCachePath", quote(sanitize(encoderProbeCachePath()))});
    out.row({"encoderProbeKey", maskDriver(QJsonDocument(encoderProbeKey()).toJson(QJsonDocument::Compact), driver)});
    struct Scenario {
        QByteArray label;
        QByteArray cache; // "-" for none
        int preferredStatus = 0;
        int gpuStatus = 0;
        qint64 mtime = 1700000000;
        bool ffmpegOnPath = true;
        bool cacheWritable = true;
    };
    const QByteArray matching = QJsonDocument(QJsonObject{{"key", encoderProbeKey()}, {"preferred", true},
                                                          {"gpuConversion", false}}).toJson(QJsonDocument::Indented);
    const QByteArray stringy = QJsonDocument(QJsonObject{{"key", encoderProbeKey()}, {"preferred", "true"},
                                                         {"gpuConversion", 1}}).toJson(QJsonDocument::Indented);
    const QList<Scenario> scenarios{
        {"no cache, both probes pass", "-", 0, 0},
        {"no cache, NVENC missing", "-", 1, 1},
        {"no cache, no GPU conversion", "-", 0, 1},
        {"no cache, only the conversion probe passes", "-", 1, 0},
        {"cache matches", matching},
        {"cache with wrong types", stringy},
        {"cache key differs (ffmpeg updated)", matching, 1, 0, 1700000001},
        {"cache is not JSON", "{", 0, 0},
        {"cache is an array", "[]", 0, 1},
        {"cache directory not writable", "-", 0, 0, 1700000000, true, false},
        {"ffmpeg not on PATH", "-", 0, 0, 1700000000, false},
    };
    for (const Scenario &scenario : scenarios) {
        QDir(cache).removeRecursively();
        QDir().mkpath(QDir(cache).filePath(QStringLiteral("omareel")));
        if (scenario.cache != "-") writeFile(cacheFile, scenario.cache);
        if (!scenario.cacheWritable) ::chmod(QFile::encodeName(QDir(cache).filePath(QStringLiteral("omareel"))).constData(), 0500);
        setTime(ffmpeg, scenario.mtime);
        if (!scenario.ffmpegOnPath) environment.set("PATH", QFile::encodeName(shimDirectory(context.scratch, QStringLiteral("empty"), {})));
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay")));
        Replay::Answer preferred; preferred.status = scenario.preferredStatus;
        replay.answer(QStringLiteral("ffmpeg"), preferredProbe, preferred);
        Replay::Answer gpu; gpu.status = scenario.gpuStatus; gpu.readStdin = true;
        if (scenario.gpuStatus) gpu.err = "Impossible to convert between the formats\n";
        replay.answer(QStringLiteral("ffmpeg"), gpuProbe, gpu);
        replay.activate();
        out << "== " << scenario.label << "\n";
        const QJsonObject key = encoderProbeKey();
        out.row({"key", maskDriver(QJsonDocument(key).toJson(QJsonDocument::Compact), driver)});
        const EncoderProbeResults results = encoderProbeResults();
        out.row({"encoderProbeResults", boolText(results.preferred), boolText(results.gpuConversion), boolText(results.cached)});
        // The two probes run at once, so their calls are listed sorted.
        QList<QStringList> calls = replay.calls();
        std::sort(calls.begin(), calls.end());
        for (const QStringList &call : calls) out.row({"  call", quoteList(call)});
        qint64 stdinBytes = 0;
        for (int index = 0; index < calls.size(); ++index) stdinBytes += replay.standardInput(index).size();
        out.row({"  stdin bytes", QByteArray::number(stdinBytes)});
        dumpCache();
        out.row({"cachedPreferredEncoderAvailable", boolText(Omareel::cachedPreferredEncoderAvailable())});
        ::chmod(QFile::encodeName(QDir(cache).filePath(QStringLiteral("omareel"))).constData(), 0700);
        if (!scenario.ffmpegOnPath) environment.set("PATH", QFile::encodeName(bin));
    }
    QDir(cache).removeRecursively();
    out.row({"cachedPreferredEncoderAvailable", "<no cache>", boolText(Omareel::cachedPreferredEncoderAvailable())});
    writeFile(QDir(context.output).filePath(QStringLiteral("encoder-probe.txt")), out.data());
}

void captureWriter(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject scenarios = cases.value(QStringLiteral("writer")).toObject();
    const QString files = QDir(context.scratch).filePath(QStringLiteral("writer"));
    for (auto it = scenarios.begin(); it != scenarios.end(); ++it) {
        const QJsonObject input = it.value().toObject();
        QDir(files).removeRecursively();
        QDir().mkpath(files);
        const QSize size = sizeFrom(input.value(QStringLiteral("size")));
        Private d;
        d.width = size.width();
        d.height = size.height();
        d.stride = input.contains(QStringLiteral("stride")) ? input.value(QStringLiteral("stride")).toInt() : size.width() * 4;
        d.config.fps = std::max(1, input.value(QStringLiteral("fps")).toInt());
        d.config.timestampPath = QDir(files).filePath(input.contains(QStringLiteral("timestampPath"))
            ? input.value(QStringLiteral("timestampPath")).toString() : QStringLiteral("screen.mp4.ts"));
        d.crop = input.contains(QStringLiteral("crop")) ? rectFrom(input.value(QStringLiteral("crop"))) : QRect(QPoint(), size);
        d.encodedSize = d.crop.size();
        d.cropRows = Omareel::captureCropRows(size, d.stride, d.crop);
        const QString stream = QDir(files).filePath(QStringLiteral("stream.mkv"));
        d.encoderFd = input.value(QStringLiteral("encoder")).toString() == QLatin1String("closed")
            ? -1 : ::open(QFile::encodeName(stream).constData(), O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
        if (input.value(QStringLiteral("abort")).toBool()) d.writerAbort.store(true);
        const qsizetype slotBytes = qsizetype(d.stride) * d.height;
        std::vector<QByteArray> memory(size_t(Private::slotCount), QByteArray(slotBytes, '\0'));
        d.captureSlots.resize(Private::slotCount);
        for (int index = 0; index < Private::slotCount; ++index) {
            d.captureSlots[size_t(index)].owner = &d;
            d.captureSlots[size_t(index)].index = index;
            d.captureSlots[size_t(index)].memory = reinterpret_cast<uchar *>(memory[size_t(index)].data());
        }
        std::atomic_bool finished{false};
        const qint64 before = realtimeNowUs();
        std::thread writer;
        // Frames are queued as captureFrame queues them: a slot taken from the ring, its pixels
        // and presentation time set, marked queued, the writer woken. Frame k's bytes are
        // (16k + offset) & 0xff, so the output shows which frame and which pixels were written.
        // The first frames fill the ring before the writer starts, so a writer that fails at
        // once leaves the same frames queued every time.
        int queued = 0;
        const QJsonArray timeList = input.value(QStringLiteral("times")).toArray();
        for (const QJsonValue &time : timeList) {
            if (!writer.joinable() && queued == Private::slotCount)
                writer = std::thread([&] { d.writerLoop(); finished.store(true); });
            int slot = -1;
            while (!finished.load()) {
                {
                    std::lock_guard<std::mutex> lock(d.ringMutex);
                    if (d.ring.count(CaptureRingBookkeeping::State::Available) > 0) slot = d.ring.acquire();
                }
                if (slot >= 0) break;
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            if (slot < 0) break;
            uchar *bytes = d.captureSlots[size_t(slot)].memory;
            for (qsizetype offset = 0; offset < slotBytes; ++offset) bytes[offset] = uchar((16 * queued + offset) & 0xff);
            d.captureSlots[size_t(slot)].presentationUs = time.toInteger();
            {
                std::lock_guard<std::mutex> lock(d.ringMutex);
                d.ring.markQueued(slot);
            }
            d.queuedFrame.notify_one();
            ++queued;
        }
        if (!writer.joinable()) writer = std::thread([&] { d.writerLoop(); finished.store(true); });
        d.stopUs = input.value(QStringLiteral("stop")).toInteger();
        {
            std::lock_guard<std::mutex> lock(d.ringMutex);
            d.writerStopping = true;
        }
        d.queuedFrame.notify_all();
        writer.join();
        const qint64 after = realtimeNowUs();
        if (d.encoderFd >= 0) ::close(d.encoderFd);
        d.encoderFd = -1;
        out << "== " << it.key().toUtf8() << "\n";
        QList<QByteArray> times;
        for (const QJsonValue &time : timeList) times << QByteArray::number(time.toInteger());
        out.row({"input", QByteArray::number(d.config.fps), sizeText(size), QByteArray::number(d.stride), rectText(d.crop),
                 times.isEmpty() ? QByteArray("-") : times.join(','), QByteArray::number(d.stopUs),
                 QByteArray::number(queued)});
        out.row({"result", "frames=" + QByteArray::number(d.frames.load()), "firstUs=" + QByteArray::number(d.firstUs.load()),
                 "lastFrameIndex=" + QByteArray::number(d.lastFrameIndex.load()),
                 "videoDurationUs=" + QByteArray::number(videoDuration(&d)),
                 "failed=" + boolText(d.writerFailed.load()), "error=" + quote(d.writerError)});
        out.row({"ring", ringStates(d.ring, Private::slotCount), QByteArray::number(d.ring.drops())});
        bool readable = false;
        const QByteArray ts = readFile(d.config.timestampPath, &readable);
        out.row({"timestamp", readable ? quoteBytes(maskRealtime(ts, before, after)) : QByteArray("-")});
        const QByteArray bytes = readFile(stream, &readable);
        out.row({"stream", readable ? QByteArray::number(bytes.size()) : QByteArray("-"),
                 readable ? bytes.toHex() : QByteArray("-")});
    }
    QDir(files).removeRecursively();
    writeFile(QDir(context.output).filePath(QStringLiteral("writer-loop.txt")), out.data());
}

} // namespace

void runScreenCapture(const Context &context)
{
    const QJsonObject cases = corpusObject(context, QStringLiteral("screencapture/cases.json"));
    captureRings(context, cases);
    captureCrops(context, cases);
    captureHeaders(context, cases);
    captureStart(context, cases);
    captureProbes(context);
    captureWriter(context, cases);
}

} // namespace Oracle
