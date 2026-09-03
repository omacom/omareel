#pragma once

#include <QColor>
#include <QFileSystemWatcher>
#include <QObject>
#include <QString>

namespace OmaRecord {

class Theme : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QColor accent READ accent NOTIFY changed)
    Q_PROPERTY(QColor accentForeground READ accentForeground NOTIFY changed)
    Q_PROPERTY(QColor background READ background NOTIFY changed)
    Q_PROPERTY(QColor lighterBackground READ lighterBackground NOTIFY changed)
    Q_PROPERTY(QColor darkBackground READ darkBackground NOTIFY changed)
    Q_PROPERTY(QColor foreground READ foreground NOTIFY changed)
    Q_PROPERTY(QColor surface READ surface NOTIFY changed)
    Q_PROPERTY(QColor surfaceRaised READ surfaceRaised NOTIFY changed)
    Q_PROPERTY(QColor hairline READ hairline NOTIFY changed)
    Q_PROPERTY(QColor hairlineStrong READ hairlineStrong NOTIFY changed)
    Q_PROPERTY(QColor textMuted READ textMuted NOTIFY changed)
    Q_PROPERTY(QColor textFaint READ textFaint NOTIFY changed)
    Q_PROPERTY(QColor accentSoft READ accentSoft NOTIFY changed)
    Q_PROPERTY(QColor record READ record NOTIFY changed)
    Q_PROPERTY(QString fontFamily READ fontFamily CONSTANT)
    Q_PROPERTY(QString monoFamily READ monoFamily CONSTANT)
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
    QColor textMuted() const;
    QColor textFaint() const;
    QColor accentSoft() const;
    QColor record() const { return m_record; }
    QString fontFamily() const { return m_fontFamily; }
    QString monoFamily() const { return m_monoFamily; }
    bool dark() const { return m_dark; }

signals:
    void changed();
    void sourceChanged();

private slots:
    void reload();

private:
    void rearmWatcher();
    QFileSystemWatcher m_watcher;
    QColor m_accent = QColor(QStringLiteral("#7aa2f7"));
    QColor m_background = QColor(QStringLiteral("#1a1b26"));
    QColor m_lighterBackground = QColor(QStringLiteral("#24283b"));
    QColor m_darkBackground = QColor(QStringLiteral("#14151d"));
    QColor m_foreground = QColor(QStringLiteral("#c0caf5"));
    QColor m_record = QColor(QStringLiteral("#f55251"));
    QString m_fontFamily;
    QString m_monoFamily;
    bool m_dark = true;
};

} // namespace OmaRecord
