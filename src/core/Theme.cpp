#include "Theme.h"
#include "OmarchyPaths.h"

#include <QDir>
#include <QFile>
#include <QRegularExpression>
#include <cmath>

using namespace OmaRecord;

static double relativeLuminance(const QColor &color)
{
    const auto linear = [](double channel) {
        channel /= 255.0;
        return channel <= 0.04045 ? channel / 12.92 : std::pow((channel + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * linear(color.red()) + 0.7152 * linear(color.green())
        + 0.0722 * linear(color.blue());
}

Theme::Theme(QObject *parent): QObject(parent)
{
    const auto sourceChanged = [this] { emit this->sourceChanged(); reload(); };
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, sourceChanged);
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, sourceChanged);
    reload();
}

QColor Theme::accentForeground() const
{
    return relativeLuminance(m_accent) > 0.179
        ? QColor(QStringLiteral("#101116")) : QColor(QStringLiteral("#ffffff"));
}

void Theme::reload()
{
    QColor accent(QStringLiteral("#7aa2f7"));
    QColor background(QStringLiteral("#1a1b26"));
    QColor foreground(QStringLiteral("#c0caf5"));
    QColor lighterBackground;
    QString mode;
    QFile file(QDir(OmarchyPaths::stateRoot()).filePath(QStringLiteral("theme/colors.toml")));
    if (file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        const QRegularExpression linePattern(
            QStringLiteral(R"(^\s*([A-Za-z0-9_-]+)\s*=\s*["']([^"']+)["'])"));
        while (!file.atEnd()) {
            const auto match = linePattern.match(QString::fromUtf8(file.readLine()));
            if (!match.hasMatch()) continue;
            const QString key = match.captured(1);
            const QString text = match.captured(2);
            if (key == QLatin1String("mode")) { mode = text.toLower(); continue; }
            const QColor value(text);
            if (!value.isValid()) continue;
            if (key == QLatin1String("accent")) accent = value;
            else if (key == QLatin1String("background")) background = value;
            else if (key == QLatin1String("lighter_background")) lighterBackground = value;
            else if (key == QLatin1String("foreground")) foreground = value;
        }
    }
    if (!lighterBackground.isValid()) lighterBackground = background.lighter(112);
    const QColor darkBackground(qRound(background.red() * .75),
                                qRound(background.green() * .75),
                                qRound(background.blue() * .75));
    const bool dark = mode == QLatin1String("dark")
        || (mode != QLatin1String("light") && relativeLuminance(background) < .35);
    const bool didChange = accent != m_accent
        || background != m_background || lighterBackground != m_lighterBackground
        || darkBackground != m_darkBackground || foreground != m_foreground || dark != m_dark;
    m_accent = accent;
    m_background = background;
    m_lighterBackground = lighterBackground;
    m_darkBackground = darkBackground;
    m_foreground = foreground;
    m_dark = dark;
    rearmWatcher();
    if (didChange) emit changed();
}

void Theme::rearmWatcher()
{
    if (!m_watcher.files().isEmpty()) m_watcher.removePaths(m_watcher.files());
    if (!m_watcher.directories().isEmpty()) m_watcher.removePaths(m_watcher.directories());
    const QString current = OmarchyPaths::stateRoot();
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
