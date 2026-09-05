#include "CaptureExclusion.h"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QProcess>
#include <QRegularExpression>

using namespace Omareel;
namespace {
QByteArray hyprctl(const QStringList &arguments, bool *ok = nullptr)
{
    QProcess process;
    process.start("hyprctl", arguments);
    const bool success = process.waitForFinished(2000) && process.exitStatus() == QProcess::NormalExit && process.exitCode() == 0;
    if (!success && process.state() != QProcess::NotRunning) { process.kill(); process.waitForFinished(1000); }
    if (ok) *ok = success;
    return success ? process.readAllStandardOutput() : QByteArray();
}
}

bool CaptureExclusion::compatibleHash(const QString &built, const QString &running)
{
    return QRegularExpression("^[a-f0-9]{40}$").match(built).hasMatch() && built == running;
}

CaptureExclusion::Status CaptureExclusion::status()
{
    Status result;
    const auto plugins = QJsonDocument::fromJson(hyprctl({"-j", "plugin", "list"})).array();
    for (const auto &value : plugins) {
        if (value.toObject().value("name").toString() != "omareel-capture-exclude") continue;
        result.details = QJsonDocument::fromJson(hyprctl({"omareel-exclude", "status"})).object();
        result.builtHash = result.details.value("built_hash").toString();
        const auto version = QJsonDocument::fromJson(hyprctl({"-j", "version"})).object();
        result.loaded = compatibleHash(result.builtHash, version.value("commit").toString())
            && result.details.value("prefixes").toArray().contains("omareel-");
        if (!result.loaded) result.reason = "Plugin status or default prefix is unavailable";
        return result;
    }
    result.reason = "Capture exclusion plugin is not loaded";
    return result;
}

CaptureExclusion::Status CaptureExclusion::ensureLoaded()
{
    const QString overridePath = qEnvironmentVariable("OMAREEL_PLUGIN_PATH");
    // An explicit path is authoritative, including an intentionally unavailable path.
    if (qEnvironmentVariableIsSet("OMAREEL_PLUGIN_PATH") && !QFileInfo(overridePath).isFile())
        return {false, {}, "Configured capture exclusion plugin does not exist", {}};
    auto result = status();
    if (result.loaded) return result;
    const QStringList candidates = qEnvironmentVariableIsSet("OMAREEL_PLUGIN_PATH")
        ? QStringList{overridePath}
        : QStringList{QDir(QCoreApplication::applicationDirPath()).filePath("plugin/omareel-capture-exclude.so"),
                      "/usr/lib/omareel/omareel-capture-exclude.so"};
    for (const auto &path : candidates) {
        if (!QFileInfo(path).isFile()) continue;
        QFile hash(path + ".hash");
        const QString built = hash.open(QIODevice::ReadOnly) ? QString::fromUtf8(hash.readAll()).trimmed() : QString();
        const auto version = QJsonDocument::fromJson(hyprctl({"-j", "version"})).object();
        if (!compatibleHash(built, version.value("commit").toString()))
            return {false, built, "Capture exclusion plugin needs rebuilding for this Hyprland commit", {}};
        bool loaded = false;
        const auto reply = hyprctl({"plugin", "load", QFileInfo(path).absoluteFilePath()}, &loaded);
        // Hyprctl can exit successfully while the compositor returns a load error.
        if (!loaded || reply.trimmed() != "ok") return {false, built, "Hyprland refused to load the capture exclusion plugin", {}};
        return status();
    }
    return result;
}

QString CaptureExclusion::overlayMonitor(bool plugin, const QString &recorded, const QStringList &monitors)
{
    if (plugin && monitors.contains(recorded)) return recorded;
    for (const auto &name : monitors) if (name != recorded) return name;
    return {};
}

bool CaptureExclusion::overlaysOffMonitor(const QString &monitor)
{
    bool ok = false;
    QJsonParseError error;
    const auto document = QJsonDocument::fromJson(hyprctl({"-j", "layers"}, &ok), &error);
    if (!ok || error.error != QJsonParseError::NoError || !document.isObject() || !document.object().contains(monitor)) return false;
    const auto levels = document.object().value(monitor).toObject().value("levels").toObject();
    for (const auto &level : levels)
        for (const auto &layer : level.toArray())
            if (layer.toObject().value("namespace").toString().startsWith("omareel-")) return false;
    return true;
}
