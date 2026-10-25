#pragma once

#include <QFileSystemWatcher>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantMap>

namespace Omareel {

// The Qt side of the theme object, which is Rust (crates/omareel_qt/src/theme.rs): it watches
// the Omarchy theme files and asks for a reload 220 ms after they change.
class ThemeBase : public QObject
{
    Q_OBJECT
    // `type` is a Rust keyword, which CXX-Qt cannot name a property, so it is declared here.
    Q_PROPERTY(QVariantMap type READ typeMap NOTIFY typeChanged)
public:
    explicit ThemeBase(QObject *parent = nullptr);

    // Replaces the watched files and directories with those of `paths` that exist.
    void watchPaths(const QStringList &paths);
    virtual void reload() = 0;
    virtual QVariantMap typeMap() const = 0;

signals:
    void sourceChanged();
    void typeChanged();

private:
    QFileSystemWatcher m_watcher;
};

// Font calls that need a QGuiApplication; free functions so Rust can call them.
namespace ThemeFonts {
// The interface family main() resolved, if any.
void setConfiguredFamily(const QString &family);
QString configuredFamily();
// Registers the bundled interface face and returns its family, or an empty string when it
// could not be loaded.
QString bundledUiFamily();
// Sets the application font, when there is a QGuiApplication.
void setApplicationFont(const QString &family, int pixelSize);
// The face Theme uses: the one main() resolved, else the bundled Inter, else "sans-serif".
QString interfaceFamily();
} // namespace ThemeFonts

} // namespace Omareel
