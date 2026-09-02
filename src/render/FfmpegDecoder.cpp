#include "FfmpegDecoder.h"

#include <QFileInfo>

using namespace OmaRecord;

FfmpegDecoder::~FfmpegDecoder()
{
    cancel();
}

bool FfmpegDecoder::start(const QString &path, double startTime, double length, double fps,
                          int width, int height, QString *error)
{
    cancel();
    m_cancelled = false;
    m_path = path;
    m_start = std::max(0.0, startTime);
    m_length = std::max(0.0, length);
    m_fps = fps;
    m_width = width;
    m_height = height;
    m_hardware = true;
    m_deliveredFrame = false;
    m_eof = false;
    if (!QFileInfo(path).isFile() || width <= 0 || height <= 0 || fps <= 0.0 || length <= 0.0) {
        if (error) *error = QStringLiteral("Invalid decoder input");
        return false;
    }
    return launch(true, error);
}

bool FfmpegDecoder::launch(bool hardware, QString *error)
{
    QStringList args{QStringLiteral("-v"), QStringLiteral("error")};
    if (hardware) args << QStringLiteral("-hwaccel") << QStringLiteral("cuda");
    args << QStringLiteral("-ss") << QString::number(m_start, 'f', 6)
         << QStringLiteral("-i") << m_path
         << QStringLiteral("-t") << QString::number(m_length, 'f', 6)
         << QStringLiteral("-vf") << QStringLiteral("fps=%1").arg(m_fps, 0, 'g', 12)
         << QStringLiteral("-f") << QStringLiteral("rawvideo")
         << QStringLiteral("-pix_fmt") << QStringLiteral("rgba") << QStringLiteral("-");
    m_process.setProcessChannelMode(QProcess::SeparateChannels);
    m_process.start(QStringLiteral("ffmpeg"), args, QIODevice::ReadOnly);
    if (!m_process.waitForStarted(5000)) {
        if (error) *error = m_process.errorString();
        return false;
    }
    m_hardware = hardware;
    return true;
}

bool FfmpegDecoder::readFrame(QImage *image, QString *error)
{
    if (m_cancelled) { if (error) *error = QStringLiteral("Export cancelled"); return false; }
    const qint64 needed = qint64(m_width) * m_height * 4;
    QByteArray bytes;
    bytes.reserve(needed);
    while (bytes.size() < needed && !m_cancelled) {
        if (m_process.bytesAvailable() == 0 && !m_process.waitForReadyRead(30000)) {
            if (m_process.state() == QProcess::NotRunning) break;
            if (error) *error = QStringLiteral("Timed out reading decoded video");
            return false;
        }
        bytes += m_process.read(needed - bytes.size());
    }
    if (bytes.size() != needed) {
        const QString details = QString::fromUtf8(m_process.readAllStandardError()).trimmed();
        if (m_hardware && !m_deliveredFrame && !m_cancelled) {
            m_process.kill();
            m_process.waitForFinished();
            if (!launch(false, error)) return false;
            return readFrame(image, error);
        }
        m_eof = true;
        if (!details.isEmpty() && error) *error = details;
        return false;
    }
    *image = QImage(reinterpret_cast<const uchar *>(bytes.constData()), m_width, m_height,
                    m_width * 4, QImage::Format_RGBA8888).copy();
    m_deliveredFrame = true;
    if (error) error->clear();
    return true;
}

void FfmpegDecoder::cancel()
{
    m_cancelled = true;
    if (m_process.state() != QProcess::NotRunning) {
        m_process.kill();
        m_process.waitForFinished(2000);
    }
}
