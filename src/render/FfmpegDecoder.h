#pragma once

#include <QImage>
#include <QProcess>
#include <QString>
#include <atomic>

namespace OmaRecord {

class FfmpegDecoder
{
public:
    FfmpegDecoder() = default;
    ~FfmpegDecoder();
    bool start(const QString &path, double start, double length, double fps,
               int width, int height, bool scale, QString *error = nullptr);
    bool readFrame(QImage *image, QString *error = nullptr);
    void cancel();
    bool atEnd() const { return m_eof; }

private:
    bool launch(bool hardware, QString *error);
    QProcess m_process;
    QString m_path;
    double m_start = 0.0;
    double m_length = 0.0;
    double m_fps = 60.0;
    int m_width = 0;
    int m_height = 0;
    bool m_scale = false;
    bool m_hardware = true;
    bool m_deliveredFrame = false;
    bool m_eof = false;
    std::atomic_bool m_cancelled{false};
};

} // namespace OmaRecord
