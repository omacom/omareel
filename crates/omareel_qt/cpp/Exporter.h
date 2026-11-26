#pragma once

#include "omareel_qt/src/export.cxxqt.h"

#include <QImage>
#include <functional>
#include <QList>
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
    QString preset;   // an export preset's id (crates/omareel_core/src/export/presets.rs)
    QString aspect;   // the project's aspect for this export only
    QString reframe;  // "follow" or "off" for this export only
};

// What `omareel frame` renders: stills at output times, or `count` spread over the output.
// Zero means the project's width and frame rate, as for an export.
struct StillOptions {
    QString bundlePath;
    QList<double> times;
    int count = 0;
    int width = 0;
    int fps = 0;
    // As `omareel export --preset/--aspect/--reframe`, for these stills only.
    QString preset;
    QString aspect;
    QString reframe;
    // Takes each still as it is rendered (its index, the still), instead of the list holding
    // them all; false stops with the error it set. Without it the stills are listed.
    std::function<bool(int, const struct Still &, QString *)> take;
};

struct Still {
    QImage image;
    // The output time the still shows: the start of the frame on screen at the time asked for.
    double time = 0.0;
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
    // Renders frames of the export as images, each the frame the export shows at its time
    // (same composition, same decoded pictures), without encoding anything. `duration` gets the
    // output's length. False with *error set.
    bool renderStills(const StillOptions &options, QList<Still> *stills, double *duration, QString *error);
    // The stills as one image: a grid `columns` wide with `gap` pixels around the tiles, each
    // labelled with its time.
    static QImage contactSheet(const QList<Still> &stills, int columns, int gap);

public slots:
    // Safe from any thread and while an export runs: it only sets the flag the export checks.
    void cancel();

signals:
    // Before the frames, when the project normalises loudness: how much of the sound has been
    // measured, 0 to 1.
    void measuring(double fraction);
    void progress(int frame, int total);
    void finished(const QString &path);
    void failed(const QString &message);

private:
    bool run(const ExportOptions &options, QString *error);
    ::rust::Box<omareel::exporter::CancelFlag> m_cancel;
};

} // namespace Omareel
