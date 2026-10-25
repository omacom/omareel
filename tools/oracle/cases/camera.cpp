// camera: the pure parts of src/record/CameraCapture.cpp, which is included whole (its
// selectedFormat is file-static): stableCameraPath and cameraNodePath over fake /dev and /sys trees,
// cameraIdNode, cameraFrameDeadlineUs, FirstFrameTimestamp's file, and the format choice
// (selectedFormat, cameraCanRecord) for cameras built with Qt's private QCameraDevicePrivate.
// tests/golden/README.md, camera.

#include "record/CameraCapture.cpp"

#include "common.h"
#include "recording.h"

#include <QtMultimedia/private/qcameradevice_p.h>

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonObject>

#include <limits>
#include <sys/stat.h>

namespace Oracle {

namespace {

QVideoFrameFormat::PixelFormat pixelFormat(const QString &name)
{
    if (name == QLatin1String("Jpeg")) return QVideoFrameFormat::Format_Jpeg;
    if (name == QLatin1String("YUYV")) return QVideoFrameFormat::Format_YUYV;
    if (name == QLatin1String("NV12")) return QVideoFrameFormat::Format_NV12;
    if (name == QLatin1String("Y8")) return QVideoFrameFormat::Format_Y8;
    fail(QStringLiteral("unknown pixel format %1").arg(name));
    return QVideoFrameFormat::Format_Invalid;
}

QByteArray pixelFormatName(QVideoFrameFormat::PixelFormat format)
{
    switch (format) {
    case QVideoFrameFormat::Format_Jpeg: return "Jpeg";
    case QVideoFrameFormat::Format_YUYV: return "YUYV";
    case QVideoFrameFormat::Format_NV12: return "NV12";
    case QVideoFrameFormat::Format_Y8: return "Y8";
    case QVideoFrameFormat::Format_Invalid: return "Invalid";
    default: return "Format" + QByteArray::number(int(format));
    }
}

} // namespace

// Cameras as the multimedia backend would list them. Used by the launcher family too.
QCameraFormat fakeCameraFormat(const QJsonArray &format)
{
    auto *d = new QCameraFormatPrivate;
    d->pixelFormat = pixelFormat(format.at(0).toString());
    d->resolution = QSize(format.at(1).toInt(), format.at(2).toInt());
    d->minFrameRate = float(format.at(3).toDouble());
    d->maxFrameRate = float(format.at(4).toDouble());
    return d->create();
}

QCameraDevice fakeCamera(const QByteArray &id, const QString &description, const QJsonArray &formats)
{
    auto *d = new QCameraDevicePrivate;
    d->id = id;
    d->description = description;
    for (const QJsonValue &format : formats) d->videoFormats << fakeCameraFormat(format.toArray());
    return d->create();
}

bool fakeCameraCanRecord(const QCameraDevice &device)
{
    return Omareel::cameraCanRecord(device);
}

namespace {

QByteArray formatCells(const QCameraFormat &format)
{
    if (format.isNull()) return "null";
    return pixelFormatName(format.pixelFormat()) + ' ' + QByteArray::number(format.resolution().width()) + 'x'
        + QByteArray::number(format.resolution().height()) + ' ' + num(format.minFrameRate()) + ' '
        + num(format.maxFrameRate());
}

void cameraTrees(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject trees = cases.value(QStringLiteral("trees")).toObject();
    for (auto tree = trees.begin(); tree != trees.end(); ++tree) {
        const QString root = QDir(context.scratch).filePath(QStringLiteral("trees/") + tree.key());
        QDir(root).removeRecursively();
        QDir().mkpath(root);
        const auto at = [&root](const QString &relative) {
            return relative.isEmpty() || relative.startsWith(QLatin1Char('/')) ? relative : QDir(root).filePath(relative);
        };
        for (const QJsonValue &stateValue : tree.value().toArray()) {
            const QJsonObject state = stateValue.toObject();
            for (const QJsonValue &path : state.value(QStringLiteral("remove")).toArray())
                QFile::remove(at(path.toString()));
            const QJsonObject files = state.value(QStringLiteral("files")).toObject();
            for (auto it = files.begin(); it != files.end(); ++it) writeFile(at(it.key()), it.value().toString().toUtf8());
            for (const QJsonValue &directory : state.value(QStringLiteral("dirs")).toArray())
                QDir().mkpath(at(directory.toString()));
            const QJsonObject links = state.value(QStringLiteral("links")).toObject();
            for (auto it = links.begin(); it != links.end(); ++it) {
                QDir().mkpath(QFileInfo(at(it.key())).absolutePath());
                QFile::remove(at(it.key()));
                if (!QFile::link(at(it.value().toString()), at(it.key())))
                    fail(QStringLiteral("could not link %1").arg(it.key()));
            }
            out << "== " << tree.key().toUtf8() << ": " << state.value(QStringLiteral("label")).toString().toUtf8() << "\n";
            for (const QJsonValue &query : state.value(QStringLiteral("queries")).toArray()) {
                const QString path = at(query.toString());
                out.row({quote(query.toString()),
                         quote(sanitize(Omareel::stableCameraPath(path, QDir(root).filePath(QStringLiteral("dev")),
                                                                  QDir(root).filePath(QStringLiteral("sys"))))),
                         quote(sanitize(Omareel::cameraNodePath(path)))});
            }
        }
        QDir(root).removeRecursively();
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("stable-camera-path.txt")), out.data());
}

void cameraIds(const Context &context, const QJsonObject &cases)
{
    Text out;
    for (const QJsonValue &id : cases.value(QStringLiteral("ids")).toArray())
        out.row({quote(id.toString()), quote(Omareel::cameraIdNode(id.toString().toUtf8()))});
    out.row({"<bytes \\xff/dev/video3>", quote(Omareel::cameraIdNode(QByteArray("\xff/dev/video3")))});
    writeFile(QDir(context.output).filePath(QStringLiteral("camera-id-node.txt")), out.data());
}

void cameraDeadlines(const Context &context)
{
    Text out;
    for (const qint64 first : {qint64(0), qint64(1), qint64(123456789), qint64(-1000000), qint64(1) << 52})
        for (const qint64 frame : {qint64(0), qint64(1), qint64(2), qint64(3), qint64(29), qint64(30), qint64(31),
                                   qint64(240), qint64(108000), qint64(-1), qint64(-2), qint64(-31),
                                   qint64(1000000000), qint64(9000000000000LL)})
            out.row({QByteArray::number(first), QByteArray::number(frame),
                     QByteArray::number(Omareel::cameraFrameDeadlineUs(first, frame))});
    writeFile(QDir(context.output).filePath(QStringLiteral("camera-frame-deadline.txt")), out.data());
}

void cameraTimestamps(const Context &context)
{
    Text out;
    const QString files = QDir(context.scratch).filePath(QStringLiteral("timestamps"));
    QDir().mkpath(files);
    QDir().mkpath(QDir(files).filePath(QStringLiteral("directory.ts")));
    const auto show = [&](const QString &path) {
        bool readable = false;
        const QByteArray bytes = QFileInfo(path).isFile() ? readFile(path, &readable) : QByteArray();
        out.row({"  file", quote(sanitize(path)), readable ? quoteBytes(bytes) : QByteArray("-"), octalMode(path)});
    };
    const auto arrive = [&](Omareel::FirstFrameTimestamp &timestamp, qint64 monotonic, qint64 realtime) {
        const bool ok = timestamp.recordFrameArrival(monotonic, realtime);
        out.row({"recordFrameArrival", QByteArray::number(monotonic), QByteArray::number(realtime), boolText(ok),
                 QByteArray::number(timestamp.firstFrameUs())});
    };
    {
        out << "== the tag's test\n";
        const QString path = QDir(files).filePath(QStringLiteral("camera.mp4.ts"));
        Omareel::FirstFrameTimestamp timestamp(path);
        arrive(timestamp, 1234567, 9876543);
        arrive(timestamp, 2345678, 8765432);
        show(path);
        const QString next = QDir(files).filePath(QStringLiteral("next.ts"));
        timestamp.setPath(next);
        out.row({"setPath", quote(sanitize(next)), QByteArray::number(timestamp.firstFrameUs())});
        arrive(timestamp, 42, 43);
        show(next);
        show(path);
    }
    {
        out << "== refused values\n";
        const QString path = QDir(files).filePath(QStringLiteral("refused.ts"));
        Omareel::FirstFrameTimestamp timestamp(path);
        arrive(timestamp, 0, 1);
        arrive(timestamp, -5, 1);
        arrive(timestamp, 1, 0);
        arrive(timestamp, 1, -1);
        show(path);
        arrive(timestamp, std::numeric_limits<qint64>::max(), std::numeric_limits<qint64>::max());
        show(path);
    }
    {
        out << "== no path\n";
        Omareel::FirstFrameTimestamp timestamp;
        arrive(timestamp, 1, 1);
        timestamp.setPath(QString());
        arrive(timestamp, 1, 1);
    }
    for (const auto &[label, name] : QList<QPair<QByteArray, QString>>{
             {"missing directory", QStringLiteral("missing/camera.mp4.ts")}, {"path is a directory", QStringLiteral("directory.ts")}}) {
        out << "== " << label << "\n";
        const QString path = QDir(files).filePath(name);
        Omareel::FirstFrameTimestamp timestamp(path);
        arrive(timestamp, 10, 20);
        arrive(timestamp, 11, 21);
        show(path);
    }
    {
        out << "== replaces an existing file, keeping mode 0600\n";
        const QString path = QDir(files).filePath(QStringLiteral("existing.ts"));
        writeFile(path, "old contents\n");
        ::chmod(QFile::encodeName(path).constData(), 0600);
        Omareel::FirstFrameTimestamp timestamp(path);
        arrive(timestamp, 7, 8);
        show(path);
    }
    QDir(files).removeRecursively();
    writeFile(QDir(context.output).filePath(QStringLiteral("first-frame-timestamp.txt")), out.data());
}

void cameraFormats(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject devices = cases.value(QStringLiteral("devices")).toObject();
    for (auto it = devices.begin(); it != devices.end(); ++it) {
        const QCameraDevice device = fakeCamera("/dev/video0", it.key(), it.value().toArray());
        QList<QByteArray> cells{it.key().toUtf8()};
        for (const int height : {1080, 720, 0, 480})
            cells << QByteArray::number(height) + '=' + formatCells(selectedFormat(device, height));
        cells << "cameraCanRecord=" + boolText(Omareel::cameraCanRecord(device));
        out.row(cells);
    }
    out.row({"<null device>", "1080=" + formatCells(selectedFormat(QCameraDevice(), 1080)),
             "cameraCanRecord=" + boolText(Omareel::cameraCanRecord(QCameraDevice()))});
    writeFile(QDir(context.output).filePath(QStringLiteral("selected-format.txt")), out.data());

    // cameraDeviceForPath compares the Qt id's /dev/videoN with the path's canonical node, which
    // is a real /dev entry or nothing; only paths that resolve to nothing are hermetic.
    Text paths;
    const QJsonArray formats{QJsonArray{QStringLiteral("Jpeg"), 1920, 1080, 30, 30}};
    const QList<QCameraDevice> cameras{fakeCamera("/dev/video0", QStringLiteral("A"), formats),
                                       fakeCamera("v4l2 /dev/video2", QStringLiteral("B"), formats)};
    for (const QString &path : {QString(), QStringLiteral("/dev/oracle-camera-that-does-not-exist"),
                                QDir(context.scratch).filePath(QStringLiteral("missing/video0"))}) {
        const QCameraDevice device = Omareel::cameraDeviceForPath(cameras, path);
        paths.row({quote(sanitize(path)), device.isNull() ? QByteArray("null") : quote(QString::fromUtf8(device.id()))});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("camera-device-for-path.txt")), paths.data());
}

} // namespace

void runCamera(const Context &context)
{
    const QJsonObject cases = corpusObject(context, QStringLiteral("camera/cases.json"));
    cameraTrees(context, cases);
    cameraIds(context, cases);
    cameraDeadlines(context);
    cameraTimestamps(context);
    cameraFormats(context, cases);
}

} // namespace Oracle
