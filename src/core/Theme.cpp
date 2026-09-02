#include "Theme.h"

#include <QDir>
#include <QFile>
#include <QRegularExpression>

using namespace OmaRecord;

static QString stateRoot()
{
    return QDir::homePath() + QStringLiteral("/.local/state/omarchy/current");
}

Theme::Theme(QObject *parent): QObject(parent)
{
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, [this] { reload(); });
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, [this] { reload(); });
    reload();
}

void Theme::reload()
{
    QColor accent(QStringLiteral("#7aa2f7"));
    QColor background(QStringLiteral("#1a1b26"));
    QColor foreground(QStringLiteral("#c0caf5"));
    QFile file(stateRoot() + QStringLiteral("/theme/colors.toml"));
    if (file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        const QRegularExpression linePattern(
            QStringLiteral(R"(^\s*([A-Za-z0-9_-]+)\s*=\s*["']([^"']+)["'])"));
        while (!file.atEnd()) {
            const auto match = linePattern.match(QString::fromUtf8(file.readLine()));
            if (!match.hasMatch()) continue;
            const QColor value(match.captured(2));
            if (!value.isValid()) continue;
            if (match.captured(1) == QLatin1String("accent")) accent = value;
            else if (match.captured(1) == QLatin1String("background")) background = value;
            else if (match.captured(1) == QLatin1String("foreground")) foreground = value;
        }
    }
    const bool didChange = accent != m_accent || background != m_background || foreground != m_foreground;
    m_accent = accent;
    m_background = background;
    m_foreground = foreground;
    rearmWatcher();
    if (didChange) emit changed();
}

void Theme::rearmWatcher()
{
    if (!m_watcher.files().isEmpty()) m_watcher.removePaths(m_watcher.files());
    if (!m_watcher.directories().isEmpty()) m_watcher.removePaths(m_watcher.directories());
    const QString current = stateRoot();
    const QString state = QFileInfo(current).absolutePath();
    const QString theme = current + QStringLiteral("/theme");
    const QString colors = theme + QStringLiteral("/colors.toml");
    QStringList paths;
    if (QFileInfo::exists(state)) paths << state;
    if (QFileInfo::exists(current)) paths << current;
    if (QFileInfo::exists(theme)) paths << theme;
    if (QFileInfo::exists(colors)) paths << colors;
    if (!paths.isEmpty()) m_watcher.addPaths(paths);
}
