#include "CameraCapture.h"

#include <QCamera>
#include <QCameraDevice>
#include <QCameraFormat>
#include <QDateTime>
#include <QFileInfo>
#include <QMediaCaptureSession>
#include <QMediaDevices>
#include <QMediaFormat>
#include <QMediaRecorder>
#include <QSaveFile>
#include <QTimer>
#include <QUrl>
#include <QVideoFrame>
#include <QVideoFrameInput>
#include <QVideoSink>
#include <algorithm>
#include <cmath>
#include <ctime>
#include <limits>

using namespace OmaRecord;

namespace {

qint64 monotonicUs()
{
    timespec value{};
    clock_gettime(CLOCK_MONOTONIC, &value);
    return qint64(value.tv_sec) * 1000000 + value.tv_nsec / 1000;
}

QCameraDevice selectedDevice(const QString &path)
{
    const QByteArray pathBytes = QFileInfo(path).absoluteFilePath().toUtf8();
    const QString baseName = QFileInfo(path).fileName();
    const QList<QCameraDevice> devices = QMediaDevices::videoInputs();
    for (const QCameraDevice &device : devices)
        if (device.id() == pathBytes) return device;
    for (const QCameraDevice &device : devices) {
        const QString id = QString::fromUtf8(device.id());
        if (id.contains(path) || id.endsWith(baseName)
            || device.description().contains(path, Qt::CaseInsensitive)
            || device.description().contains(baseName, Qt::CaseInsensitive))
            return device;
    }
    return {};
}

QCameraFormat selectedFormat(const QCameraDevice &device, int requestedHeight)
{
    QCameraFormat best;
    double bestScore = std::numeric_limits<double>::max();
    const QSize requested(requestedHeight == 720 ? 1280 : 1920,
                          requestedHeight == 720 ? 720 : 1080);
    for (const QCameraFormat &format : device.videoFormats()) {
        if (format.maxFrameRate() + 0.01 < 30.0) continue;
        const QSize size = format.resolution();
        const double sizePenalty = std::abs(size.width() - requested.width())
            + std::abs(size.height() - requested.height());
        const double frameRatePenalty = std::abs(format.maxFrameRate() - 30.0f) * 100.0;
        const double score = sizePenalty + frameRatePenalty;
        if (score < bestScore) {
            best = format;
            bestScore = score;
        }
    }
    return best;
}

} // namespace

FirstFrameTimestamp::FirstFrameTimestamp(QString path): m_path(std::move(path))
{
}

void FirstFrameTimestamp::setPath(const QString &path)
{
    m_path = path;
    m_firstFrameUs = 0;
}

bool FirstFrameTimestamp::recordFrameArrival(qint64 monotonicValue, qint64 realtimeUs)
{
    if (m_firstFrameUs > 0) return true;
    if (m_path.isEmpty() || monotonicValue <= 0 || realtimeUs <= 0) return false;
    QSaveFile file(m_path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) return false;
    file.write("monotonic_microsec\trealtime_microsec\n");
    file.write(QByteArray::number(monotonicValue) + '\t' + QByteArray::number(realtimeUs) + '\n');
    if (!file.commit()) return false;
    m_firstFrameUs = monotonicValue;
    return true;
}

CameraCapture::CameraCapture(const QString &devicePath, int requestedHeight,
                             const QString &videoPath, const QString &timestampPath,
                             QObject *parent)
    : QObject(parent), m_devicePath(devicePath),
      m_requestedHeight(requestedHeight == 720 ? 720 : 1080),
      m_videoPath(videoPath), m_timestamp(timestampPath),
      m_session(std::make_unique<QMediaCaptureSession>()),
      m_recordSession(std::make_unique<QMediaCaptureSession>()),
      m_recorder(std::make_unique<QMediaRecorder>()),
      m_startTimer(std::make_unique<QTimer>()),
      m_recordTimer(std::make_unique<QTimer>())
{
    m_startTimer->setSingleShot(true);
    m_startTimer->setInterval(2000);
    connect(m_startTimer.get(), &QTimer::timeout, this, [this] {
        if (!m_ready) fail(QStringLiteral("Camera did not produce a frame within two seconds"));
    });
    connect(m_recorder.get(), &QMediaRecorder::errorOccurred, this,
            [this](QMediaRecorder::Error, const QString &message) { fail(message); });
    connect(m_recorder.get(), &QMediaRecorder::recorderStateChanged, this,
            [this](QMediaRecorder::RecorderState state) {
        if (state != QMediaRecorder::StoppedState || !m_stopping) return;
        if (m_camera) m_camera->stop();
        m_running = false;
        emit runningChanged();
        emit stopped();
    });
    m_recordTimer->setTimerType(Qt::PreciseTimer);
    m_recordTimer->setInterval(1000 / 30);
    connect(m_recordTimer.get(), &QTimer::timeout, this, &CameraCapture::sendLatestFrame);
}

CameraCapture::~CameraCapture()
{
    if (m_recorder && m_recorder->recorderState() != QMediaRecorder::StoppedState)
        m_recorder->stop();
    if (m_camera) m_camera->stop();
}

void CameraCapture::attachVideoOutput(QObject *output)
{
    m_session->setVideoOutput(output);
    if (QVideoSink *sink = m_session->videoSink())
        connect(sink, &QVideoSink::videoFrameChanged, this, &CameraCapture::handleFrame,
                Qt::UniqueConnection);
}

void CameraCapture::start()
{
    if (m_running) return;
    const QCameraDevice device = selectedDevice(m_devicePath);
    if (device.isNull()) {
        fail(QStringLiteral("Selected camera is unavailable"));
        return;
    }
    m_camera = std::make_unique<QCamera>(device);
    connect(m_camera.get(), &QCamera::errorOccurred, this,
            [this](QCamera::Error, const QString &message) { fail(message); });
    const QCameraFormat format = selectedFormat(device, m_requestedHeight);
    if (format.isNull()) {
        fail(QStringLiteral("Selected camera has no 30 fps capture format"));
        return;
    }
    m_camera->setCameraFormat(format);
    m_captureSize = format.resolution();
    m_frameRate = std::min<qreal>(30.0, format.maxFrameRate());
    emit captureSizeChanged();

    QMediaFormat mediaFormat(QMediaFormat::MPEG4);
    mediaFormat.setVideoCodec(QMediaFormat::VideoCodec::H264);
    m_recorder->setMediaFormat(mediaFormat);
    m_recorder->setQuality(QMediaRecorder::HighQuality);
    m_recorder->setEncodingMode(QMediaRecorder::ConstantQualityEncoding);
    m_recorder->setVideoResolution(m_captureSize);
    m_recorder->setVideoFrameRate(30.0);
    m_recorder->setOutputLocation(QUrl::fromLocalFile(m_videoPath));
    m_frameInput = std::make_unique<QVideoFrameInput>();
    connect(m_frameInput.get(), &QVideoFrameInput::readyToSendVideoFrame, this, [this] {
        m_inputReady = true;
        if (m_camera && !m_camera->isActive() && !m_stopping) {
            m_camera->start();
        }
    });
    m_session->setCamera(m_camera.get());
    m_recordSession->setVideoFrameInput(m_frameInput.get());
    m_recordSession->setRecorder(m_recorder.get());
    m_running = true;
    m_stopping = false;
    emit runningChanged();
    m_startTimer->start();
    m_recorder->record();
}

void CameraCapture::stop()
{
    if (!m_running) {
        if (m_camera) m_camera->stop();
        emit stopped();
        return;
    }
    m_startTimer->stop();
    m_recordTimer->stop();
    m_stopping = true;
    if (m_frameInput) m_frameInput->sendVideoFrame({});
    m_recorder->stop();
}

void CameraCapture::handleFrame(const QVideoFrame &frame)
{
    if (!frame.isValid()) return;
    const qint64 timestamp = monotonicUs();
    if (!m_ready) {
        if (!m_timestamp.recordFrameArrival(timestamp, QDateTime::currentMSecsSinceEpoch() * 1000)) {
            fail(QStringLiteral("Could not write camera timestamp"));
            return;
        }
        m_ready = true;
        m_startTimer->stop();
        emit readyChanged();
    }
    m_latestFrame = std::make_unique<QVideoFrame>(frame);
    if (!m_recordTimer->isActive()) m_recordTimer->start();
}

void CameraCapture::sendLatestFrame()
{
    if (!m_frameInput || !m_latestFrame || !m_inputReady || m_stopping) return;
    QVideoFrame recordedFrame(*m_latestFrame);
    const qint64 startTime = m_recordedFrameCount * 1000000 / 30;
    recordedFrame.setStartTime(startTime);
    recordedFrame.setEndTime(startTime + 1000000 / 30);
    if (m_frameInput->sendVideoFrame(recordedFrame)) {
        m_inputReady = false;
        ++m_recordedFrameCount;
    }
}

void CameraCapture::fail(const QString &message)
{
    if (!m_errorString.isEmpty()) return;
    m_startTimer->stop();
    m_errorString = message.isEmpty() ? QStringLiteral("Camera capture failed") : message;
    emit errorOccurred(m_errorString);
    if (m_recorder->recorderState() != QMediaRecorder::StoppedState) {
        m_stopping = true;
        m_recorder->stop();
    } else {
        if (m_camera) m_camera->stop();
        m_running = false;
        emit runningChanged();
        emit stopped();
    }
}
