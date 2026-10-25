// launcher: the decisions inside src/ui/Launcher.cpp, a QObject that lists the machine's cameras
// and microphones when it is constructed, so it is not run: its pieces are copied below with the
// lines they come from, the members they read made inputs. Cameras are real QCameraDevice values
// built with Qt's private QCameraDevicePrivate (camera.cpp), so cameraCanRecord and cameraIdNode
// are the tag's. gpu-screen-recorder --list-audio-devices is answered by the replay shim.
// tests/golden/README.md, launcher.

#include "common.h"
#include "recording.h"
#include "replay.h"

#include "record/CameraCapture.h"

#include <QCameraDevice>
#include <QDir>
#include <QFileInfo>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QSet>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

#include <algorithm>
#include <cmath>
#include <limits>

namespace Oracle {

QCameraDevice fakeCamera(const QByteArray &id, const QString &description, const QJsonArray &formats);

namespace {

using Omareel::cameraCanRecord;
using Omareel::cameraIdNode;

// src/ui/Launcher.cpp:30-37 at rust-migration-base, verbatim.
static QString cameraName(const QString &description)
{
    const qsizetype separator = description.indexOf(QLatin1String(": "));
    if (separator <= 0) return description;
    const QString device = description.left(separator).trimmed();
    const QString stream = description.mid(separator + 2).trimmed();
    return device.startsWith(stream) ? device : description;
}

// src/ui/Launcher.cpp:371-376, verbatim (a private static of Launcher).
QString formatDuration(double seconds)
{
    const int total = std::max(0, qRound(seconds));
    return QStringLiteral("%1:%2").arg(total / 60, 2, 10, QLatin1Char('0'))
                                      .arg(total % 60, 2, 10, QLatin1Char('0'));
}

// The members of Launcher (src/ui/Launcher.h at the tag) the copies below read and write.
struct Launcher {
    QString m_microphoneDevice = QStringLiteral("default_input");
    QString m_chosenMicrophoneDevice = QStringLiteral("default_input");
    QVariantList m_audioDevices;
    QString m_webcamDevice = QStringLiteral("/dev/video2");
    QString m_chosenWebcamDevice;
    QVariantList m_webcamDevices;
    QList<QCameraDevice> m_cameraDevices;
    int m_webcamHeight = 1080;
    int m_webcamRotation = 0;
    QString m_selfViewSize = QStringLiteral("M");
    bool m_systemAudio = true;
    bool m_microphone = true;
    bool m_webcam = false;
    qint64 m_hostPid = 0;
    // Stand-ins for what the copies ask the machine.
    QHash<QString, QString> stablePaths; // stableCameraPath(node)
    QSet<QString> existing;              // QFileInfo::exists(path)
    bool hostAlive = true;               // ::kill(pid_t(m_hostPid), 0) == 0

    QString stableCameraPath(const QString &node) const { return stablePaths.value(node, node); }

    // src/ui/Launcher.cpp:224-249 (refreshAudioDevices) without emit audioDevicesChanged();
    // returns whether recordingPreferencesChanged would be emitted (lines 251-252).
    bool refreshAudioDevices()
    {
        QProcess process;
        process.start(QStringLiteral("gpu-screen-recorder"), {QStringLiteral("--list-audio-devices")});
        QVariantList devices;
        if (process.waitForFinished(5000) && process.exitCode() == 0) {
            const QStringList lines = QString::fromUtf8(process.readAllStandardOutput()).split('\n', Qt::SkipEmptyParts);
            for (const QString &line : lines) {
                const int separator = line.indexOf(QLatin1Char('|'));
                if (separator <= 0) continue;
                const QString id = line.left(separator).trimmed();
                if (id != QLatin1String("default_input") && !id.startsWith(QLatin1String("alsa_input."))) continue;
                devices << QVariantMap{{QStringLiteral("value"), id},
                                       {QStringLiteral("text"), line.mid(separator + 1).trimmed()}};
            }
        }
        if (devices.isEmpty())
            devices << QVariantMap{{QStringLiteral("value"), QStringLiteral("default_input")},
                                   {QStringLiteral("text"), QStringLiteral("Default input")}};
        // A microphone that isn't plugged in stays chosen, so it is used again once it is back.
        const bool chosenFound = std::any_of(devices.cbegin(), devices.cend(), [this](const QVariant &device) {
            return device.toMap().value(QStringLiteral("value")).toString() == m_chosenMicrophoneDevice;
        });
        const QString microphone = chosenFound ? m_chosenMicrophoneDevice : QStringLiteral("default_input");
        m_audioDevices = devices;
        const bool changed = microphone != m_microphoneDevice;
        m_microphoneDevice = microphone;
        return changed;
    }

    // src/ui/Launcher.cpp:265-303 (refreshWebcamDevices) with the cameras given instead of
    // QMediaDevices::videoInputs(), stableCameraPath and QFileInfo::exists answered by the
    // members above, and emit webcamDevicesChanged() and what follows line 303 (saving the
    // preferences, telling the self-view host) left out; returns whether it got past the early
    // return.
    bool refreshWebcamDevices(const QList<QCameraDevice> &videoInputs)
    {
        QVariantList devices;
        QSet<QString> seen;
        m_cameraDevices = videoInputs;
        for (const QCameraDevice &camera : m_cameraDevices) {
            if (!cameraCanRecord(camera)) continue;
            const QString node = cameraIdNode(camera.id());
            if (node.isEmpty() || seen.contains(node)) continue;
            seen.insert(node);
            devices << QVariantMap{{QStringLiteral("value"), stableCameraPath(node)},
                {QStringLiteral("text"), cameraName(camera.description())}, {QStringLiteral("node"), node}};
        }
        // Two cameras of the same model are told apart by their device node.
        for (QVariant &device : devices) {
            QVariantMap entry = device.toMap();
            const QString text = entry.value(QStringLiteral("text")).toString();
            const auto sameName = [&](const QVariant &other) {
                return other.toMap().value(QStringLiteral("text")).toString() == text;
            };
            if (std::count_if(devices.cbegin(), devices.cend(), sameName) > 1) {
                entry.insert(QStringLiteral("text"), QStringLiteral("%1 (%2)").arg(text,
                    entry.value(QStringLiteral("node")).toString()));
                device = entry;
            }
        }
        if (m_cameraDevices.isEmpty() && existing.contains(m_webcamDevice))
            devices << QVariantMap{{QStringLiteral("value"), m_webcamDevice},
                                   {QStringLiteral("text"), m_webcamDevice}};
        const auto listed = [&devices](const QString &path) {
            return std::any_of(devices.cbegin(), devices.cend(), [&path](const QVariant &device) {
                return device.toMap().value(QStringLiteral("value")).toString() == path;
            });
        };
        const QString selected = listed(m_chosenWebcamDevice) ? m_chosenWebcamDevice
            : listed(m_webcamDevice) || devices.isEmpty() ? m_webcamDevice
            : devices.first().toMap().value(QStringLiteral("value")).toString();
        m_webcamDevices = devices;
        if (selected == m_webcamDevice) return false;
        m_webcamDevice = selected;
        return true;
    }

    // src/ui/Launcher.cpp:154 (setWebcamHeight), 164-165 (setWebcamRotation) and 191-193
    // (setSelfViewSize): how the setters normalise a value before comparing it.
    static int webcamHeight(int value)
    {
        value = value == 720 ? 720 : 1080;
        return value;
    }
    static int webcamRotation(int value)
    {
        value = ((value % 360) + 360) % 360;
        if (value != 0 && value != 90 && value != 180 && value != 270) value = 0;
        return value;
    }
    static QString selfViewSize(const QString &value)
    {
        const QString normalized = value.toUpper();
        if (normalized != QLatin1String("S") && normalized != QLatin1String("M")
            && normalized != QLatin1String("L")) return QStringLiteral("<ignored>");
        return normalized;
    }

    // src/ui/Launcher.cpp:329-335 (refreshRecording, with the webcam on): the self-view status
    // line, with the host file's contents given and hostAlive standing for the kill(pid, 0) check.
    QString selfViewStatus(const QJsonObject &host) const
    {
        QString status = QStringLiteral("Starting camera…");
        if (m_hostPid > 0 && !hostAlive) status = QStringLiteral("Camera preview closed");
        else if (!host.value("camera_error").toString().isEmpty()) status = host.value("camera_error").toString();
        else if (host.value("camera_status").toString() == "ready")
            status = host.value("visible").toBool() ? QStringLiteral("Self-view floating on %1 — drag it where you want it").arg(host.value("monitor").toString())
                                                    : QStringLiteral("Camera ready — self-view hidden");
        return status;
    }

    // src/ui/Launcher.cpp:419-430 (record's start timer): the arguments of `omareel record`.
    QStringList recordArguments() const
    {
        QStringList arguments{QStringLiteral("record")};
        if (!m_systemAudio && !m_microphone) arguments << QStringLiteral("--no-audio");
        else {
            if (m_systemAudio) arguments << QStringLiteral("--with-desktop-audio");
            // The chosen microphone even while unplugged: the recorder uses the default one then
            // and says so.
            if (m_microphone) arguments << QStringLiteral("--with-microphone-audio")
                                        << QStringLiteral("--microphone-device") << m_chosenMicrophoneDevice;
        }
        if (m_webcam) arguments << QStringLiteral("--with-webcam")
                                << QStringLiteral("--webcam-device") << m_webcamDevice
                                << QStringLiteral("--webcam-height") << QString::number(m_webcamHeight);
        return arguments;
    }
};

// src/ui/Launcher.cpp:50-52 (the constructor, when the plugin is not loaded), with the number of
// screens given.
QString captureExclusionNote(qsizetype screens)
{
    return screens > 1
        ? QStringLiteral("Overlays move off the recorded screen while recording")
        : QStringLiteral("Self-view hides while recording");
}

QByteArray deviceList(const QVariantList &devices)
{
    QList<QByteArray> items;
    for (const QVariant &device : devices) {
        const QVariantMap map = device.toMap();
        QList<QByteArray> fields;
        for (auto it = map.cbegin(); it != map.cend(); ++it)
            fields << it.key().toUtf8() + '=' + quote(it.value().toString());
        items << '{' + fields.join(',') + '}';
    }
    return '[' + items.join(',') + ']';
}

void launcherNames(const Context &context, const QJsonObject &cases)
{
    Text out;
    for (const QJsonValue &name : cases.value(QStringLiteral("cameraNames")).toArray())
        out.row({"cameraName", quote(name.toString()), quote(cameraName(name.toString()))});
    const double inf = std::numeric_limits<double>::infinity();
    for (const double seconds : {0.0, -0.0, -1.0, 0.4, 0.5, 0.49999999999999994, 1.5, 2.5, 59.4, 59.5, 60.0, 61.0,
                                 599.5, 3599.4, 3600.0, 5999.0, 6000.0, 86399.5, 359999.0, 2147483646.0, -2147483647.0,
                                 qQNaN(), inf, -inf})
        out.row({"formatDuration", num(seconds),
                 // qRound of NaN, infinities and values past int are undefined; the oracle skips them.
                 std::isfinite(seconds) && std::abs(seconds) < 2147483647.0 ? quote(formatDuration(seconds)) : QByteArray("-")});
    for (const int value : {720, 1080, 0, 480, 721, -720, 2160})
        out.row({"setWebcamHeight", QByteArray::number(value), QByteArray::number(Launcher::webcamHeight(value))});
    for (const int value : {0, 90, 180, 270, 360, 450, -90, -180, -270, -360, 45, 89, 1, -1, 720, 2147483647,
                            -2147483647 - 1})
        out.row({"setWebcamRotation", QByteArray::number(value), QByteArray::number(Launcher::webcamRotation(value))});
    for (const QString &value : {QStringLiteral("S"), QStringLiteral("m"), QStringLiteral("l"), QStringLiteral("XL"),
                                 QString(), QStringLiteral(" M"), QStringLiteral("ß"), QStringLiteral("ſ")})
        out.row({"setSelfViewSize", quote(value), quote(Launcher::selfViewSize(value))});
    for (const qsizetype screens : {qsizetype(0), qsizetype(1), qsizetype(2), qsizetype(3)})
        out.row({"captureExclusionNote", QByteArray::number(screens), quote(captureExclusionNote(screens))});
    writeFile(QDir(context.output).filePath(QStringLiteral("helpers.txt")), out.data());
}

void launcherAudio(const Context &context, const QJsonObject &cases)
{
    Text out;
    for (const QJsonValue &value : cases.value(QStringLiteral("audio")).toArray()) {
        const QJsonObject input = value.toObject();
        Replay replay(QDir(context.scratch).filePath(QStringLiteral("replay")));
        ScopedEnvironment environment;
        if (input.value(QStringLiteral("file")).isString()) {
            Replay::Answer answer;
            answer.out = readFile(QDir(context.corpus).filePath(input.value(QStringLiteral("file")).toString()));
            answer.status = input.value(QStringLiteral("status")).toInt();
            replay.answer(QStringLiteral("gpu-screen-recorder"), {QStringLiteral("--list-audio-devices")}, answer);
        } else {
            environment.set("PATH", QFile::encodeName(shimDirectory(context.scratch, QStringLiteral("empty"), {})));
        }
        replay.activate();
        Launcher launcher;
        launcher.m_chosenMicrophoneDevice = input.value(QStringLiteral("chosen")).toString();
        launcher.m_microphoneDevice = input.value(QStringLiteral("current")).toString();
        const bool changed = launcher.refreshAudioDevices();
        out.row({input.value(QStringLiteral("file")).isString() ? input.value(QStringLiteral("file")).toString().toUtf8()
                                                                : QByteArray("<gpu-screen-recorder not on PATH>"),
                 QByteArray::number(input.value(QStringLiteral("status")).toInt()), quote(launcher.m_chosenMicrophoneDevice),
                 quote(input.value(QStringLiteral("current")).toString()), deviceList(launcher.m_audioDevices),
                 quote(launcher.m_microphoneDevice), boolText(changed)});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("audio-devices.txt")), out.data());
}

void launcherWebcams(const Context &context, const QJsonObject &cases)
{
    Text out;
    const QJsonObject stable = cases.value(QStringLiteral("stable")).toObject();
    const QJsonObject scenarios = cases.value(QStringLiteral("webcams")).toObject();
    for (auto it = scenarios.begin(); it != scenarios.end(); ++it) {
        const QJsonObject input = it.value().toObject();
        Launcher launcher;
        for (auto path = stable.begin(); path != stable.end(); ++path)
            launcher.stablePaths.insert(path.key(), path.value().toString());
        for (const QJsonValue &path : input.value(QStringLiteral("exists")).toArray()) launcher.existing.insert(path.toString());
        launcher.m_webcamDevice = input.value(QStringLiteral("device")).toString();
        launcher.m_chosenWebcamDevice = input.value(QStringLiteral("chosen")).toString();
        QList<QCameraDevice> cameras;
        for (const QJsonValue &camera : input.value(QStringLiteral("cameras")).toArray()) {
            const QJsonObject object = camera.toObject();
            cameras << fakeCamera(object.value(QStringLiteral("id")).toString().toUtf8(),
                                  object.value(QStringLiteral("description")).toString(),
                                  object.value(QStringLiteral("formats")).toArray());
        }
        const bool changed = launcher.refreshWebcamDevices(cameras);
        out << "== " << it.key().toUtf8() << "\n";
        out.row({"devices", deviceList(launcher.m_webcamDevices)});
        out.row({"selected", quote(launcher.m_webcamDevice), boolText(changed)});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("webcam-devices.txt")), out.data());
}

void launcherStatus(const Context &context)
{
    Text out;
    const QList<QJsonObject> hosts{
        QJsonObject{}, QJsonObject{{"camera_status", "starting"}},
        QJsonObject{{"camera_status", "ready"}, {"visible", true}, {"monitor", "DP-1"}},
        QJsonObject{{"camera_status", "ready"}, {"visible", false}, {"monitor", "DP-1"}},
        QJsonObject{{"camera_status", "ready"}, {"visible", "true"}, {"monitor", 3}},
        QJsonObject{{"camera_status", "ready"}, {"visible", true}},
        QJsonObject{{"camera_status", "ready"}, {"camera_error", "Selected camera is unavailable"}},
        QJsonObject{{"camera_status", "failed"}, {"camera_error", ""}},
        QJsonObject{{"camera_status", "READY"}, {"visible", true}},
        QJsonObject{{"camera_status", "ready"}, {"visible", true}, {"monitor", "Écran %1 ☃"}},
    };
    for (const qint64 pid : {qint64(0), qint64(4242)})
        for (const bool alive : {true, false})
            for (const QJsonObject &host : hosts) {
                if (pid == 0 && !alive) continue;
                Launcher launcher;
                launcher.m_hostPid = pid;
                launcher.hostAlive = alive;
                out.row({"selfViewStatus", QByteArray::number(pid), boolText(alive),
                         QJsonDocument(host).toJson(QJsonDocument::Compact), quote(launcher.selfViewStatus(host))});
            }
    for (const bool system : {false, true})
        for (const bool microphone : {false, true})
            for (const bool webcam : {false, true}) {
                Launcher launcher;
                launcher.m_systemAudio = system;
                launcher.m_microphone = microphone;
                launcher.m_webcam = webcam;
                launcher.m_chosenMicrophoneDevice = QStringLiteral("alsa_input.unplugged");
                launcher.m_microphoneDevice = QStringLiteral("default_input");
                launcher.m_webcamDevice = QStringLiteral("/dev/v4l/by-id/usb-Insta360_Link_2-video-index0");
                launcher.m_webcamHeight = 720;
                out.row({"recordArguments", boolText(system), boolText(microphone), boolText(webcam),
                         quoteList(launcher.recordArguments())});
            }
    writeFile(QDir(context.output).filePath(QStringLiteral("status.txt")), out.data());
}

} // namespace

void runLauncher(const Context &context)
{
    const QJsonObject cases = corpusObject(context, QStringLiteral("launcher/cases.json"));
    launcherNames(context, cases);
    launcherAudio(context, cases);
    launcherWebcams(context, cases);
    launcherStatus(context);
}

} // namespace Oracle
