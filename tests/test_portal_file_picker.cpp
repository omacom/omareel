#include "ui/PortalFilePicker.h"
#include "omareel_qt/src/theme.cxxqt.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QDir>
#include <QProcess>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQmlError>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QUrl>
#include <QtTest>

#include <memory>

using namespace Omareel;

namespace {

struct DecodedFilter {
    QString name;
    QList<QPair<uint, QString>> rules;
};

DecodedFilter readFilter(const QDBusArgument &arg)
{
    DecodedFilter out;
    arg.beginStructure();
    arg >> out.name;
    arg.beginArray();
    while (!arg.atEnd()) {
        uint type = 0;
        QString pattern;
        arg.beginStructure();
        arg >> type >> pattern;
        arg.endStructure();
        out.rules.append({type, pattern});
    }
    arg.endArray();
    arg.endStructure();
    return out;
}

DecodedFilter decodeFilter(const QVariant &value)
{
    return readFilter(value.value<QDBusArgument>());
}

QList<DecodedFilter> decodeFilters(const QVariant &value)
{
    QList<DecodedFilter> list;
    const QDBusArgument arg = value.value<QDBusArgument>();
    arg.beginArray();
    while (!arg.atEnd())
        list.append(readFilter(arg));
    arg.endArray();
    return list;
}

} // namespace

class FakePortalRequest : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.portal.Request")

public:
    bool closed = false;

public slots:
    void Close() { closed = true; }
};

class FakePortalChooser : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.portal.FileChooser")

public:
    FakePortalChooser(QDBusConnection bus, QString clientService, FakePortalRequest *request)
        : m_bus(std::move(bus)), m_clientService(std::move(clientService)), m_request(request) {}

    QString lastMethod;
    QString lastParentWindow;
    QString lastTitle;
    QVariantMap lastOptions;
    QString lastRequestPath;
    QStringList overrideRequestPaths;
    QString immediateResponseUri;
    QHash<QString, FakePortalRequest *> extraRequests;

    void respondAt(const QString &path, uint code, const QVariantMap &results = {})
    {
        QDBusMessage signal = QDBusMessage::createSignal(
            path, QStringLiteral("org.freedesktop.portal.Request"), QStringLiteral("Response"));
        signal << code << results;
        m_bus.send(signal);
    }

    void respond(uint code, const QVariantMap &results = {})
    {
        respondAt(lastRequestPath, code, results);
    }

public slots:
    QDBusObjectPath OpenFile(const QString &parentWindow, const QString &title, const QVariantMap &options)
    {
        return handleCall(QStringLiteral("OpenFile"), parentWindow, title, options);
    }

    QDBusObjectPath SaveFile(const QString &parentWindow, const QString &title, const QVariantMap &options)
    {
        return handleCall(QStringLiteral("SaveFile"), parentWindow, title, options);
    }

private:
    QDBusObjectPath handleCall(const QString &method, const QString &parentWindow,
                               const QString &title, const QVariantMap &options)
    {
        lastMethod = method;
        lastParentWindow = parentWindow;
        lastTitle = title;
        lastOptions = options;
        if (!overrideRequestPaths.isEmpty()) {
            lastRequestPath = overrideRequestPaths.takeFirst();
        } else {
            if (!lastRequestPath.isEmpty() && !extraRequests.contains(lastRequestPath))
                m_bus.unregisterObject(lastRequestPath);
            QString sender = m_clientService.startsWith(QLatin1Char(':'))
                                 ? m_clientService.mid(1)
                                 : m_clientService;
            sender.replace(QLatin1Char('.'), QLatin1Char('_'));
            lastRequestPath = QStringLiteral("/org/freedesktop/portal/desktop/request/%1/%2")
                                  .arg(sender, options.value(QStringLiteral("handle_token")).toString());
        }
        FakePortalRequest *target = extraRequests.value(lastRequestPath, m_request);
        m_bus.registerObject(lastRequestPath, target, QDBusConnection::ExportAllSlots);
        if (!immediateResponseUri.isEmpty()) {
            respond(0, {{QStringLiteral("uris"), QStringList{immediateResponseUri}}});
            immediateResponseUri.clear();
        }
        return QDBusObjectPath(lastRequestPath);
    }

    QDBusConnection m_bus;
    QString m_clientService;
    FakePortalRequest *m_request;
};

class FakeEditor : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantMap project READ project CONSTANT)
    Q_PROPERTY(bool exporting READ exporting CONSTANT)
    Q_PROPERTY(double exportProgress READ exportProgress CONSTANT)
    Q_PROPERTY(double exportFps READ exportFps CONSTANT)
    Q_PROPERTY(int exportEtaSeconds READ exportEtaSeconds CONSTANT)
    Q_PROPERTY(QString exportError READ exportError CONSTANT)
    Q_PROPERTY(double duration READ duration CONSTANT)
    Q_PROPERTY(int sourceWidth READ sourceWidth CONSTANT)
    Q_PROPERTY(int sourceHeight READ sourceHeight CONSTANT)
    Q_PROPERTY(QVariantList wallpaperGroups READ wallpaperGroups CONSTANT)
    Q_PROPERTY(QVariantList gradients READ gradients CONSTANT)

public:
    explicit FakeEditor(QString exportDir, QObject *parent = nullptr)
        : QObject(parent), m_exportDir(std::move(exportDir))
    {
        QVariantMap gif{{QStringLiteral("height"), 720}, {QStringLiteral("fps"), 20},
                        {QStringLiteral("quality"), QStringLiteral("social")}};
        QVariantMap exp{{QStringLiteral("format"), QStringLiteral("mp4")},
                        {QStringLiteral("height"), 1080},
                        {QStringLiteral("fps"), 60},
                        {QStringLiteral("quality"), QStringLiteral("social")},
                        {QStringLiteral("gif"), gif}};
        QVariantMap grad{{QStringLiteral("stops"), QVariantList{}}, {QStringLiteral("angle"), 135}};
        QVariantMap bg{{QStringLiteral("type"), QStringLiteral("color")},
                       {QStringLiteral("color"), QStringLiteral("#000000")},
                       {QStringLiteral("blur"), 0},
                       {QStringLiteral("resolvedImage"), QString()},
                       {QStringLiteral("gradient"), grad}};
        m_project.insert(QStringLiteral("export"), exp);
        m_project.insert(QStringLiteral("background"), bg);
    }

    QVariantMap project() const { return m_project; }
    bool exporting() const { return false; }
    double exportProgress() const { return 0.0; }
    double exportFps() const { return 0.0; }
    int exportEtaSeconds() const { return 0; }
    QString exportError() const { return {}; }
    double duration() const { return 5.0; }
    int sourceWidth() const { return 1920; }
    int sourceHeight() const { return 1080; }
    QVariantList wallpaperGroups() const { return {}; }
    QVariantList gradients() const { return {}; }

    QUrl insertedUrl;
    QString lastProjectPath;
    QVariant lastProjectValue;

    Q_INVOKABLE QString defaultExportPath(const QString &format) const
    {
        return QDir(m_exportDir).filePath(QStringLiteral("demo.%1").arg(format == QLatin1String("gif") ? QStringLiteral("gif") : QStringLiteral("mp4")));
    }
    Q_INVOKABLE QString formatTime(double) const { return QStringLiteral("00:05"); }
    Q_INVOKABLE void beginCoalescedEdit(const QString &) {}
    Q_INVOKABLE void endCoalescedEdit() {}
    Q_INVOKABLE void setProjectValue(const QString &path, const QVariant &value, bool = false)
    {
        lastProjectPath = path;
        lastProjectValue = value;
    }
    Q_INVOKABLE void insertMedia(const QUrl &url) { insertedUrl = url; }
    Q_INVOKABLE void exportTo(const QString &, const QVariantMap &) {}
    Q_INVOKABLE void cancelExport() {}
    Q_INVOKABLE void openContainingFolder(const QString &) {}
    Q_INVOKABLE void copyPath(const QString &) {}

signals:
    void exportFinished(const QString &path);

private:
    QString m_exportDir;
    QVariantMap m_project;
};

class FakeLauncher : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool recording READ recording CONSTANT)
    Q_PROPERTY(bool startingRecording READ startingRecording CONSTANT)
    Q_PROPERTY(bool stoppingRecording READ stoppingRecording CONSTANT)
    Q_PROPERTY(QString recordingElapsed READ recordingElapsed CONSTANT)
    Q_PROPERTY(QString captureExclusionNote READ captureExclusionNote CONSTANT)
    Q_PROPERTY(QString captureExclusionReason READ captureExclusionReason CONSTANT)
    Q_PROPERTY(bool systemAudio READ systemAudio WRITE setSystemAudio NOTIFY changed)
    Q_PROPERTY(bool microphone READ microphone WRITE setMicrophone NOTIFY changed)
    Q_PROPERTY(QString microphoneDevice READ microphoneDevice WRITE setMicrophoneDevice NOTIFY changed)
    Q_PROPERTY(QVariantList audioDevices READ audioDevices CONSTANT)
    Q_PROPERTY(double microphoneLevel READ microphoneLevel CONSTANT)
    Q_PROPERTY(bool webcam READ webcam WRITE setWebcam NOTIFY changed)
    Q_PROPERTY(QString webcamDevice READ webcamDevice WRITE setWebcamDevice NOTIFY changed)
    Q_PROPERTY(QVariantList webcamDevices READ webcamDevices CONSTANT)
    Q_PROPERTY(QString selfViewStatus READ selfViewStatus CONSTANT)
    Q_PROPERTY(int webcamRotation READ webcamRotation WRITE setWebcamRotation NOTIFY changed)
    Q_PROPERTY(bool webcamFlipHorizontal READ webcamFlipHorizontal WRITE setWebcamFlipHorizontal NOTIFY changed)
    Q_PROPERTY(bool countdownBeforeRecording READ countdownBeforeRecording WRITE setCountdownBeforeRecording NOTIFY changed)
    Q_PROPERTY(bool selfViewEnabled READ selfViewEnabled WRITE setSelfViewEnabled NOTIFY changed)
    Q_PROPERTY(QString selfViewSize READ selfViewSize WRITE setSelfViewSize NOTIFY changed)

public:
    bool recording() const { return false; }
    bool startingRecording() const { return false; }
    bool stoppingRecording() const { return false; }
    QString recordingElapsed() const { return QStringLiteral("00:00"); }
    QString captureExclusionNote() const { return {}; }
    QString captureExclusionReason() const { return {}; }
    bool systemAudio() const { return false; }
    void setSystemAudio(bool) {}
    bool microphone() const { return false; }
    void setMicrophone(bool) {}
    QString microphoneDevice() const { return {}; }
    void setMicrophoneDevice(const QString &) {}
    QVariantList audioDevices() const { return {}; }
    double microphoneLevel() const { return 0.0; }
    bool webcam() const { return false; }
    void setWebcam(bool) {}
    QString webcamDevice() const { return {}; }
    void setWebcamDevice(const QString &) {}
    QVariantList webcamDevices() const { return {}; }
    QString selfViewStatus() const { return {}; }
    int webcamRotation() const { return 0; }
    void setWebcamRotation(int) {}
    bool webcamFlipHorizontal() const { return false; }
    void setWebcamFlipHorizontal(bool) {}
    bool countdownBeforeRecording() const { return false; }
    void setCountdownBeforeRecording(bool) {}
    bool selfViewEnabled() const { return false; }
    void setSelfViewEnabled(bool) {}
    QString selfViewSize() const { return QStringLiteral("M"); }
    void setSelfViewSize(const QString &) {}

    QString openedBundle;

    Q_INVOKABLE void openBundle(const QString &path) { openedBundle = path; }
    Q_INVOKABLE void showRecordingsFolder() {}
    Q_INVOKABLE void record() {}
    Q_INVOKABLE void stopRecording() {}
    Q_INVOKABLE void cancelRecording() {}

signals:
    void changed();
    void errorOccurred(const QString &message);
};

class PortalFilePickerTest : public QObject
{
    Q_OBJECT

private:
    struct PrivateBus {
        QProcess daemon;
        QString address;
        QString serviceName;
        QString clientName;
        QDBusConnection serviceBus{QString()};
        QDBusConnection clientBus{QString()};
        bool valid = false;

        explicit PrivateBus(const QString &tag)
            : serviceName(QStringLiteral("omareel-portal-service-%1").arg(tag)),
              clientName(QStringLiteral("omareel-portal-client-%1").arg(tag))
        {
            if (QStandardPaths::findExecutable(QStringLiteral("dbus-daemon")).isEmpty())
                return;
            daemon.start(QStringLiteral("dbus-daemon"),
                         {QStringLiteral("--session"), QStringLiteral("--print-address"),
                          QStringLiteral("--nofork")});
            if (!daemon.waitForReadyRead(5000))
                return;
            address = QString::fromUtf8(daemon.readLine()).trimmed();
            serviceBus = QDBusConnection::connectToBus(address, serviceName);
            clientBus = QDBusConnection::connectToBus(address, clientName);
            valid = serviceBus.isConnected() && clientBus.isConnected();
        }

        ~PrivateBus()
        {
            if (!clientName.isEmpty())
                QDBusConnection::disconnectFromBus(clientName);
            if (!serviceName.isEmpty())
                QDBusConnection::disconnectFromBus(serviceName);
            if (daemon.state() != QProcess::NotRunning) {
                daemon.kill();
                daemon.waitForFinished();
            }
        }
    };

private slots:
    void openSaveFolderCancelFailureCrashAndClose()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("dbus-daemon")).isEmpty())
            QSKIP("needs dbus-daemon for a private bus");

        PrivateBus bus(QStringLiteral("core"));
        QVERIFY(bus.valid);
        QVERIFY(bus.serviceBus.registerService(QStringLiteral("org.freedesktop.portal.Desktop")));

        FakePortalRequest request;
        FakePortalChooser chooser(bus.serviceBus, bus.clientBus.baseService(), &request);
        QVERIFY(bus.serviceBus.registerObject(QStringLiteral("/org/freedesktop/portal/desktop"),
                                              &chooser, QDBusConnection::ExportAllSlots));

        const QByteArray expectedHome = QDir::homePath().toUtf8() + '\0';
        QTemporaryDir dir;
        QVERIFY(dir.isValid());

        {
            PortalFilePicker picker(bus.clientBus);
            QSignalSpy acceptSpy(&picker, &PortalFilePicker::accepted);
            QSignalSpy cancelSpy(&picker, &PortalFilePicker::cancelled);
            QSignalSpy failSpy(&picker, &PortalFilePicker::failed);

            // OpenFile: empty folder falls back to $HOME, parses nameFilters, sets current_filter.
            picker.setMode(PortalFilePicker::Mode::OpenFile);
            picker.setTitle(QStringLiteral("Insert a video at the playhead"));
            picker.setNameFilters({QStringLiteral("Videos (*.mp4 *.mov *.mkv)"),
                                   QStringLiteral("All files (*)")});
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            QCOMPARE(chooser.lastParentWindow, QString());
            QCOMPARE(chooser.lastTitle, QStringLiteral("Insert a video at the playhead"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("modal")).toBool(), true);
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("multiple")).toBool(), false);
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("accept_label")).toString(), QStringLiteral("Open"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("current_folder")).toByteArray(), expectedHome);
            const auto filters = decodeFilters(chooser.lastOptions.value(QStringLiteral("filters")));
            QCOMPARE(filters.size(), 2);
            QCOMPARE(filters[0].name, QStringLiteral("Videos"));
            QCOMPARE(filters[0].rules.size(), 3);
            QCOMPARE(filters[0].rules[0], qMakePair(0u, QStringLiteral("*.mp4")));
            QCOMPARE(filters[1].name, QStringLiteral("All files"));
            QCOMPARE(filters[1].rules, (QList<QPair<uint, QString>>{{0u, QStringLiteral("*")}}));
            const auto currentFilter = decodeFilter(chooser.lastOptions.value(QStringLiteral("current_filter")));
            QCOMPARE(currentFilter.name, QStringLiteral("Videos"));

            // Stale Response on a different object path is ignored.
            chooser.respondAt(QStringLiteral("/org/freedesktop/portal/desktop/request/stale/1"), 0,
                              {{QStringLiteral("uris"), QStringList{QStringLiteral("file:///tmp/wrong.mp4")}}});
            QTest::qWait(50);
            QCOMPARE(acceptSpy.count(), 0);

            const QString chosenVideo = dir.filePath(QStringLiteral("my clip.mp4"));
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{QUrl::fromLocalFile(chosenVideo).toString()}}});
            QTRY_COMPARE(acceptSpy.count(), 1);
            QCOMPARE(picker.selectedFile(), QUrl::fromLocalFile(chosenVideo));
            QCOMPARE(picker.selectedPath(), chosenVideo);

            // Cancel (response == 1) emits cancelled() only.
            chooser.lastMethod.clear();
            picker.setCurrentFolder(dir.path());
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("current_folder")).toByteArray(),
                     dir.path().toUtf8() + '\0');
            chooser.respond(1);
            QTRY_COMPARE(cancelSpy.count(), 1);
            QCOMPARE(failSpy.count(), 0);

            // Empty URIs or response == 2 emits failed(), not cancelled().
            chooser.lastMethod.clear();
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            chooser.respond(0);
            QTRY_COMPARE(failSpy.count(), 1);
            QCOMPARE(cancelSpy.count(), 1);

            chooser.lastMethod.clear();
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            chooser.respond(2);
            QTRY_COMPARE(failSpy.count(), 2);

            // SaveFile: missing folder falls back to $HOME, current_name preserved, returned path untouched.
            chooser.lastMethod.clear();
            picker.setMode(PortalFilePicker::Mode::SaveFile);
            picker.setTitle(QStringLiteral("Export to"));
            picker.setSuggestedPath(dir.filePath(QStringLiteral("missing/clip.gif")));
            picker.setNameFilters({QStringLiteral("GIF image (*.gif)")});
            QVERIFY(!picker.busy());
            picker.open();
            QVERIFY(picker.busy());
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("SaveFile"));
            QCOMPARE(chooser.lastTitle, QStringLiteral("Export to"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("accept_label")).toString(), QStringLiteral("Save"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("current_folder")).toByteArray(), expectedHome);
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("current_name")).toString(), QStringLiteral("clip.gif"));
            const QString chosenGif = dir.filePath(QStringLiteral("rendered demo.gif"));
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{QUrl::fromLocalFile(chosenGif).toString()}}});
            QTRY_COMPARE(acceptSpy.count(), 2);
            QVERIFY(!picker.busy());
            QCOMPARE(picker.selectedPath(), chosenGif);
            QCOMPARE(picker.selectedFile(), QUrl::fromLocalFile(chosenGif));

            // OpenFolder passes directory: true and omits filters.
            chooser.lastMethod.clear();
            picker.setMode(PortalFilePicker::Mode::OpenFolder);
            picker.setTitle(QStringLiteral("Open an omareel bundle"));
            picker.setCurrentFolder(QString());
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("directory")).toBool(), true);
            QVERIFY(!chooser.lastOptions.contains(QStringLiteral("filters")));
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{QUrl::fromLocalFile(dir.path()).toString()}}});
            QTRY_COMPARE(acceptSpy.count(), 3);
            QCOMPARE(picker.selectedPath(), dir.path());

            // Non-predicted request path returned by an older portal moves the subscription.
            chooser.lastMethod.clear();
            const QString legacyPath = QStringLiteral("/org/freedesktop/portal/desktop/request/legacy/42");
            chooser.overrideRequestPaths = {legacyPath};
            picker.setMode(PortalFilePicker::Mode::OpenFile);
            picker.open();
            QTRY_COMPARE(picker.pendingRequestPath(), legacyPath);
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{QUrl::fromLocalFile(chosenVideo).toString()}}});
            QTRY_COMPARE(acceptSpy.count(), 4);

            // Response emitted before OpenFile returns still succeeds via handle_token pre-subscription.
            chooser.lastMethod.clear();
            request.closed = false;
            chooser.immediateResponseUri = QUrl::fromLocalFile(chosenVideo).toString();
            picker.open();
            QTRY_COMPARE(acceptSpy.count(), 5);
            QVERIFY(!request.closed);

            // Replacing a pending legacy-path request before its reply still closes the first returned path.
            FakePortalRequest firstLegacyRequest;
            const QString firstLegacyPath = QStringLiteral("/org/freedesktop/portal/desktop/request/legacy/99");
            const QString secondLegacyPath = QStringLiteral("/org/freedesktop/portal/desktop/request/legacy/100");
            chooser.extraRequests.insert(firstLegacyPath, &firstLegacyRequest);
            chooser.overrideRequestPaths = {firstLegacyPath, secondLegacyPath};
            picker.open();
            picker.open();
            QTRY_VERIFY(firstLegacyRequest.closed);
            QTRY_COMPARE(picker.pendingRequestPath(), secondLegacyPath);
            chooser.respond(1);
            QTRY_COMPARE(cancelSpy.count(), 2);
            bus.serviceBus.unregisterObject(firstLegacyPath);
            chooser.extraRequests.clear();

            // Replacing a pending request sends Request.Close to the previous one.
            chooser.lastMethod.clear();
            request.closed = false;
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            chooser.lastMethod.clear();
            picker.open();
            QTRY_VERIFY(request.closed);
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));

            // Portal crash mid-dialog (service disappears) fails cleanly instead of wedging.
            QVERIFY(bus.serviceBus.unregisterService(QStringLiteral("org.freedesktop.portal.Desktop")));
            QTRY_COMPARE(failSpy.count(), 3);

            // Direct owner replacement (oldOwner -> newOwner without intermediate unregistration) fails cleanly.
            QCOMPARE(bus.serviceBus.interface()->registerService(
                         QStringLiteral("org.freedesktop.portal.Desktop"),
                         QDBusConnectionInterface::DontQueueService,
                         QDBusConnectionInterface::AllowReplacement).value(),
                     QDBusConnectionInterface::ServiceRegistered);
            chooser.lastMethod.clear();
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            QDBusConnection replacementBus =
                QDBusConnection::connectToBus(bus.address, QStringLiteral("omareel-portal-replacement"));
            const auto disconnectReplacement = qScopeGuard([&] {
                QDBusConnection::disconnectFromBus(QStringLiteral("omareel-portal-replacement"));
            });
            QVERIFY(replacementBus.isConnected());
            QCOMPARE(replacementBus.interface()->registerService(
                         QStringLiteral("org.freedesktop.portal.Desktop"),
                         QDBusConnectionInterface::ReplaceExistingService,
                         QDBusConnectionInterface::AllowReplacement).value(),
                     QDBusConnectionInterface::ServiceRegistered);
            QTRY_COMPARE(failSpy.count(), 4);
            QVERIFY(replacementBus.unregisterService(QStringLiteral("org.freedesktop.portal.Desktop")));

            // Re-registering portal allows a new request to succeed; destruction closes a pending request.
            QVERIFY(bus.serviceBus.registerService(QStringLiteral("org.freedesktop.portal.Desktop")));
            chooser.lastMethod.clear();
            request.closed = false;
            picker.open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
        }
        QTRY_VERIFY(request.closed);
    }

    void disconnectedBusFailsImmediately()
    {
        PortalFilePicker picker(QDBusConnection(QStringLiteral("omareel-disconnected-test")));
        QSignalSpy failSpy(&picker, &PortalFilePicker::failed);
        picker.open();
        QCOMPARE(failSpy.count(), 1);
    }

    void qmlDialogsWireToPortalFilePicker()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("dbus-daemon")).isEmpty())
            QSKIP("needs dbus-daemon for a private bus");

        PrivateBus bus(QStringLiteral("qml"));
        QVERIFY(bus.valid);
        QVERIFY(bus.serviceBus.registerService(QStringLiteral("org.freedesktop.portal.Desktop")));

        FakePortalRequest request;
        FakePortalChooser chooser(bus.serviceBus, bus.clientBus.baseService(), &request);
        QVERIFY(bus.serviceBus.registerObject(QStringLiteral("/org/freedesktop/portal/desktop"),
                                              &chooser, QDBusConnection::ExportAllSlots));

        QTemporaryDir dir;
        QVERIFY(dir.isValid());
        Theme theme;
        FakeEditor editor(dir.path());
        FakeLauncher launcher;

        QQmlEngine engine;
        QStringList warnings;
        connect(&engine, &QQmlEngine::warnings, this, [&](const QList<QQmlError> &list) {
            for (const QQmlError &error : list)
                warnings << error.toString();
        });
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("launcher"), &launcher);

        // 1. ExportDialog.qml
        {
            QQmlComponent component(&engine);
            component.setData(R"(
                import QtQuick
                import QtQuick.Controls
                import Omareel
                ApplicationWindow {
                    width: 600; height: 800
                    ExportDialog { id: dlg; objectName: "dlg" }
                })", QUrl());
            QVERIFY2(component.isReady(), qPrintable(component.errorString()));
            std::unique_ptr<QObject> root(component.create());
            QVERIFY(root);
            auto *dialog = root->findChild<QObject *>(QStringLiteral("dlg"));
            auto *optionsLayout = root->findChild<QObject *>(QStringLiteral("exportOptionsLayout"));
            auto *picker = root->findChild<PortalFilePicker *>(QStringLiteral("exportOutputPicker"));
            QVERIFY(dialog && optionsLayout && picker);
            picker->setConnection(bus.clientBus);

            QVERIFY(optionsLayout->property("enabled").toBool());
            chooser.lastMethod.clear();
            picker->open();
            QVERIFY(!optionsLayout->property("enabled").toBool());
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("SaveFile"));
            QCOMPARE(chooser.lastTitle, QStringLiteral("Export to"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("current_folder")).toByteArray(),
                     dir.path().toUtf8() + '\0');
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("current_name")).toString(),
                     QStringLiteral("demo.mp4"));
            const auto mp4Filters = decodeFilters(chooser.lastOptions.value(QStringLiteral("filters")));
            QCOMPARE(mp4Filters.size(), 1);
            QCOMPARE(mp4Filters[0].name, QStringLiteral("MP4 video"));
            QCOMPARE(mp4Filters[0].rules, (QList<QPair<uint, QString>>{{0u, QStringLiteral("*.mp4")}}));

            const QString chosenMp4 = dir.filePath(QStringLiteral("final cut.mp4"));
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{QUrl::fromLocalFile(chosenMp4).toString()}}});
            QTRY_COMPARE(dialog->property("outputPath").toString(), chosenMp4);
            QVERIFY(optionsLayout->property("enabled").toBool());

            // Switching format to gif updates filters.
            QVERIFY(QMetaObject::invokeMethod(dialog, "selectFormat", Q_ARG(QVariant, QStringLiteral("gif"))));
            chooser.lastMethod.clear();
            picker->open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("SaveFile"));
            const auto gifFilters = decodeFilters(chooser.lastOptions.value(QStringLiteral("filters")));
            QCOMPARE(gifFilters.size(), 1);
            QCOMPARE(gifFilters[0].name, QStringLiteral("GIF image"));
            QCOMPARE(gifFilters[0].rules, (QList<QPair<uint, QString>>{{0u, QStringLiteral("*.gif")}}));
            chooser.respond(2);
            QTRY_VERIFY(!dialog->property("pickerError").toString().isEmpty());
        }

        // 2. InsertClipDialog.qml
        {
            QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/Omareel/InsertClipDialog.qml")));
            QVERIFY2(component.isReady(), qPrintable(component.errorString()));
            std::unique_ptr<QObject> obj(component.create());
            auto *picker = qobject_cast<PortalFilePicker *>(obj.get());
            QVERIFY(picker);
            picker->setConnection(bus.clientBus);

            chooser.lastMethod.clear();
            picker->open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            QCOMPARE(chooser.lastTitle, QStringLiteral("Insert a video at the playhead"));
            const auto filters = decodeFilters(chooser.lastOptions.value(QStringLiteral("filters")));
            QCOMPARE(filters.size(), 2);
            QCOMPARE(filters[0].name, QStringLiteral("Videos"));
            QCOMPARE(filters[0].rules.size(), 6);
            QCOMPARE(filters[1].name, QStringLiteral("All files"));
            const QUrl clipUrl = QUrl::fromLocalFile(dir.filePath(QStringLiteral("retake.mp4")));
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{clipUrl.toString()}}});
            QTRY_COMPARE(editor.insertedUrl, clipUrl);
        }

        // 3. BackgroundPanel.qml
        {
            QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/Omareel/BackgroundPanel.qml")));
            QVERIFY2(component.isReady(), qPrintable(component.errorString()));
            std::unique_ptr<QObject> panel(component.create());
            QVERIFY(panel);
            auto *picker = panel->findChild<PortalFilePicker *>(QStringLiteral("backgroundImagePicker"));
            QVERIFY(picker);
            picker->setConnection(bus.clientBus);

            chooser.lastMethod.clear();
            picker->open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            QCOMPARE(chooser.lastTitle, QStringLiteral("Choose a background image"));
            const auto filters = decodeFilters(chooser.lastOptions.value(QStringLiteral("filters")));
            QCOMPARE(filters.size(), 1);
            QCOMPARE(filters[0].name, QStringLiteral("Images"));
            QCOMPARE(filters[0].rules.size(), 4);
            const QUrl imgUrl = QUrl::fromLocalFile(dir.filePath(QStringLiteral("bg.png")));
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{imgUrl.toString()}}});
            QTRY_COMPARE(editor.lastProjectPath, QStringLiteral("background.image"));
            QCOMPARE(editor.lastProjectValue.toUrl(), imgUrl);
        }

        // 4. Launcher.qml
        {
            QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/Omareel/Launcher.qml")));
            QVERIFY2(component.isReady(), qPrintable(component.errorString()));
            std::unique_ptr<QObject> window(component.create());
            QVERIFY(window);
            auto *picker = window->findChild<PortalFilePicker *>(QStringLiteral("openBundlePicker"));
            QVERIFY(picker);
            picker->setConnection(bus.clientBus);

            chooser.lastMethod.clear();
            picker->open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            QCOMPARE(chooser.lastTitle, QStringLiteral("Open an omareel bundle"));
            QCOMPARE(chooser.lastOptions.value(QStringLiteral("directory")).toBool(), true);
            chooser.respond(0, {{QStringLiteral("uris"), QStringList{QUrl::fromLocalFile(dir.path()).toString()}}});
            QTRY_COMPARE(launcher.openedBundle, dir.path());

            chooser.lastMethod.clear();
            picker->open();
            QTRY_COMPARE(chooser.lastMethod, QStringLiteral("OpenFile"));
            chooser.respond(2);
            QTRY_VERIFY(!window->property("errorMessage").toString().isEmpty());
        }

        QVERIFY2(warnings.isEmpty(), qPrintable(warnings.join(QLatin1Char('\n'))));
    }
};

QTEST_MAIN(PortalFilePickerTest)
#include "test_portal_file_picker.moc"
