#pragma once

#include "core/InputLog.h"
#include "core/MotionTrack.h"
#include "core/Project.h"

#include <QFutureWatcher>
#include <QJsonObject>
#include <QMediaPlayer>
#include <QObject>
#include <QTimer>
#include <QSet>
#include <QVariantList>
#include <QVariantMap>
#include <memory>

class QVideoSink;
class QAudioOutput;

namespace OmaRecord {

class Exporter;
class FrameSource;
class PreviewSink;

// QML receives a QVariantMap snapshot because style panels naturally address nested
// JSON fields. All writes return through setProjectValue()/timeline methods, keeping
// undo, autosave, MotionTrack invalidation, and validation in one place.
class Editor : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantMap project READ projectMap NOTIFY projectChanged)
    Q_PROPERTY(QVariantList clips READ clips NOTIFY projectChanged)
    Q_PROPERTY(QVariantList zooms READ zooms NOTIFY projectChanged)
    Q_PROPERTY(QString bundlePath READ bundlePath CONSTANT)
    Q_PROPERTY(QString bundleName READ bundleName NOTIFY projectChanged)
    Q_PROPERTY(double duration READ duration NOTIFY durationChanged)
    Q_PROPERTY(double sourceDuration READ sourceDuration CONSTANT)
    Q_PROPERTY(int sourceWidth READ sourceWidth CONSTANT)
    Q_PROPERTY(int sourceHeight READ sourceHeight CONSTANT)
    Q_PROPERTY(double fps READ fps CONSTANT)
    Q_PROPERTY(bool hasAudio READ hasAudio CONSTANT)
    Q_PROPERTY(bool hasDesktopAudio READ hasDesktopAudio CONSTANT)
    Q_PROPERTY(bool hasMicrophoneAudio READ hasMicrophoneAudio CONSTANT)
    Q_PROPERTY(bool hasCamera READ hasCamera CONSTANT)
    Q_PROPERTY(QVariantList waveform READ waveform NOTIFY waveformChanged)
    Q_PROPERTY(double position READ position NOTIFY positionChanged)
    Q_PROPERTY(bool playing READ playing NOTIFY playingChanged)
    Q_PROPERTY(bool canUndo READ canUndo NOTIFY historyChanged)
    Q_PROPERTY(bool canRedo READ canRedo NOTIFY historyChanged)
    Q_PROPERTY(bool dirty READ dirty NOTIFY dirtyChanged)
    Q_PROPERTY(QString selectedClipId READ selectedClipId WRITE setSelectedClipId NOTIFY selectionChanged)
    Q_PROPERTY(QString selectedZoomId READ selectedZoomId WRITE setSelectedZoomId NOTIFY selectionChanged)
    Q_PROPERTY(bool pickingZoomTarget READ pickingZoomTarget WRITE setPickingZoomTarget NOTIFY pickingZoomTargetChanged)
    Q_PROPERTY(QStringList presetNames READ presetNames NOTIFY presetsChanged)
    Q_PROPERTY(QVariantList wallpapers READ wallpapers NOTIFY wallpapersChanged)
    Q_PROPERTY(QVariantList gradients READ gradients CONSTANT)
    Q_PROPERTY(double exportProgress READ exportProgress NOTIFY exportProgressChanged)
    Q_PROPERTY(bool exporting READ exporting NOTIFY exportingChanged)
    Q_PROPERTY(QString exportError READ exportError NOTIFY exportErrorChanged)

    // Composition.qml contract; Editor is registered as both "editor" and "comp".
    Q_PROPERTY(int outputWidth READ outputWidth NOTIFY outputSizeChanged)
    Q_PROPERTY(int outputHeight READ outputHeight NOTIFY outputSizeChanged)
    Q_PROPERTY(double time READ sourcePosition NOTIFY compositionChanged)
    Q_PROPERTY(QVariantMap zoom READ previewZoom NOTIFY compositionChanged)
    Q_PROPERTY(QVariantMap cursor READ previewCursor NOTIFY compositionChanged)
    Q_PROPERTY(QVariantList ripples READ previewRipples NOTIFY compositionChanged)
    Q_PROPERTY(bool softwareRendering READ softwareRendering CONSTANT)

public:
    explicit Editor(const QString &bundlePath, QObject *parent = nullptr);
    ~Editor() override;

    bool isValid() const { return m_valid; }
    QString errorString() const { return m_error; }
    QVariantMap projectMap() const;
    QVariantList clips() const;
    QVariantList zooms() const;
    QString bundlePath() const { return m_bundlePath; }
    QString bundleName() const { return m_project.name; }
    double duration() const;
    double sourceDuration() const { return m_sourceDuration; }
    int sourceWidth() const { return m_sourceWidth; }
    int sourceHeight() const { return m_sourceHeight; }
    double fps() const { return m_fps; }
    bool hasAudio() const { return m_hasAudio; }
    bool hasDesktopAudio() const { return m_hasDesktopAudio; }
    bool hasMicrophoneAudio() const { return m_hasMicrophoneAudio; }
    bool hasCamera() const { return m_hasCamera; }
    QVariantList waveform() const { return m_waveform; }
    double position() const { return m_outputPosition; }
    bool playing() const;
    bool canUndo() const { return !m_undo.isEmpty(); }
    bool canRedo() const { return !m_redo.isEmpty(); }
    bool dirty() const { return m_dirty; }
    QString selectedClipId() const { return m_selectedClipId; }
    QString selectedZoomId() const { return m_selectedZoomId; }
    bool pickingZoomTarget() const { return m_pickingZoomTarget; }
    QStringList presetNames() const;
    QVariantList wallpapers() const;
    QVariantList gradients() const { return m_gradients; }
    double exportProgress() const { return m_exportProgress; }
    bool exporting() const { return m_exporting; }
    QString exportError() const { return m_exportError; }
    int outputWidth() const;
    int outputHeight() const;
    double sourcePosition() const;
    QVariantMap previewZoom() const;
    QVariantMap previewCursor() const;
    QVariantList previewRipples() const;
    bool softwareRendering() const;

    void setSelectedClipId(const QString &id);
    void setSelectedZoomId(const QString &id);
    void setPickingZoomTarget(bool value);

    Q_INVOKABLE void attachFrameSource(QObject *source);
    Q_INVOKABLE void setProjectValue(const QString &path, const QVariant &value, bool coalesce = false);
    Q_INVOKABLE void beginCoalescedEdit(const QString &key);
    Q_INVOKABLE void endCoalescedEdit();
    Q_INVOKABLE void undo();
    Q_INVOKABLE void redo();
    Q_INVOKABLE bool saveNow();
    Q_INVOKABLE void playPause();
    Q_INVOKABLE void play();
    Q_INVOKABLE void pause();
    Q_INVOKABLE void seek(double outputTime);
    Q_INVOKABLE void stepFrames(int frames);
    Q_INVOKABLE void seekBoundary(int direction);
    Q_INVOKABLE double outputToSource(double outputTime) const;
    Q_INVOKABLE double sourceToOutput(double sourceTime, int preferredClip = -1) const;
    Q_INVOKABLE bool splitAtPlayhead();
    Q_INVOKABLE bool trimClip(const QString &id, double newIn, double newOut);
    Q_INVOKABLE bool removeClip(const QString &id);
    Q_INVOKABLE bool mergeClip(const QString &id, int direction);
    Q_INVOKABLE bool resetClipTrims(const QString &id);
    Q_INVOKABLE bool setClipSpeed(const QString &id, double speed);
    Q_INVOKABLE QString addZoomAt(double outputTime, double length = 2.0);
    Q_INVOKABLE bool moveZoom(const QString &id, double sourceStart);
    Q_INVOKABLE bool resizeZoom(const QString &id, double sourceStart, double sourceEnd);
    Q_INVOKABLE bool setZoomLevel(const QString &id, double level, bool coalesce = false);
    Q_INVOKABLE bool setZoomTarget(const QString &id, const QVariant &target);
    Q_INVOKABLE bool setZoomTargetFromPreview(double x, double y);
    Q_INVOKABLE bool removeZoom(const QString &id);
    Q_INVOKABLE void regenerateZooms();
    Q_INVOKABLE void savePreset(const QString &name);
    Q_INVOKABLE void loadPreset(const QString &name);
    Q_INVOKABLE void deletePreset(const QString &name);
    Q_INVOKABLE void exportTo(const QString &path, const QVariantMap &settings = {});
    Q_INVOKABLE void cancelExport();
    Q_INVOKABLE QString defaultExportPath(const QString &format) const;
    Q_INVOKABLE QString formatTime(double seconds) const;
    Q_INVOKABLE void refreshOmarchyTheme();

signals:
    void projectChanged();
    void durationChanged();
    void positionChanged();
    void playingChanged();
    void historyChanged();
    void dirtyChanged();
    void selectionChanged();
    void pickingZoomTargetChanged();
    void presetsChanged();
    void wallpapersChanged();
    void waveformChanged();
    void outputSizeChanged();
    void compositionChanged();
    void exportProgressChanged();
    void exportingChanged();
    void exportErrorChanged();
    void exportFinished(const QString &path);
    void autosaved(const QString &path);
    void errorOccurred(const QString &message);

private:
    bool loadBundle();
    void snapshot(const QString &coalesceKey = {});
    void changed(bool motion = true);
    void restore(const QJsonObject &json);
    int clipIndex(const QString &id) const;
    int zoomIndex(const QString &id) const;
    int clipForOutput(double output, double *clipOutputStart = nullptr) const;
    void rebuildMotion();
    void applyMotionResult();
    void updatePreview();
    void handlePlayerPosition(qint64 milliseconds);
    void startWaveformBuild();
    QString presetsDirectory() const;
    QJsonObject stylePreset() const;

    QString m_bundlePath;
    QString m_projectPath;
    QString m_videoPath;
    Project m_project;
    InputLog m_input;
    double m_sourceDuration = 0.0;
    int m_sourceWidth = 0;
    int m_sourceHeight = 0;
    double m_fps = 60.0;
    bool m_hasAudio = false;
    bool m_hasDesktopAudio = false;
    bool m_hasMicrophoneAudio = false;
    bool m_hasCamera = false;
    bool m_valid = false;
    QString m_error;
    QMediaPlayer m_player;
    std::unique_ptr<QAudioOutput> m_audioOutput;
    std::unique_ptr<QVideoSink> m_videoSink;
    std::unique_ptr<PreviewSink> m_previewSink;
    double m_outputPosition = 0.0;
    int m_activeClip = 0;
    bool m_internalSeek = false;
    bool m_warmingPreview = false;
    QTimer m_autosaveTimer;
    QTimer m_motionTimer;
    QVector<QJsonObject> m_undo;
    QVector<QJsonObject> m_redo;
    QString m_coalesceKey;
    bool m_coalesceSnapshotTaken = false;
    bool m_dirty = false;
    QString m_selectedClipId;
    QString m_selectedZoomId;
    bool m_pickingZoomTarget = false;
    int m_motionGeneration = 0;
    int m_runningMotionGeneration = 0;
    QFutureWatcher<MotionTrack> m_motionWatcher;
    std::shared_ptr<const MotionTrack> m_motion;
    QVariantList m_waveform;
    QFutureWatcher<QVariantList> m_waveformWatcher;
    QVariantList m_gradients;
    mutable QVariantMap m_projectMapCache;
    mutable bool m_projectMapCacheValid = false;
    mutable QVariantList m_wallpapersCache;
    mutable bool m_wallpapersCacheValid = false;
    mutable QSet<QString> m_pendingThumbnails;
    Exporter *m_exporter = nullptr;
    double m_exportProgress = 0.0;
    bool m_exporting = false;
    QString m_exportError;
};

} // namespace OmaRecord
