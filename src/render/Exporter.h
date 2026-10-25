#pragma once

#include "omareel_qt/src/export.cxxqt.h"

#include <QObject>
#include <QSize>
#include <QVariantList>
#include <QVariantMap>

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

class CompositionState : public QObject
{
    Q_OBJECT
    Q_PROPERTY(int outputWidth MEMBER outputWidth NOTIFY changed)
    Q_PROPERTY(int outputHeight MEMBER outputHeight NOTIFY changed)
    Q_PROPERTY(int sourceWidth MEMBER sourceWidth NOTIFY changed)
    Q_PROPERTY(int sourceHeight MEMBER sourceHeight NOTIFY changed)
    Q_PROPERTY(double captureScale MEMBER captureScale NOTIFY changed)
    Q_PROPERTY(bool cameraAvailable MEMBER cameraAvailable NOTIFY changed)
    Q_PROPERTY(bool cameraVisible MEMBER cameraVisible NOTIFY changed)
    Q_PROPERTY(int cameraSourceWidth MEMBER cameraSourceWidth NOTIFY changed)
    Q_PROPERTY(int cameraSourceHeight MEMBER cameraSourceHeight NOTIFY changed)
    Q_PROPERTY(double time MEMBER time NOTIFY changed)
    Q_PROPERTY(QVariantMap project MEMBER project NOTIFY changed)
    Q_PROPERTY(QVariantMap zoom MEMBER zoom NOTIFY changed)
    Q_PROPERTY(QVariantMap layout MEMBER layout NOTIFY changed)
    Q_PROPERTY(QVariantMap media MEMBER media NOTIFY changed)
    Q_PROPERTY(QVariantMap cursor MEMBER cursor NOTIFY changed)
    Q_PROPERTY(QVariantList ripples MEMBER ripples NOTIFY changed)
    Q_PROPERTY(QVariantList keystrokePills MEMBER keystrokePills NOTIFY changed)
    Q_PROPERTY(bool softwareRendering MEMBER softwareRendering CONSTANT)
public:
    using QObject::QObject;
    int outputWidth = 0;
    int outputHeight = 0;
    int sourceWidth = 0;
    int sourceHeight = 0;
    double captureScale = 1.0;   // physical pixels per logical pixel at capture time
    bool cameraAvailable = false;
    bool cameraVisible = true;
    int cameraSourceWidth = 0;
    int cameraSourceHeight = 0;
    double time = 0.0;
    QVariantMap project;
    QVariantMap zoom;
    QVariantMap layout;
    QVariantMap media;
    QVariantMap cursor;
    QVariantList ripples;
    QVariantList keystrokePills;
    bool softwareRendering = false;
    void notifyChanged() { emit changed(); }
signals:
    void changed();
};

// Exports a bundle to MP4 or GIF. What to export and how is decided in Rust
// (crates/omareel_core/src/export, through crates/omareel_qt/src/export.rs); this class renders
// the composition with ExportRenderer, runs the event loop while it waits, and reports.
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
