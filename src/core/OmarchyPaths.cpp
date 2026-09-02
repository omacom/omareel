#include "OmarchyPaths.h"

#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QStandardPaths>

using namespace OmaRecord;

QString OmarchyPaths::stateRoot()
{
    const QString overridePath = qEnvironmentVariable("OMARECORD_OMARCHY_STATE_DIR");
    return overridePath.isEmpty()
        ? QDir::home().filePath(QStringLiteral(".local/state/omarchy/current"))
        : QDir(overridePath).absolutePath();
}

QStringList OmarchyPaths::themeBackgrounds()
{
    const QDir directory(QDir(stateRoot()).filePath(QStringLiteral("theme/backgrounds")));
    QStringList result;
    for (const QFileInfo &file : directory.entryInfoList(
             {QStringLiteral("*.png"), QStringLiteral("*.jpg"), QStringLiteral("*.jpeg"),
              QStringLiteral("*.webp")}, QDir::Files, QDir::Name))
        result << file.canonicalFilePath();
    return result;
}

QString OmarchyPaths::currentBackground()
{
    const QFileInfo link(QDir(stateRoot()).filePath(QStringLiteral("background")));
    if (link.isSymLink()) {
        const QString target = link.symLinkTarget();
        if (QFileInfo(target).isFile()) return QFileInfo(target).canonicalFilePath();
    }
    const QStringList backgrounds = themeBackgrounds();
    return backgrounds.isEmpty() ? QString() : backgrounds.first();
}

QString OmarchyPaths::recordingsDirectory()
{
    const QString overridePath = qEnvironmentVariable("OMARECORD_RECENTS_DIR");
    return overridePath.isEmpty()
        ? QDir(QStandardPaths::writableLocation(QStandardPaths::MoviesLocation))
              .filePath(QStringLiteral("omarecord"))
        : QDir(overridePath).absolutePath();
}

QString OmarchyPaths::wallpaperThumbnailPath(const QString &wallpaperPath)
{
    const QFileInfo source(wallpaperPath);
    const QByteArray identity = source.absoluteFilePath().toUtf8() + '\0'
        + QByteArray::number(source.size()) + '\0'
        + QByteArray::number(source.lastModified().toMSecsSinceEpoch());
    const QString hash = QString::fromLatin1(
        QCryptographicHash::hash(identity, QCryptographicHash::Sha256).toHex());
    const QString directory = QDir(QStandardPaths::writableLocation(QStandardPaths::GenericCacheLocation))
                                  .filePath(QStringLiteral("omarecord/wallpapers"));
    return QDir(directory).filePath(hash + QStringLiteral(".jpg"));
}

bool OmarchyPaths::generateWallpaperThumbnail(const QString &wallpaperPath,
                                               const QString &thumbnailPath)
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
