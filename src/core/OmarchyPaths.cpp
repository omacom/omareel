#include "OmarchyPaths.h"

#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QMap>
#include <QSet>
#include <QStandardPaths>

using namespace Omareel;

QString OmarchyPaths::stateRoot()
{
    const QString overridePath = qEnvironmentVariable("OMAREEL_OMARCHY_STATE_DIR");
    return overridePath.isEmpty()
        ? QDir::home().filePath(QStringLiteral(".local/state/omarchy/current"))
        : QDir(overridePath).absolutePath();
}

QStringList OmarchyPaths::themeBackgrounds()
{
    QStringList result;
    for (const QVariant &groupValue : themeBackgroundGroups()) {
        const QVariantList files = groupValue.toMap().value(QStringLiteral("paths")).toList();
        for (const QVariant &file : files) result << file.toString();
    }
    return result;
}

QString OmarchyPaths::currentThemeName()
{
    QFile file(QDir(stateRoot()).filePath(QStringLiteral("theme.name")));
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) return {};
    return QString::fromUtf8(file.readAll()).trimmed();
}

QVariantList OmarchyPaths::themeBackgroundGroups()
{
    const QStringList filters{QStringLiteral("*.png"), QStringLiteral("*.jpg"),
                              QStringLiteral("*.jpeg"), QStringLiteral("*.webp")};
    const QStringList roots{
        QDir::home().filePath(QStringLiteral(".config/omarchy/themes")),
        QDir::home().filePath(QStringLiteral(".local/share/omarchy/themes")),
        QStringLiteral("/usr/share/omarchy/themes")
    };
    QMap<QString, QStringList> grouped;
    QSet<QString> seen;
    for (const QString &rootPath : roots) {
        const QDir root(rootPath);
        for (const QFileInfo &theme : root.entryInfoList(QDir::Dirs | QDir::NoDotAndDotDot,
                                                        QDir::Name)) {
            const QDir backgrounds(QDir(theme.absoluteFilePath()).filePath(QStringLiteral("backgrounds")));
            for (const QFileInfo &file : backgrounds.entryInfoList(filters, QDir::Files, QDir::Name)) {
                const QString path = file.canonicalFilePath();
                if (path.isEmpty() || seen.contains(path)) continue;
                seen.insert(path);
                grouped[theme.fileName()] << path;
            }
        }
    }

    const QString currentName = currentThemeName();
    const QString currentPath = currentBackground();
    if (!currentPath.isEmpty() && !seen.contains(currentPath))
        grouped[currentName.isEmpty() ? QStringLiteral("Current") : currentName].prepend(currentPath);

    QStringList names = grouped.keys();
    if (!currentName.isEmpty() && names.removeOne(currentName)) names.prepend(currentName);
    QVariantList result;
    for (const QString &name : names) {
        QVariantList paths;
        for (const QString &path : grouped.value(name)) paths << path;
        QString label = name;
        label.replace(QLatin1Char('-'), QLatin1Char(' '));
        if (!label.isEmpty()) label[0] = label[0].toUpper();
        result << QVariantMap{{QStringLiteral("name"), name},
                              {QStringLiteral("label"), label},
                              {QStringLiteral("current"), name == currentName},
                              {QStringLiteral("paths"), paths}};
    }
    return result;
}

QString OmarchyPaths::currentBackground()
{
    const QFileInfo link(QDir(stateRoot()).filePath(QStringLiteral("background")));
    if (link.isSymLink()) {
        const QString target = link.symLinkTarget();
        if (QFileInfo(target).isFile()) return QFileInfo(target).canonicalFilePath();
    }
    const QDir directory(QDir(stateRoot()).filePath(QStringLiteral("theme/backgrounds")));
    QStringList backgrounds;
    for (const QFileInfo &file : directory.entryInfoList(
             {QStringLiteral("*.png"), QStringLiteral("*.jpg"), QStringLiteral("*.jpeg"),
              QStringLiteral("*.webp")}, QDir::Files, QDir::Name))
        backgrounds << file.canonicalFilePath();
    return backgrounds.isEmpty() ? QString() : backgrounds.first();
}

QString OmarchyPaths::recordingsDirectory()
{
    const QString overridePath = qEnvironmentVariable("OMAREEL_RECENTS_DIR");
    return overridePath.isEmpty()
        ? QDir(QStandardPaths::writableLocation(QStandardPaths::MoviesLocation))
              .filePath(QStringLiteral("omareel"))
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
                                  .filePath(QStringLiteral("omareel/wallpapers"));
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
