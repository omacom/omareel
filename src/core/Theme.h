#pragma once

#include <QColor>
#include <QFileSystemWatcher>
#include <QObject>
#include <QVariantMap>

namespace OmaRecord {

class Theme : public QObject
{
    Q_OBJECT
#define THEME_COLOR(name, getter) Q_PROPERTY(QColor name READ getter NOTIFY changed)
    THEME_COLOR(accent, accent)
    THEME_COLOR(accentForeground, accentForeground)
    THEME_COLOR(background, background)
    THEME_COLOR(lighterBackground, lighterBackground)
    THEME_COLOR(darkBackground, darkBackground)
    THEME_COLOR(foreground, foreground)
    THEME_COLOR(surface, surface)
    THEME_COLOR(surfaceRaised, surfaceRaised)
    THEME_COLOR(hairline, hairline)
    THEME_COLOR(hairlineStrong, hairlineStrong)
    THEME_COLOR(separator, separator)
    THEME_COLOR(textMuted, textMuted)
    THEME_COLOR(textFaint, textFaint)
    THEME_COLOR(accentSoft, accentSoft)
    THEME_COLOR(zoomAccent, zoomAccent)
    THEME_COLOR(record, record)
    THEME_COLOR(normalFill, normalFill)
    THEME_COLOR(hoverFill, hoverFill)
    THEME_COLOR(selectedFill, selectedFill)
    THEME_COLOR(pressedFill, pressedFill)
    THEME_COLOR(selectionFill, selectionFill)
    THEME_COLOR(normalBorder, normalBorder)
    THEME_COLOR(hoverBorder, hoverBorder)
    THEME_COLOR(selectedBorder, selectedBorder)
    THEME_COLOR(popupBackground, popupBackground)
    THEME_COLOR(popupText, popupText)
    THEME_COLOR(popupBorder, popupBorder)
    THEME_COLOR(tooltipBackground, tooltipBackground)
    THEME_COLOR(tooltipText, tooltipText)
    THEME_COLOR(tooltipBorder, tooltipBorder)
    THEME_COLOR(menuBackground, menuBackground)
    THEME_COLOR(menuText, menuText)
    THEME_COLOR(menuBorder, menuBorder)
    THEME_COLOR(menuScrim, menuScrim)
    THEME_COLOR(menuSelectedBackground, menuSelectedBackground)
    THEME_COLOR(menuSelectedText, menuSelectedText)
#undef THEME_COLOR
    Q_PROPERTY(QString fontFamily READ fontFamily NOTIFY changed)
    Q_PROPERTY(QString monoFamily READ monoFamily NOTIFY changed)
    Q_PROPERTY(QString themeName READ themeName NOTIFY changed)
    Q_PROPERTY(QVariantMap font READ font NOTIFY changed)
    Q_PROPERTY(QVariantMap space READ space NOTIFY changed)
    Q_PROPERTY(int radius READ radius NOTIFY changed)
    Q_PROPERTY(bool rounded READ rounded NOTIFY changed)
    Q_PROPERTY(int normalBorderWidth READ normalBorderWidth NOTIFY changed)
    Q_PROPERTY(int hoverBorderWidth READ hoverBorderWidth NOTIFY changed)
    Q_PROPERTY(int selectedBorderWidth READ selectedBorderWidth NOTIFY changed)
    Q_PROPERTY(bool dark READ dark NOTIFY changed)

public:
    explicit Theme(QObject *parent = nullptr);
    static void setFontFamilies(const QString &fontFamily, const QString &monoFamily);

    QColor accent() const { return m_accent; }
    QColor accentForeground() const;
    QColor background() const { return m_background; }
    QColor lighterBackground() const { return m_lighterBackground; }
    QColor darkBackground() const { return m_darkBackground; }
    QColor foreground() const { return m_foreground; }
    QColor surface() const { return m_background; }
    QColor surfaceRaised() const { return m_lighterBackground; }
    QColor hairline() const;
    QColor hairlineStrong() const;
    QColor separator() const;
    QColor textMuted() const;
    QColor textFaint() const;
    QColor accentSoft() const;
    QColor zoomAccent() const { return m_accent; }
    QColor record() const { return m_record; }
    QColor normalFill() const { return m_normalFill; }
    QColor hoverFill() const { return m_hoverFill; }
    QColor selectedFill() const { return m_selectedFill; }
    QColor pressedFill() const { return m_pressedFill; }
    QColor selectionFill() const { return m_selectionFill; }
    QColor normalBorder() const { return m_normalBorder; }
    QColor hoverBorder() const { return m_hoverBorder; }
    QColor selectedBorder() const { return m_selectedBorder; }
    QColor popupBackground() const { return m_popupBackground; }
    QColor popupText() const { return m_popupText; }
    QColor popupBorder() const { return m_popupBorder; }
    QColor tooltipBackground() const { return m_tooltipBackground; }
    QColor tooltipText() const { return m_tooltipText; }
    QColor tooltipBorder() const { return m_tooltipBorder; }
    QColor menuBackground() const { return m_menuBackground; }
    QColor menuText() const { return m_menuText; }
    QColor menuBorder() const { return m_menuBorder; }
    QColor menuScrim() const { return m_menuScrim; }
    QColor menuSelectedBackground() const { return m_menuSelectedBackground; }
    QColor menuSelectedText() const { return m_menuSelectedText; }
    QString fontFamily() const { return m_fontFamily; }
    QString monoFamily() const { return m_fontFamily; }
    QString themeName() const { return m_themeName; }
    QVariantMap font() const { return m_font; }
    QVariantMap space() const { return m_space; }
    int radius() const { return m_radius; }
    bool rounded() const { return m_radius > 0; }
    int normalBorderWidth() const { return m_normalBorderWidth; }
    int hoverBorderWidth() const { return m_hoverBorderWidth; }
    int selectedBorderWidth() const { return m_selectedBorderWidth; }
    bool dark() const { return m_dark; }

    Q_INVOKABLE QColor controlFill(bool focused, bool hot, bool selected = false) const;
    Q_INVOKABLE QColor controlBorder(bool focused, bool hot, bool selected = false) const;
    Q_INVOKABLE int controlBorderWidth(bool focused, bool hot, bool selected = false) const;

signals:
    void changed();
    void sourceChanged();

private slots:
    void reload();

private:
    void rearmWatcher();
    QFileSystemWatcher m_watcher;
    QColor m_accent, m_background, m_lighterBackground, m_darkBackground, m_foreground, m_record;
    QColor m_normalFill, m_hoverFill, m_selectedFill, m_pressedFill, m_selectionFill;
    QColor m_normalBorder, m_hoverBorder, m_selectedBorder;
    QColor m_popupBackground, m_popupText, m_popupBorder;
    QColor m_tooltipBackground, m_tooltipText, m_tooltipBorder;
    QColor m_menuBackground, m_menuText, m_menuBorder, m_menuScrim;
    QColor m_menuSelectedBackground, m_menuSelectedText;
    QString m_fontFamily = QStringLiteral("monospace");
    QString m_themeName;
    QVariantMap m_font, m_space;
    int m_radius = 0;
    int m_normalBorderWidth = 1, m_hoverBorderWidth = 1, m_selectedBorderWidth = 0;
    bool m_dark = true;
};

} // namespace OmaRecord
