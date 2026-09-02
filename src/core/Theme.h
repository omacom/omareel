#pragma once

#include <QColor>
#include <QFileSystemWatcher>
#include <QObject>

namespace OmaRecord {

class Theme : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QColor accent READ accent NOTIFY changed)
    Q_PROPERTY(QColor accentForeground READ accentForeground NOTIFY changed)
    Q_PROPERTY(QColor background READ background NOTIFY changed)
    Q_PROPERTY(QColor lighterBackground READ lighterBackground NOTIFY changed)
    Q_PROPERTY(QColor foreground READ foreground NOTIFY changed)
public:
    explicit Theme(QObject *parent = nullptr);
    QColor accent() const { return m_accent; }
    QColor accentForeground() const { return m_accentForeground; }
    QColor background() const { return m_background; }
    QColor lighterBackground() const { return m_lighterBackground; }
    QColor foreground() const { return m_foreground; }

signals:
    void changed();

private slots:
    void reload();

private:
    void rearmWatcher();
    QFileSystemWatcher m_watcher;
    QColor m_accent = QColor(QStringLiteral("#7aa2f7"));
    QColor m_accentForeground = QColor(QStringLiteral("#101116"));
    QColor m_background = QColor(QStringLiteral("#1a1b26"));
    QColor m_lighterBackground = QColor(QStringLiteral("#24283b"));
    QColor m_foreground = QColor(QStringLiteral("#c0caf5"));
};

} // namespace OmaRecord
