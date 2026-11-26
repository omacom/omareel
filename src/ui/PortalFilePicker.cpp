// Adapted from Plain, Omacut, and Omawrite's portal file picker (MIT, David Heinemeier Hansson).
#include "PortalFilePicker.h"

#include <QDBusArgument>
#include <QDBusError>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusObjectPath>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusServiceWatcher>
#include <QDir>
#include <QFileInfo>
#include <QRandomGenerator>
#include <QRegularExpression>

namespace {

const QString PortalService = QStringLiteral("org.freedesktop.portal.Desktop");
const QString PortalObjectPath = QStringLiteral("/org/freedesktop/portal/desktop");
const QString FileChooserInterface = QStringLiteral("org.freedesktop.portal.FileChooser");
const QString RequestInterface = QStringLiteral("org.freedesktop.portal.Request");

struct PortalFilterRule {
    uint type;
    QString pattern;
};

using PortalFilterRules = QList<PortalFilterRule>;

struct PortalFileFilter {
    QString name;
    PortalFilterRules rules;
};

using PortalFileFilters = QList<PortalFileFilter>;

QDBusArgument &operator<<(QDBusArgument &argument, const PortalFilterRule &rule)
{
    argument.beginStructure();
    argument << rule.type << rule.pattern;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, PortalFilterRule &rule)
{
    argument.beginStructure();
    argument >> rule.type >> rule.pattern;
    argument.endStructure();
    return argument;
}

QDBusArgument &operator<<(QDBusArgument &argument, const PortalFileFilter &filter)
{
    argument.beginStructure();
    argument << filter.name << filter.rules;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, PortalFileFilter &filter)
{
    argument.beginStructure();
    argument >> filter.name >> filter.rules;
    argument.endStructure();
    return argument;
}

void registerPortalFilterTypes()
{
    static const bool registered = [] {
        qDBusRegisterMetaType<PortalFilterRule>();
        qDBusRegisterMetaType<PortalFilterRules>();
        qDBusRegisterMetaType<PortalFileFilter>();
        qDBusRegisterMetaType<PortalFileFilters>();
        return true;
    }();
    Q_UNUSED(registered);
}

// Messages built directly so a hung portal never blocks the GUI thread on
// QDBusInterface's synchronous Introspect call (including from ~PortalFilePicker).
QDBusMessage portalCall(const QString &path, const QString &interface, const QString &method)
{
    return QDBusMessage::createMethodCall(PortalService, path, interface, method);
}

QString portalToken()
{
    return QStringLiteral("omareel_%1").arg(QRandomGenerator::global()->generate64());
}

QString existingFolderOrHome(const QString &folder)
{
    return !folder.isEmpty() && QDir(folder).exists() ? folder : QDir::homePath();
}

// The portal takes folders as NUL-terminated UTF-8 byte strings.
QByteArray portalPathBytes(const QString &path)
{
    QByteArray bytes = path.toUtf8();
    bytes.append('\0');
    return bytes;
}

// Parses Qt-style name filters such as "Videos (*.mp4 *.mov)" into portal filters.
PortalFileFilters parseNameFilters(const QStringList &nameFilters)
{
    static const QRegularExpression whitespace(QStringLiteral("\\s+"));
    PortalFileFilters filters;
    for (const QString &raw : nameFilters) {
        const QString entry = raw.trimmed();
        if (entry.isEmpty())
            continue;
        const qsizetype openParen = entry.lastIndexOf(QLatin1Char('('));
        const qsizetype closeParen = entry.lastIndexOf(QLatin1Char(')'));
        QString label;
        QStringList patterns;
        if (openParen >= 0 && closeParen > openParen) {
            label = entry.left(openParen).trimmed();
            patterns = entry.mid(openParen + 1, closeParen - openParen - 1)
                           .split(whitespace, Qt::SkipEmptyParts);
        } else {
            patterns = entry.split(whitespace, Qt::SkipEmptyParts);
        }
        PortalFilterRules rules;
        for (const QString &pattern : patterns)
            rules.append({0u, pattern});
        if (rules.isEmpty())
            continue;
        filters.append({label.isEmpty() ? entry : label, rules});
    }
    return filters;
}

} // namespace

namespace Omareel {

PortalFilePicker::PortalFilePicker(QObject *parent)
    : QObject(parent)
{
    registerPortalFilterTypes();
}

PortalFilePicker::PortalFilePicker(const QDBusConnection &bus, QObject *parent)
    : QObject(parent), m_injectedBus(bus)
{
    registerPortalFilterTypes();
}

PortalFilePicker::~PortalFilePicker()
{
    closePendingRequest();
    clearPending();
}

void PortalFilePicker::setConnection(const QDBusConnection &bus)
{
    m_injectedBus = bus;
}

void PortalFilePicker::setMode(Mode mode)
{
    if (m_mode == mode)
        return;
    m_mode = mode;
    emit modeChanged();
}

void PortalFilePicker::setTitle(const QString &title)
{
    if (m_title == title)
        return;
    m_title = title;
    emit titleChanged();
}

void PortalFilePicker::setCurrentFolder(const QString &currentFolder)
{
    if (m_currentFolder == currentFolder)
        return;
    m_currentFolder = currentFolder;
    emit currentFolderChanged();
}

void PortalFilePicker::setSuggestedPath(const QString &suggestedPath)
{
    if (m_suggestedPath == suggestedPath)
        return;
    m_suggestedPath = suggestedPath;
    emit suggestedPathChanged();
}

void PortalFilePicker::setNameFilters(const QStringList &nameFilters)
{
    if (m_nameFilters == nameFilters)
        return;
    m_nameFilters = nameFilters;
    emit nameFiltersChanged();
}

void PortalFilePicker::open()
{
    QVariantMap options;
    options.insert(QStringLiteral("modal"), true);

    if (m_mode == Mode::SaveFile) {
        QString suggested = m_suggestedPath;
        if (suggested.startsWith(QLatin1String("file://"))) {
            const QUrl suggestedUrl(suggested);
            if (suggestedUrl.isLocalFile())
                suggested = suggestedUrl.toLocalFile();
        }
        QString folder = m_currentFolder;
        QString fileName;
        if (!suggested.isEmpty()) {
            const QFileInfo target(suggested);
            folder = target.absolutePath();
            fileName = target.fileName();
        }
        if (fileName.isEmpty())
            fileName = tr("Untitled");
        options.insert(QStringLiteral("accept_label"), tr("Save"));
        options.insert(QStringLiteral("current_folder"),
                       portalPathBytes(existingFolderOrHome(folder)));
        options.insert(QStringLiteral("current_name"), fileName);
        const PortalFileFilters filters = parseNameFilters(m_nameFilters);
        if (!filters.isEmpty()) {
            options.insert(QStringLiteral("filters"), QVariant::fromValue(filters));
            options.insert(QStringLiteral("current_filter"), QVariant::fromValue(filters.first()));
        }
        requestFile(QStringLiteral("SaveFile"),
                    m_title.isEmpty() ? tr("Save File") : m_title,
                    options, Mode::SaveFile);
        return;
    }

    options.insert(QStringLiteral("accept_label"), tr("Open"));
    options.insert(QStringLiteral("multiple"), false);
    options.insert(QStringLiteral("current_folder"),
                   portalPathBytes(existingFolderOrHome(m_currentFolder)));

    if (m_mode == Mode::OpenFolder) {
        options.insert(QStringLiteral("directory"), true);
        requestFile(QStringLiteral("OpenFile"),
                    m_title.isEmpty() ? tr("Open Folder") : m_title,
                    options, Mode::OpenFolder);
        return;
    }

    const PortalFileFilters filters = parseNameFilters(m_nameFilters);
    if (!filters.isEmpty()) {
        options.insert(QStringLiteral("filters"), QVariant::fromValue(filters));
        options.insert(QStringLiteral("current_filter"), QVariant::fromValue(filters.first()));
    }
    requestFile(QStringLiteral("OpenFile"),
                m_title.isEmpty() ? tr("Open File") : m_title,
                options, Mode::OpenFile);
}

QDBusConnection PortalFilePicker::activeBus() const
{
    return m_injectedBus ? *m_injectedBus : QDBusConnection::sessionBus();
}

bool PortalFilePicker::connectToRequestPath(const QString &path)
{
    m_pendingPath = path;
    return m_pendingBus && m_pendingBus->connect(
        PortalService, m_pendingPath,
        RequestInterface, QStringLiteral("Response"),
        this, SLOT(handleResponse(uint,QVariantMap,QDBusMessage)));
}

void PortalFilePicker::closePendingRequest()
{
    if (m_pendingPath.isEmpty() || !m_pendingBus)
        return;
    m_pendingBus->send(portalCall(m_pendingPath, RequestInterface, QStringLiteral("Close")));
}

void PortalFilePicker::requestFile(const QString &method, const QString &title,
                                   QVariantMap options, Mode mode)
{
    // Replace any pending request (another dialog still up, or a portal that
    // crashed before answering) by closing it best-effort first.
    if (m_pendingBus) {
        closePendingRequest();
        clearPending();
    }

    const QDBusConnection bus = activeBus();
    if (!bus.isConnected()) {
        emit failed(tr("The desktop's file chooser (xdg-desktop-portal) is not available."));
        return;
    }

    // Subscribe to Response at the request path derived from handle_token
    // before making the call, so a reply cannot slip past while the match rule
    // is still being installed.
    const QString token = portalToken();
    options.insert(QStringLiteral("handle_token"), token);
    const QString base = bus.baseService();
    QString sender = base.startsWith(QLatin1Char(':')) ? base.mid(1) : base;
    sender.replace(QLatin1Char('.'), QLatin1Char('_'));
    const QString predictedPath =
        QStringLiteral("/org/freedesktop/portal/desktop/request/%1/%2").arg(sender, token);

    m_pendingBus = bus;
    m_pendingMode = mode;
    if (!connectToRequestPath(predictedPath)) {
        m_pendingPath.clear();
        m_pendingBus.reset();
        emit failed(tr("Could not listen for the file chooser's answer."));
        return;
    }
    emit busyChanged();

    const int generation = ++m_pendingGeneration;
    m_ownerWatcher = new QDBusServiceWatcher(
        PortalService, bus, QDBusServiceWatcher::WatchForOwnerChange, this);
    connect(m_ownerWatcher, &QDBusServiceWatcher::serviceOwnerChanged, this,
            [this, generation](const QString &, const QString &oldOwner, const QString &) {
                // Ignore empty oldOwner so D-Bus auto-activation isn't treated as a crash.
                if (generation != m_pendingGeneration || !m_pendingBus || oldOwner.isEmpty())
                    return;
                clearPending();
                emit failed(tr("The desktop's file chooser (xdg-desktop-portal) is not available."));
            });

    QDBusMessage call = portalCall(PortalObjectPath, FileChooserInterface, method);
    call << QString() << title << options;
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(call), this);
    m_pendingWatcher = watcher;

    connect(watcher, &QDBusPendingCallWatcher::finished, this,
            [this, watcher, generation, bus, predictedPath]() {
        QDBusPendingReply<QDBusObjectPath> reply = *watcher;
        watcher->deleteLater();
        if (m_pendingWatcher == watcher)
            m_pendingWatcher = nullptr;

        // Superseded by a newer request, or already answered while in flight.
        // If a legacy portal returned a non-predicted path for a superseded request,
        // send a non-blocking Close there so it does not stay open.
        if (generation != m_pendingGeneration || !m_pendingBus) {
            if (!reply.isError()) {
                const QString actualPath = reply.value().path();
                if (!actualPath.isEmpty() && actualPath != predictedPath) {
                    QDBusConnection closeBus = bus;
                    closeBus.send(portalCall(actualPath, RequestInterface, QStringLiteral("Close")));
                }
            }
            return;
        }

        if (reply.isError()) {
            clearPending();
            const QDBusError::ErrorType type = reply.error().type();
            if (type == QDBusError::ServiceUnknown || type == QDBusError::UnknownObject
                || type == QDBusError::UnknownInterface)
                emit failed(tr("The desktop's file chooser (xdg-desktop-portal) is not available."));
            else
                emit failed(tr("The file chooser failed: %1").arg(reply.error().message()));
            return;
        }

        // Old portal versions can return a different request path than
        // handle_token predicts; move the subscription over if so.
        const QString actualPath = reply.value().path();
        if (actualPath.isEmpty()) {
            clearPending();
            emit failed(tr("Could not listen for the file chooser's answer."));
            return;
        }
        if (actualPath != m_pendingPath) {
            m_pendingBus->disconnect(
                PortalService, m_pendingPath,
                RequestInterface, QStringLiteral("Response"),
                this, SLOT(handleResponse(uint,QVariantMap,QDBusMessage)));
            if (!connectToRequestPath(actualPath)) {
                closePendingRequest();
                clearPending();
                emit failed(tr("Could not listen for the file chooser's answer."));
            }
        }
    });
}

void PortalFilePicker::handleResponse(uint response, const QVariantMap &results,
                                      const QDBusMessage &message)
{
    if (!m_pendingBus || message.path() != m_pendingPath)
        return;

    const Mode activeMode = m_pendingMode;
    clearPending();

    if (response == 1) {
        emit cancelled();
        return;
    }

    const QStringList uris = results.value(QStringLiteral("uris")).toStringList();
    if (response != 0 || uris.isEmpty()) {
        emit failed(activeMode == Mode::OpenFolder
                        ? tr("The file chooser did not return a folder.")
                        : tr("The file chooser did not return a file."));
        return;
    }

    const QUrl url(uris.first());
    if (!url.isValid() || url.isEmpty()) {
        emit failed(activeMode == Mode::OpenFolder
                        ? tr("The file chooser did not return a folder.")
                        : tr("The file chooser did not return a file."));
        return;
    }

    const QString path = url.isLocalFile() ? url.toLocalFile() : url.toString();
    if (m_selectedFile != url || m_selectedPath != path) {
        m_selectedFile = url;
        m_selectedPath = path;
        emit selectedFileChanged();
    }
    emit accepted(url);
}

void PortalFilePicker::clearPending()
{
    const bool wasBusy = m_pendingBus.has_value();
    ++m_pendingGeneration;
    m_pendingWatcher = nullptr;
    if (m_ownerWatcher) {
        m_ownerWatcher->disconnect(this);
        m_ownerWatcher->deleteLater();
        m_ownerWatcher = nullptr;
    }
    if (!m_pendingPath.isEmpty() && m_pendingBus) {
        m_pendingBus->disconnect(
            PortalService, m_pendingPath,
            RequestInterface, QStringLiteral("Response"),
            this, SLOT(handleResponse(uint,QVariantMap,QDBusMessage)));
    }
    m_pendingPath.clear();
    m_pendingBus.reset();
    if (wasBusy)
        emit busyChanged();
}

} // namespace Omareel
