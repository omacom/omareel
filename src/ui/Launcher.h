#pragma once

#include <QDateTime>
#include <QObject>
#include <QVariantList>

namespace OmaRecord {

class Launcher : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantList recentBundles READ recentBundles NOTIFY recentBundlesChanged)
public:
    explicit Launcher(QObject *parent = nullptr);
    QVariantList recentBundles() const { return m_recentBundles; }

    Q_INVOKABLE void refresh();
    Q_INVOKABLE void openBundle(const QString &path);
    Q_INVOKABLE void record(const QString &mode);

signals:
    void recentBundlesChanged();
    void quitRequested();
    void errorOccurred(const QString &message);

private:
    static QString formatDuration(double seconds);
    static QString formatDate(const QDateTime &dateTime);
    void probeDurationAsync(const QString &bundlePath);
    QVariantList m_recentBundles;
};

} // namespace OmaRecord
