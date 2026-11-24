#pragma once

#include <QDBusConnection>
#include <QDBusMessage>
#include <QObject>
#include <QStringList>
#include <QUrl>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <optional>

class QDBusPendingCallWatcher;
class QDBusServiceWatcher;

namespace Omareel {

// Open and save dialogs through xdg-desktop-portal's FileChooser over D-Bus,
// so they always use the desktop's own chooser rather than falling back to
// Qt Quick's non-native QML FileDialog.
// Adapted from Plain, Omacut, and Omawrite (MIT, David Heinemeier Hansson).
class PortalFilePicker : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(Mode mode READ mode WRITE setMode NOTIFY modeChanged)
    Q_PROPERTY(QString title READ title WRITE setTitle NOTIFY titleChanged)
    Q_PROPERTY(QString currentFolder READ currentFolder WRITE setCurrentFolder NOTIFY currentFolderChanged)
    Q_PROPERTY(QString suggestedPath READ suggestedPath WRITE setSuggestedPath NOTIFY suggestedPathChanged)
    Q_PROPERTY(QString defaultSuffix READ defaultSuffix WRITE setDefaultSuffix NOTIFY defaultSuffixChanged)
    Q_PROPERTY(QStringList nameFilters READ nameFilters WRITE setNameFilters NOTIFY nameFiltersChanged)
    Q_PROPERTY(QUrl selectedFile READ selectedFile NOTIFY selectedFileChanged)
    Q_PROPERTY(QString selectedPath READ selectedPath NOTIFY selectedFileChanged)

public:
    enum class Mode {
        OpenFile,
        SaveFile,
        OpenFolder
    };
    Q_ENUM(Mode)

    explicit PortalFilePicker(QObject *parent = nullptr);
    explicit PortalFilePicker(const QDBusConnection &bus, QObject *parent = nullptr);
    ~PortalFilePicker() override;

    // Tests inject a private D-Bus connection without touching the session bus.
    void setConnection(const QDBusConnection &bus);

    Mode mode() const { return m_mode; }
    void setMode(Mode mode);

    QString title() const { return m_title; }
    void setTitle(const QString &title);

    QString currentFolder() const { return m_currentFolder; }
    void setCurrentFolder(const QString &currentFolder);

    QString suggestedPath() const { return m_suggestedPath; }
    void setSuggestedPath(const QString &suggestedPath);

    QString defaultSuffix() const { return m_defaultSuffix; }
    void setDefaultSuffix(const QString &defaultSuffix);

    QStringList nameFilters() const { return m_nameFilters; }
    void setNameFilters(const QStringList &nameFilters);

    QUrl selectedFile() const { return m_selectedFile; }
    QString selectedPath() const { return m_selectedPath; }

    Q_INVOKABLE void open();

signals:
    void modeChanged();
    void titleChanged();
    void currentFolderChanged();
    void suggestedPathChanged();
    void defaultSuffixChanged();
    void nameFiltersChanged();
    void selectedFileChanged();
    void accepted(const QUrl &url);
    void cancelled();
    void failed(const QString &message);

private slots:
    void handleResponse(uint response, const QVariantMap &results, const QDBusMessage &message);

private:
    QDBusConnection activeBus() const;
    void requestFile(const QString &method, const QString &title, QVariantMap options,
                     Mode mode, const QString &defaultSuffix);
    bool connectToRequestPath(const QString &path);
    void clearPending();
    void closePendingRequest();

    std::optional<QDBusConnection> m_injectedBus;
    std::optional<QDBusConnection> m_pendingBus;

    Mode m_mode = Mode::OpenFile;
    QString m_title;
    QString m_currentFolder;
    QString m_suggestedPath;
    QString m_defaultSuffix;
    QStringList m_nameFilters;
    QUrl m_selectedFile;
    QString m_selectedPath;

    bool m_pending = false;
    Mode m_pendingMode = Mode::OpenFile;
    QString m_pendingDefaultSuffix;
    QString m_pendingPath;
    // Bumped per request and on clear so late replies/owner changes are ignored.
    int m_pendingGeneration = 0;
    QDBusPendingCallWatcher *m_pendingWatcher = nullptr;
    QDBusServiceWatcher *m_ownerWatcher = nullptr;
};

} // namespace Omareel
