#pragma once

#include <QString>
#include <QStringList>
#include <QVariantList>

namespace OmaRecord::OmarchyPaths {

QString stateRoot();
QStringList themeBackgrounds();
QVariantList themeBackgroundGroups();
QString currentThemeName();
QString currentBackground();
QString recordingsDirectory();
QString wallpaperThumbnailPath(const QString &wallpaperPath);
bool generateWallpaperThumbnail(const QString &wallpaperPath, const QString &thumbnailPath);

} // namespace OmaRecord::OmarchyPaths
