#include "RecordingMetadata.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>

double OmaRecord::RecordingMetadata::captureDuration(const QString &bundlePath)
{
    QFile file(QDir(bundlePath).filePath(QStringLiteral("capture.json")));
    if (!file.open(QIODevice::ReadOnly)) return 0.0;
    const QJsonObject capture = QJsonDocument::fromJson(file.readAll()).object();
    const qint64 first = capture.value(QStringLiteral("first_frame_us")).toVariant().toLongLong();
    const qint64 stopped = capture.value(QStringLiteral("stopped_us")).toVariant().toLongLong();
    return stopped > first ? double(stopped - first) / 1000000.0 : 0.0;
}
