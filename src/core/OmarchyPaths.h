#pragma once

#include <QString>
#include <QStringList>

namespace OmaRecord::OmarchyPaths {

QString stateRoot();
QStringList themeBackgrounds();
QString currentBackground();
QString recordingsDirectory();
QString wallpaperThumbnailPath(const QString &wallpaperPath);
bool generateWallpaperThumbnail(const QString &wallpaperPath, const QString &thumbnailPath);

} // namespace OmaRecord::OmarchyPaths
