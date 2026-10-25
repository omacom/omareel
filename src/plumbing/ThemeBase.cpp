#include "ThemeBase.h"

#include <QCoreApplication>
#include <QFileInfo>
#include <QFont>
#include <QFontDatabase>
#include <QGuiApplication>
#include <QTimer>

using namespace Omareel;

static QString resolvedFamily;

ThemeBase::ThemeBase(QObject *parent) : QObject(parent)
{
    const auto changed = [this] {
        emit sourceChanged();
        QTimer::singleShot(220, this, &ThemeBase::reload);
    };
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, changed);
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, changed);
}

void ThemeBase::watchPaths(const QStringList &paths)
{
    if (!m_watcher.files().isEmpty()) m_watcher.removePaths(m_watcher.files());
    if (!m_watcher.directories().isEmpty()) m_watcher.removePaths(m_watcher.directories());
    QStringList existing = paths;
    existing.removeDuplicates();
    for (auto it = existing.begin(); it != existing.end();) {
        if (!QFileInfo::exists(*it)) it = existing.erase(it); else ++it;
    }
    if (!existing.isEmpty()) m_watcher.addPaths(existing);
}

void ThemeFonts::setConfiguredFamily(const QString &family)
{
    resolvedFamily = family;
}

QString ThemeFonts::configuredFamily()
{
    return resolvedFamily;
}

QString ThemeFonts::bundledUiFamily()
{
    static QString family;
    static bool attempted = false;
    if (attempted || !qobject_cast<QGuiApplication *>(QCoreApplication::instance())) return family;
    attempted = true;
    for (const char *weight : {"Regular", "Medium", "SemiBold", "Bold"}) {
        const int id = QFontDatabase::addApplicationFont(
            QStringLiteral(":/omareel/assets/fonts/Inter-%1.ttf").arg(QLatin1String(weight)));
        const QStringList families = QFontDatabase::applicationFontFamilies(id);
        if (family.isEmpty() && !families.isEmpty()) family = families.first();
    }
    return family;
}

void ThemeFonts::setApplicationFont(const QString &family, int pixelSize)
{
    if (!qobject_cast<QGuiApplication *>(QCoreApplication::instance())) return;
    QFont font(family);
    font.setPixelSize(pixelSize);
    QGuiApplication::setFont(font);
}

QString ThemeFonts::interfaceFamily()
{
    if (!resolvedFamily.isEmpty()) return resolvedFamily;
    const QString bundled = bundledUiFamily();
    return bundled.isEmpty() ? QStringLiteral("sans-serif") : bundled;
}
