#include "Theme.h"
#include "OmarchyPaths.h"

#include <QDir>
#include <QCoreApplication>
#include <QFile>
#include <QFileInfo>
#include <QFont>
#include <QGuiApplication>
#include <QHash>
#include <QProcess>
#include <QRegularExpression>
#include <QTimer>
#include <QtMath>

using namespace OmaRecord;

static QString configuredFontFamily;

static QColor alphaColor(const QColor &color, double alpha)
{
    QColor result = color;
    result.setAlphaF(qBound(0.0, alpha, 1.0));
    return result;
}

static double luminance(const QColor &color)
{
    const auto linear = [](double channel) {
        channel /= 255.0;
        return channel <= 0.04045 ? channel / 12.92 : qPow((channel + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * linear(color.red()) + 0.7152 * linear(color.green())
        + 0.0722 * linear(color.blue());
}

static QHash<QString, QString> readToml(const QString &path)
{
    QHash<QString, QString> values;
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) return values;
    QString section;
    const QRegularExpression sectionPattern(QStringLiteral(R"(^\s*\[([^]]+)\])"));
    const QRegularExpression valuePattern(
        QStringLiteral(R"(^\s*([A-Za-z0-9_-]+)\s*=\s*(?:["']([^"']*)["']|([^#\s][^#]*?))\s*(?:#.*)?$)"));
    while (!file.atEnd()) {
        const QString line = QString::fromUtf8(file.readLine());
        const auto sectionMatch = sectionPattern.match(line);
        if (sectionMatch.hasMatch()) { section = sectionMatch.captured(1); continue; }
        const auto valueMatch = valuePattern.match(line);
        if (!valueMatch.hasMatch()) continue;
        const QString raw = valueMatch.captured(2).isNull() ? valueMatch.captured(3)
                                                            : valueMatch.captured(2);
        values.insert(section + QLatin1Char('.') + valueMatch.captured(1), raw.trimmed());
    }
    return values;
}

static QColor roleColor(QString value, const QColor &foreground, const QColor &background,
                        const QColor &accent, const QColor &attention, const QColor &fallback)
{
    value = value.trimmed();
    const QString role = value.toLower();
    if (role == QLatin1String("foreground") || role == QLatin1String("text")
        || role == QLatin1String("hyprland.active-border-foreground")) return foreground;
    if (role == QLatin1String("background")) return background;
    if (role == QLatin1String("accent") || role == QLatin1String("hyprland.active-border")) return accent;
    if (role == QLatin1String("urgent")) return attention;
    const QColor parsed(value.section(QLatin1Char(' '), 0, 0));
    return parsed.isValid() ? parsed : fallback;
}

Theme::Theme(QObject *parent) : QObject(parent)
{
    if (!configuredFontFamily.isEmpty()) m_fontFamily = configuredFontFamily;
    const auto sourceChanged = [this] {
        emit this->sourceChanged();
        QTimer::singleShot(220, this, &Theme::reload);
    };
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, sourceChanged);
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, sourceChanged);
    reload();
}

void Theme::setFontFamilies(const QString &fontFamily, const QString &)
{
    configuredFontFamily = fontFamily;
}

QColor Theme::hairline() const { return alphaColor(m_foreground, .08); }
QColor Theme::hairlineStrong() const { return alphaColor(m_foreground, .14); }
QColor Theme::separator() const { return alphaColor(m_foreground, .12); }
QColor Theme::textMuted() const { return m_foreground.darker(140); }
QColor Theme::textFaint() const { return m_foreground.darker(160); }
QColor Theme::accentSoft() const { return alphaColor(m_accent, .18); }

QColor Theme::accentForeground() const
{
    return luminance(m_accent) > 0.179 ? QColor(16, 17, 22) : QColor(255, 255, 255);
}

QColor Theme::controlFill(bool focused, bool hot, bool selected) const
{
    return selected ? m_selectedFill : (focused || hot ? m_hoverFill : m_normalFill);
}

QColor Theme::controlBorder(bool focused, bool hot, bool selected) const
{
    return selected ? m_selectedBorder : (focused || hot ? m_hoverBorder : m_normalBorder);
}

int Theme::controlBorderWidth(bool focused, bool hot, bool selected) const
{
    return selected ? m_selectedBorderWidth : (focused || hot ? m_hoverBorderWidth : m_normalBorderWidth);
}

void Theme::reload()
{
    const QString themePath = QDir(OmarchyPaths::stateRoot()).filePath(QStringLiteral("theme"));
    const auto colors = readToml(QDir(themePath).filePath(QStringLiteral("colors.toml")));
    const auto shell = readToml(QDir(themePath).filePath(QStringLiteral("shell.toml")));
    const QColor accent(colors.value(QStringLiteral(".accent"), QStringLiteral("#7aa2f7")));
    const QColor background(colors.value(QStringLiteral(".background"), QStringLiteral("#1a1b26")));
    const QColor foreground(colors.value(QStringLiteral(".foreground"), QStringLiteral("#a9b1d6")));
    const QColor lighter = colors.contains(QStringLiteral(".lighter_background"))
        ? QColor(colors.value(QStringLiteral(".lighter_background"))) : background.lighter(106);
    const QColor darker = colors.contains(QStringLiteral(".dark_background"))
        ? QColor(colors.value(QStringLiteral(".dark_background")))
        : QColor(qRound(background.red() * .75), qRound(background.green() * .75),
                 qRound(background.blue() * .75));
    const QColor attention = roleColor(shell.value(QStringLiteral("bar.active"),
                                                   colors.value(QStringLiteral(".red"))),
                                           foreground, background, accent, accent, accent);
    const auto number = [&shell](const QString &key, double fallback) {
        bool ok = false;
        const double result = shell.value(key).toDouble(&ok);
        return ok ? result : fallback;
    };
    const auto color = [&](const QString &key, const QColor &fallback) {
        return roleColor(shell.value(key), foreground, background, accent, attention, fallback);
    };
    const auto surface = [&](const QString &key, const QColor &fallback, double fallbackAlpha) {
        return alphaColor(color(key, fallback), number(key + QStringLiteral("-alpha"), fallbackAlpha));
    };

    m_accent = accent;
    m_background = background;
    m_foreground = foreground;
    m_lighterBackground = lighter;
    m_darkBackground = darker;
    m_record = attention;
    m_dark = colors.value(QStringLiteral(".mode")) == QLatin1String("dark") || luminance(background) < .35;
    m_themeName = OmarchyPaths::currentThemeName();

    const QColor normal = color(QStringLiteral("controls.normal-color"), foreground);
    const QColor hover = color(QStringLiteral("controls.hover-cursor-color"), foreground);
    const QColor selected = color(QStringLiteral("controls.selected-color"), foreground);
    m_normalFill = alphaColor(normal, number(QStringLiteral("controls.normal-fill-alpha"), .04));
    m_hoverFill = alphaColor(hover, number(QStringLiteral("controls.hover-cursor-fill-alpha"), .08));
    m_selectedFill = alphaColor(selected, number(QStringLiteral("controls.selected-fill-alpha"), .18));
    m_pressedFill = alphaColor(color(QStringLiteral("controls.pressed-color"), hover),
                               number(QStringLiteral("controls.pressed-fill-alpha"), .22));
    m_selectionFill = alphaColor(color(QStringLiteral("controls.selection-color"), foreground),
                                 number(QStringLiteral("controls.selection-fill-alpha"), .35));
    m_normalBorder = alphaColor(color(QStringLiteral("controls.normal-border"), normal),
                                number(QStringLiteral("controls.normal-border-alpha"), .40));
    m_hoverBorder = alphaColor(color(QStringLiteral("controls.hover-cursor-border"), hover),
                               number(QStringLiteral("controls.hover-cursor-border-alpha"), .25));
    m_selectedBorder = alphaColor(color(QStringLiteral("controls.selected-border"), selected),
                                  number(QStringLiteral("controls.selected-border-alpha"), 1));
    m_normalBorderWidth = qMax(0, qRound(number(QStringLiteral("controls.normal-border-width"), 1)));
    m_hoverBorderWidth = qMax(0, qRound(number(QStringLiteral("controls.hover-cursor-border-width"), 1)));
    m_selectedBorderWidth = qMax(0, qRound(number(QStringLiteral("controls.selected-border-width"), 0)));

    m_popupBackground = surface(QStringLiteral("popups.background"), background, 1);
    m_popupText = color(QStringLiteral("popups.text"), foreground);
    m_popupBorder = surface(QStringLiteral("popups.border"), accent, 1);
    m_tooltipBackground = surface(QStringLiteral("tooltip.background"), background, .97);
    m_tooltipText = color(QStringLiteral("tooltip.text"), foreground);
    m_tooltipBorder = surface(QStringLiteral("tooltip.border"), foreground, 1);
    m_menuBackground = surface(QStringLiteral("menu.background"), background, 1);
    m_menuText = color(QStringLiteral("menu.text"), foreground);
    m_menuBorder = surface(QStringLiteral("menu.border"), foreground, 1);
    m_menuScrim = surface(QStringLiteral("menu.scrim"), background, .5);
    m_menuSelectedBackground = surface(QStringLiteral("menu.selected-background"), foreground, .08);
    m_menuSelectedText = color(QStringLiteral("menu.selected-text"), accent);

    const int base = qMax(1, qRound(number(QStringLiteral("font.base-size"), 12)));
    QProcess fontMatch;
    fontMatch.start(QStringLiteral("fc-match"),
                    {QStringLiteral("-f"), QStringLiteral("%{family[0]}"), QStringLiteral("monospace")});
    if (fontMatch.waitForFinished(1200) && fontMatch.exitCode() == 0) {
        const QString family = QString::fromUtf8(fontMatch.readAllStandardOutput()).trimmed();
        if (!family.isEmpty()) m_fontFamily = family;
    }
    if (qobject_cast<QGuiApplication *>(QCoreApplication::instance())) {
        QFont applicationFont(m_fontFamily);
        applicationFont.setPixelSize(base);
        QGuiApplication::setFont(applicationFont);
    }
    const auto fontToken = [&](const QString &key, double multiplier) {
        bool ok = false;
        const int explicitValue = shell.value(QStringLiteral("font.") + key).toInt(&ok);
        return ok ? qMax(1, explicitValue) : qMax(1, qRound(base * multiplier));
    };
    m_font = {{QStringLiteral("base"), base},
              {QStringLiteral("caption"), fontToken(QStringLiteral("caption"), .833)},
              {QStringLiteral("bodySmall"), fontToken(QStringLiteral("body-small"), .917)},
              {QStringLiteral("body"), fontToken(QStringLiteral("body"), 1)},
              {QStringLiteral("subtitle"), fontToken(QStringLiteral("subtitle"), 1.083)},
              {QStringLiteral("title"), fontToken(QStringLiteral("title"), 1.167)},
              {QStringLiteral("heading"), fontToken(QStringLiteral("heading"), 1.333)},
              {QStringLiteral("display"), fontToken(QStringLiteral("display"), 2)}};

    const bool scaleWithFont = shell.value(QStringLiteral("spacing.scale-with-font"),
                                           QStringLiteral("true")) != QLatin1String("false");
    const double spacingScale = number(QStringLiteral("spacing.scale"), 1) * (scaleWithFont ? base / 12.0 : 1);
    const auto spacing = [&](const QString &key, int fallback) {
        bool ok = false;
        const double explicitValue = shell.value(QStringLiteral("spacing.") + key).toDouble(&ok);
        return qMax(0, qRound(ok ? explicitValue : fallback * spacingScale));
    };
    m_space = {{QStringLiteral("xxs"), spacing(QStringLiteral("xxs"), 2)},
               {QStringLiteral("xs"), spacing(QStringLiteral("xs"), 3)},
               {QStringLiteral("sm"), spacing(QStringLiteral("sm"), 4)},
               {QStringLiteral("md"), spacing(QStringLiteral("md"), 6)},
               {QStringLiteral("lg"), spacing(QStringLiteral("lg"), 8)},
               {QStringLiteral("xl"), spacing(QStringLiteral("xl"), 10)},
               {QStringLiteral("xxl"), spacing(QStringLiteral("xxl"), 12)},
               {QStringLiteral("xxxl"), spacing(QStringLiteral("xxxl"), 14)},
               {QStringLiteral("huge"), spacing(QStringLiteral("huge"), 18)},
               {QStringLiteral("controlGap"), spacing(QStringLiteral("control-gap"), 8)},
               {QStringLiteral("controlPaddingX"), spacing(QStringLiteral("control-padding-x"), 10)},
               {QStringLiteral("controlPaddingY"), spacing(QStringLiteral("control-padding-y"), 6)},
               {QStringLiteral("controlHeight"), spacing(QStringLiteral("control-height"), 28)},
               {QStringLiteral("popupRowHeight"), spacing(QStringLiteral("popup-row-height"), 28)},
               {QStringLiteral("rowGap"), spacing(QStringLiteral("row-gap"), 8)},
               {QStringLiteral("rowPaddingX"), spacing(QStringLiteral("row-padding-x"), 12)},
               {QStringLiteral("panelGap"), spacing(QStringLiteral("panel-gap"), 14)},
               {QStringLiteral("panelPadding"), spacing(QStringLiteral("panel-padding"), 18)},
               {QStringLiteral("popupPadding"), spacing(QStringLiteral("popup-padding"), 14)}};

    QProcess process;
    process.start(QStringLiteral("hyprctl"), {QStringLiteral("getoption"),
                                               QStringLiteral("decoration:rounding"), QStringLiteral("-j")});
    if (process.waitForFinished(1200) && process.exitCode() == 0) {
        const auto match = QRegularExpression(QStringLiteral(R"("int"\s*:\s*(\d+))"))
                               .match(QString::fromUtf8(process.readAllStandardOutput()));
        if (match.hasMatch()) m_radius = match.captured(1).toInt();
    }
    rearmWatcher();
    emit changed();
}

void Theme::rearmWatcher()
{
    if (!m_watcher.files().isEmpty()) m_watcher.removePaths(m_watcher.files());
    if (!m_watcher.directories().isEmpty()) m_watcher.removePaths(m_watcher.directories());
    const QString current = OmarchyPaths::stateRoot();
    const QString theme = QDir(current).filePath(QStringLiteral("theme"));
    QStringList paths{QFileInfo(current).absolutePath(), current, theme,
                      QDir(theme).filePath(QStringLiteral("colors.toml")),
                      QDir(theme).filePath(QStringLiteral("shell.toml"))};
    paths.removeDuplicates();
    for (auto it = paths.begin(); it != paths.end();) {
        if (!QFileInfo::exists(*it)) it = paths.erase(it); else ++it;
    }
    if (!paths.isEmpty()) m_watcher.addPaths(paths);
}
