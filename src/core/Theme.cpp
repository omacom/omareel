#include "Theme.h"
#include "OmarchyPaths.h"

#include <QDir>
#include <QFile>
#include <QFont>
#include <QCoreApplication>
#include <QRegularExpression>
#include <QGuiApplication>
#include <cmath>

using namespace OmaRecord;

static QString configuredFontFamily;
static QString configuredMonoFamily;

static QColor withAlpha(const QColor &color, int alpha)
{
    QColor result = color;
    result.setAlpha(alpha);
    return result;
}

static QColor blend(const QColor &from, const QColor &toward, double amount)
{
    return QColor(qRound(from.red() * (1.0 - amount) + toward.red() * amount),
                  qRound(from.green() * (1.0 - amount) + toward.green() * amount),
                  qRound(from.blue() * (1.0 - amount) + toward.blue() * amount));
}

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
    const bool hasGuiApplication = qobject_cast<QGuiApplication *>(QCoreApplication::instance());
    m_fontFamily = configuredFontFamily.isEmpty()
        ? (hasGuiApplication ? QGuiApplication::font().family() : QStringLiteral("sans-serif"))
        : configuredFontFamily;
    m_monoFamily = configuredMonoFamily.isEmpty() ? QStringLiteral("monospace")
                                                   : configuredMonoFamily;
    const auto sourceChanged = [this] { emit this->sourceChanged(); reload(); };
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, sourceChanged);
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, sourceChanged);
    reload();
}

void Theme::setFontFamilies(const QString &fontFamily, const QString &monoFamily)
{
    configuredFontFamily = fontFamily;
    configuredMonoFamily = monoFamily;
}

QColor Theme::hairline() const
{
    return withAlpha(m_foreground, qRound(255 * .08));
}

QColor Theme::hairlineStrong() const
{
    return withAlpha(m_foreground, qRound(255 * .14));
}

QColor Theme::textMuted() const
{
    return withAlpha(m_foreground, qRound(255 * .62));
}

QColor Theme::textFaint() const
{
    return withAlpha(m_foreground, qRound(255 * .40));
}

QColor Theme::accentSoft() const
{
    return withAlpha(m_accent, qRound(255 * .14));
}

QColor Theme::zoomAccent() const
{
    QColor result = m_accent.toHsl();
    const double hue = result.hslHueF() < 0.0 ? 0.0 : result.hslHueF();
    result.setHslF(std::fmod(hue + 40.0 / 360.0, 1.0),
                   result.hslSaturationF(), result.lightnessF(), 1.0);
    return result.toRgb();
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
    if (!lighterBackground.isValid()) lighterBackground = background.lighter(106);
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
    m_record = blend(QColor(QStringLiteral("#ff453a")), accent, .15);
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
