#include "FrameSource.h"

#include <QMatrix4x4>
#include <QMutexLocker>
#include <QQuickWindow>
#include <QSGGeometryNode>
#include <QSGMaterial>
#include <QSGMaterialShader>
#include <QSGSimpleTextureNode>
#include <QSGTexture>
#include <QPainter>
#include <QPainterPath>
#include <algorithm>
#include <array>
#include <cstring>
#include <rhi/qrhi.h>

using namespace Omareel;

namespace {

// One plane of a YUV frame as a single-channel texture that stays from frame to frame: the
// next frame's bytes are uploaded into it (from the decoder's buffer, without a copy) instead of
// a new texture being made for each, as createTextureFromImage does.
class YuvPlaneTexture : public QSGTexture
{
public:
    ~YuvPlaneTexture() override
    {
        if (m_texture) m_texture->deleteLater();
    }
    qint64 comparisonKey() const override { return qint64(quintptr(this)); }
    QRhiTexture *rhiTexture() const override { return m_texture; }
    QSize textureSize() const override { return m_size; }
    bool hasAlphaChannel() const override { return false; }
    bool hasMipmaps() const override { return false; }

    // The plane `data` (`size`, tightly packed rows), kept alive by `owner` until replaced.
    void setPlane(const std::shared_ptr<const void> &owner, const uchar *data, QSize size)
    {
        m_owner = owner;
        m_data = data;
        m_size = size;
        m_dirty = true;
    }

    void commitTextureOperations(QRhi *rhi, QRhiResourceUpdateBatch *resourceUpdates) override
    {
        if (!m_texture || m_texture->pixelSize() != m_size) {
            if (m_texture) m_texture->deleteLater();
            m_texture = rhi->newTexture(QRhiTexture::R8, m_size);
            if (!m_texture->create()) {
                delete m_texture;
                m_texture = nullptr;
                return;
            }
        }
        if (!m_dirty || !m_data) return;
        // fromRawData: the upload reads the decoder's buffer, which `m_owner` keeps until the
        // next frame, after this frame has been rendered.
        const QByteArray bytes = QByteArray::fromRawData(reinterpret_cast<const char *>(m_data),
                                                         qsizetype(m_size.width()) * m_size.height());
        resourceUpdates->uploadTexture(m_texture, QRhiTextureUploadEntry(0, 0, QRhiTextureSubresourceUploadDescription(bytes)));
        m_dirty = false;
    }

private:
    QRhiTexture *m_texture = nullptr;
    std::shared_ptr<const void> m_owner;
    const uchar *m_data = nullptr;
    QSize m_size;
    bool m_dirty = false;
};

// YUV to RGB for the shader, rows R, G, B: the matrix's Kr and Kb, with limited range
// (16-235, 16-240) or full range scaling first, as swscale converts.
QMatrix4x4 yuvToRgb(bool bt709, bool fullRange)
{
    const float kr = bt709 ? 0.2126f : 0.299f, kb = bt709 ? 0.0722f : 0.114f, kg = 1.0f - kr - kb;
    const float ys = fullRange ? 1.0f : 255.0f / 219.0f, yo = fullRange ? 0.0f : -16.0f / 219.0f;
    const float cs = fullRange ? 1.0f : 255.0f / 224.0f, co = fullRange ? -128.0f / 255.0f : -128.0f / 224.0f;
    const float a = 2.0f * (1.0f - kr), d = 2.0f * (1.0f - kb);
    const float b = -2.0f * kb * (1.0f - kb) / kg, c = -2.0f * kr * (1.0f - kr) / kg;
    return QMatrix4x4(ys, 0.0f, a * cs, yo + a * co,
                      ys, b * cs, c * cs, yo + (b + c) * co,
                      ys, d * cs, 0.0f, yo + d * co,
                      0.0f, 0.0f, 0.0f, 1.0f);
}

class YuvMaterial : public QSGMaterial
{
public:
    YuvMaterial() { setFlag(Blending, true); }
    QSGMaterialType *type() const override
    {
        static QSGMaterialType type;
        return &type;
    }
    QSGMaterialShader *createShader(QSGRendererInterface::RenderMode) const override;
    int compare(const QSGMaterial *other) const override
    {
        return int(this > other) - int(this < other);
    }
    std::array<YuvPlaneTexture, 3> planes;
    QMatrix4x4 colorMatrix;
};

class YuvShader : public QSGMaterialShader
{
public:
    YuvShader()
    {
        setShaderFileName(VertexStage, QStringLiteral(":/omareel/shaders/yuv.vert.qsb"));
        setShaderFileName(FragmentStage, QStringLiteral(":/omareel/shaders/yuv.frag.qsb"));
    }
    bool updateUniformData(RenderState &state, QSGMaterial *newMaterial, QSGMaterial *) override
    {
        QByteArray *buffer = state.uniformData();
        Q_ASSERT(buffer->size() >= 132);
        if (state.isMatrixDirty()) std::memcpy(buffer->data(), state.combinedMatrix().constData(), 64);
        std::memcpy(buffer->data() + 64, static_cast<YuvMaterial *>(newMaterial)->colorMatrix.constData(), 64);
        if (state.isOpacityDirty()) {
            const float opacity = state.opacity();
            std::memcpy(buffer->data() + 128, &opacity, 4);
        }
        return true;
    }
    void updateSampledImage(RenderState &state, int binding, QSGTexture **texture, QSGMaterial *newMaterial,
                            QSGMaterial *) override
    {
        if (binding < 1 || binding > 3) return;
        // As Qt's own texture materials do: the renderer samples what this hands back but leaves
        // making and filling it to the material.
        YuvPlaneTexture *plane = &static_cast<YuvMaterial *>(newMaterial)->planes[binding - 1];
        plane->commitTextureOperations(state.rhi(), state.resourceUpdateBatch());
        *texture = plane;
    }
};

QSGMaterialShader *YuvMaterial::createShader(QSGRendererInterface::RenderMode) const
{
    return new YuvShader;
}

class YuvNode : public QSGGeometryNode
{
public:
    YuvNode() : m_geometry(QSGGeometry::defaultAttributes_TexturedPoint2D(), 4)
    {
        setGeometry(&m_geometry);
        setMaterial(&m_material);
    }
    YuvMaterial m_material;

private:
    QSGGeometry m_geometry;
};

} // namespace

FrameSource::FrameSource(QQuickItem *parent): QQuickItem(parent)
{
    setFlag(ItemHasContents, true);
    m_image = QImage(1, 1, QImage::Format_RGBA8888);
    m_image.fill(Qt::black);
    ++m_revision;
}

void FrameSource::setImage(const QImage &image)
{
    {
        QMutexLocker lock(&m_mutex);
        m_image = image;
        m_yuv = YuvFrame();
        ++m_revision;
    }
    update();
}

void FrameSource::setYuvFrame(const YuvFrame &frame)
{
    {
        QMutexLocker lock(&m_mutex);
        m_yuv = frame;
        m_image = QImage();
        ++m_revision;
    }
    update();
}

void FrameSource::setVideoFrame(const QVideoFrame &frame)
{
    // QVideoFrame::toImage() may perform a GPU readback. Preview can later gain a native
    // texture path; the copy keeps this first implementation correct and deterministic.
    setImage(frame.toImage());
}

void FrameSource::setCropRect(const QRectF &value)
{
    const QRectF normalized(std::clamp(value.x(), 0.0, 1.0),
                            std::clamp(value.y(), 0.0, 1.0),
                            std::clamp(value.width(), 0.0001, 1.0),
                            std::clamp(value.height(), 0.0001, 1.0));
    if (m_cropRect == normalized) return;
    {
        QMutexLocker lock(&m_mutex);
        m_cropRect = normalized;
        ++m_revision;
    }
    emit cropRectChanged();
    update();
}

void FrameSource::setCornerRadiusRatio(qreal value)
{
    value = std::clamp(value, 0.0, 0.5);
    if (qFuzzyCompare(m_cornerRadiusRatio, value)) return;
    {
        QMutexLocker lock(&m_mutex);
        m_cornerRadiusRatio = value;
        ++m_revision;
    }
    emit cornerRadiusRatioChanged();
    update();
}

QSGNode *FrameSource::updateYuvNode(QSGNode *oldNode, const YuvFrame &frame, const QRectF &crop, bool changed)
{
    auto *node = dynamic_cast<YuvNode *>(oldNode);
    if (!node) {
        delete oldNode;
        node = new YuvNode;
        changed = true;
    }
    if (changed) {
        const QSize luma(frame.width, frame.height), chroma(frame.width / 2, frame.height / 2);
        const qsizetype lumaBytes = qsizetype(luma.width()) * luma.height();
        const qsizetype chromaBytes = qsizetype(chroma.width()) * chroma.height();
        node->m_material.planes[0].setPlane(frame.owner, frame.planes, luma);
        node->m_material.planes[1].setPlane(frame.owner, frame.planes + lumaBytes, chroma);
        node->m_material.planes[2].setPlane(frame.owner, frame.planes + lumaBytes + chromaBytes, chroma);
        for (auto &plane : node->m_material.planes) plane.setFiltering(QSGTexture::Linear);
        node->m_material.colorMatrix = yuvToRgb(frame.bt709, frame.fullRange);
        node->markDirty(QSGNode::DirtyMaterial);
    }
    // The crop is the texture coordinates: no copy of the cropped part.
    QSGGeometry::updateTexturedRectGeometry(node->geometry(), boundingRect(), crop);
    node->markDirty(QSGNode::DirtyGeometry);
    return node;
}

QSGNode *FrameSource::updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *)
{
    QImage image;
    YuvFrame yuv;
    QRectF crop;
    qreal radiusRatio;
    quint64 revision;
    {
        QMutexLocker lock(&m_mutex);
        image = m_image;
        yuv = m_yuv;
        crop = m_cropRect;
        radiusRatio = m_cornerRadiusRatio;
        revision = m_revision;
    }
    if (!yuv.isNull() && window()) {
        QSGNode *yuvNode = updateYuvNode(oldNode, yuv, crop, revision != m_renderedRevision);
        m_renderedRevision = revision;
        return yuvNode;
    }
    auto *node = dynamic_cast<QSGSimpleTextureNode *>(oldNode);
    if (!node) {
        delete oldNode;
        node = new QSGSimpleTextureNode;
        m_renderedRevision = 0;
    }
    if (!image.isNull() && revision != m_renderedRevision && window()) {
        QRect pixelCrop(qRound(crop.x() * image.width()), qRound(crop.y() * image.height()),
                        qRound(crop.width() * image.width()), qRound(crop.height() * image.height()));
        pixelCrop = pixelCrop.intersected(image.rect());
        if (pixelCrop.isValid() && pixelCrop != image.rect()) image = image.copy(pixelCrop);
        if (radiusRatio > 0.0 && !image.isNull()) {
            QImage rounded(image.size(), QImage::Format_RGBA8888_Premultiplied);
            rounded.fill(Qt::transparent);
            QPainter painter(&rounded);
            painter.setRenderHint(QPainter::Antialiasing, true);
            const qreal radius = radiusRatio * std::min(image.width(), image.height());
            QPainterPath path;
            path.addRoundedRect(QRectF(0, 0, image.width(), image.height()), radius, radius);
            painter.setClipPath(path);
            painter.drawImage(0, 0, image);
            painter.end();
            image = std::move(rounded);
        }
        QSGTexture *texture = window()->createTextureFromImage(image);
        QSGTexture *oldTexture = node->texture();
        node->setOwnsTexture(false);
        node->setTexture(texture);
        delete oldTexture;
        node->setOwnsTexture(true);
        node->setFiltering(QSGTexture::Linear);
        m_renderedRevision = revision;
    }
    node->setRect(boundingRect());
    return node;
}
