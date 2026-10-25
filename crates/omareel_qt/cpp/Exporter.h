#pragma once

#include "omareel_qt/src/export.cxxqt.h"

#include <QObject>
#include <QSize>
#include <QString>

namespace Omareel {

QSize paddedEvenSize(int width, int height);

struct ExportOptions {
    QString bundlePath;
    QString outputPath;
    int fps = 0;
    int width = 0;
    QString quality;
    int gifFps = 0;
    int gifWidth = 0;
    bool timing = false;
};

// Exports a bundle to MP4 or GIF. What to export and how is decided in Rust
// (crates/omareel_core/src/export, through crates/omareel_qt/src/export.rs); this class renders
// the composition with ExportRenderer (src/plumbing), runs the event loop while it waits, and
// reports. It is compiled into the omareel_qt archive with the Rust it calls, so the editor's
// C++ base (EditorBase.h, beside it) can start one.
class Exporter : public QObject
{
    Q_OBJECT
public:
    explicit Exporter(QObject *parent = nullptr);
    ~Exporter() override;
    void exportBundle(const ExportOptions &options);

public slots:
    // Safe from any thread and while an export runs: it only sets the flag the export checks.
    void cancel();

signals:
    void progress(int frame, int total);
    void finished(const QString &path);
    void failed(const QString &message);

private:
    bool run(const ExportOptions &options, QString *error);
    ::rust::Box<omareel::exporter::CancelFlag> m_cancel;
};

} // namespace Omareel
