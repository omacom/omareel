/* See gpu_capture.h. */
#define _GNU_SOURCE
#include "gpu_capture.h"

#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <gbm.h>
#include <xf86drm.h>

#include <libavcodec/avcodec.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavformat/avformat.h>
#include <libavutil/hwcontext.h>
#include <libavutil/hwcontext_drm.h>
#include <libavutil/hwcontext_vaapi.h>
#include <libavutil/opt.h>
#include <va/va.h>

#define OC_MAX_BUFFERS 16

struct oc_device {
    int fd;
    struct gbm_device *gbm;
    char path[64];
    char driver[32];
};

struct oc_encoder {
    oc_encoder_config config;
    AVBufferRef *drm_device;
    AVBufferRef *va_device;
    AVBufferRef *drm_frames;
    AVBufferRef *va_frames;
    VADisplay display;
    /* Each buffer imported once as a VA surface; a submitted frame is a new reference. */
    AVFrame *imported[OC_MAX_BUFFERS];
    size_t count;
    AVFilterGraph *graph;
    AVFilterContext *source;
    AVFilterContext *sink;
    AVFrame *filtered;
    AVCodecContext *codec;
    AVFormatContext *format;
    AVStream *stream;
    AVPacket *packet;
    int header_written;
};

static int fail(char *error, size_t size, const char *format, ...)
{
    if (error && size) {
        va_list arguments;
        va_start(arguments, format);
        vsnprintf(error, size, format, arguments);
        va_end(arguments);
    }
    return -1;
}

static int fail_av(char *error, size_t size, const char *what, int code)
{
    char text[AV_ERROR_MAX_STRING_SIZE] = {0};
    av_strerror(code, text, sizeof text);
    return fail(error, size, "%s: %s", what, text);
}

int oc_device_open(uint64_t device, oc_device **out, char *error, size_t error_size)
{
    *out = NULL;
    drmDevicePtr found = NULL;
    if (drmGetDeviceFromDevId((dev_t)device, 0, &found) != 0 || !found)
        return fail(error, error_size, "The capture device %llu is not a DRM device", (unsigned long long)device);
    if (!(found->available_nodes & (1 << DRM_NODE_RENDER))) {
        drmFreeDevice(&found);
        return fail(error, error_size, "The capture device has no render node");
    }
    oc_device *opened = calloc(1, sizeof *opened);
    if (!opened) {
        drmFreeDevice(&found);
        return fail(error, error_size, "Out of memory");
    }
    snprintf(opened->path, sizeof opened->path, "%s", found->nodes[DRM_NODE_RENDER]);
    drmFreeDevice(&found);
    opened->fd = open(opened->path, O_RDWR | O_CLOEXEC);
    if (opened->fd < 0) {
        fail(error, error_size, "Could not open %s: %s", opened->path, strerror(errno));
        free(opened);
        return -1;
    }
    drmVersionPtr version = drmGetVersion(opened->fd);
    if (version) {
        snprintf(opened->driver, sizeof opened->driver, "%s", version->name);
        drmFreeVersion(version);
    }
    opened->gbm = gbm_create_device(opened->fd);
    if (!opened->gbm) {
        close(opened->fd);
        free(opened);
        return fail(error, error_size, "Could not create a GBM device");
    }
    *out = opened;
    return 0;
}

void oc_device_close(oc_device *device)
{
    if (!device)
        return;
    gbm_device_destroy(device->gbm);
    close(device->fd);
    free(device);
}

const char *oc_device_driver(const oc_device *device)
{
    return device->driver;
}

const char *oc_device_path(const oc_device *device)
{
    return device->path;
}

int oc_buffer_create(oc_device *device, uint32_t width, uint32_t height, uint32_t format,
                     const uint64_t *modifiers, size_t count, oc_buffer *out, char *error,
                     size_t error_size)
{
    memset(out, 0, sizeof *out);
    for (int i = 0; i < OC_MAX_PLANES; i++)
        out->fds[i] = -1;
    struct gbm_bo *bo = count
        ? gbm_bo_create_with_modifiers2(device->gbm, width, height, format, modifiers, (unsigned)count,
                                        GBM_BO_USE_RENDERING)
        : gbm_bo_create(device->gbm, width, height, format, GBM_BO_USE_RENDERING);
    if (!bo)
        return fail(error, error_size, "Could not allocate a %ux%u capture buffer: %s", width, height, strerror(errno));
    out->bo = bo;
    out->width = width;
    out->height = height;
    out->format = format;
    out->modifier = gbm_bo_get_modifier(bo);
    out->planes = gbm_bo_get_plane_count(bo);
    if (out->planes < 1 || out->planes > OC_MAX_PLANES) {
        oc_buffer_destroy(out);
        return fail(error, error_size, "The capture buffer has %d planes", out->planes);
    }
    for (int i = 0; i < out->planes; i++) {
        out->fds[i] = gbm_bo_get_fd_for_plane(bo, i);
        out->offsets[i] = gbm_bo_get_offset(bo, i);
        out->strides[i] = gbm_bo_get_stride_for_plane(bo, i);
        if (out->fds[i] < 0) {
            oc_buffer_destroy(out);
            return fail(error, error_size, "Could not export the capture buffer");
        }
    }
    return 0;
}

void oc_buffer_destroy(oc_buffer *buffer)
{
    for (int i = 0; i < OC_MAX_PLANES; i++) {
        if (buffer->fds[i] >= 0)
            close(buffer->fds[i]);
        buffer->fds[i] = -1;
    }
    if (buffer->bo)
        gbm_bo_destroy(buffer->bo);
    buffer->bo = NULL;
}

int oc_buffer_map(oc_buffer *buffer, const uint8_t **data, uint32_t *stride, void **mapping)
{
    *mapping = NULL;
    void *address = gbm_bo_map(buffer->bo, 0, 0, buffer->width, buffer->height, GBM_BO_TRANSFER_READ, stride, mapping);
    if (!address)
        return -1;
    *data = address;
    return 0;
}

void oc_buffer_unmap(oc_buffer *buffer, void *mapping)
{
    if (mapping)
        gbm_bo_unmap(buffer->bo, mapping);
}

static void free_descriptor(void *opaque, uint8_t *data)
{
    (void)opaque;
    av_free(data);
}

/* The buffer as a DRM PRIME frame: one object per plane, one layer. */
static int drm_frame(oc_encoder *encoder, const oc_buffer *buffer, AVFrame *frame)
{
    AVDRMFrameDescriptor *descriptor = av_mallocz(sizeof *descriptor);
    if (!descriptor)
        return AVERROR(ENOMEM);
    descriptor->nb_objects = buffer->planes;
    descriptor->nb_layers = 1;
    descriptor->layers[0].format = buffer->format;
    descriptor->layers[0].nb_planes = buffer->planes;
    for (int i = 0; i < buffer->planes; i++) {
        off_t size = lseek(buffer->fds[i], 0, SEEK_END);
        descriptor->objects[i].fd = buffer->fds[i];
        descriptor->objects[i].size = size > 0 ? (size_t)size : 0;
        descriptor->objects[i].format_modifier = buffer->modifier;
        descriptor->layers[0].planes[i].object_index = i;
        descriptor->layers[0].planes[i].offset = buffer->offsets[i];
        descriptor->layers[0].planes[i].pitch = buffer->strides[i];
    }
    frame->format = AV_PIX_FMT_DRM_PRIME;
    frame->width = (int)buffer->width;
    frame->height = (int)buffer->height;
    frame->data[0] = (uint8_t *)descriptor;
    frame->buf[0] = av_buffer_create((uint8_t *)descriptor, sizeof *descriptor, free_descriptor, NULL, 0);
    frame->hw_frames_ctx = av_buffer_ref(encoder->drm_frames);
    if (!frame->buf[0] || !frame->hw_frames_ctx) {
        if (!frame->buf[0])
            av_free(descriptor);
        return AVERROR(ENOMEM);
    }
    return 0;
}

/* The source's colours: sRGB, full range. */
static void describe_source(AVFrame *frame)
{
    frame->color_range = AVCOL_RANGE_JPEG;
    frame->colorspace = AVCOL_SPC_RGB;
    frame->color_primaries = AVCOL_PRI_BT709;
    frame->color_trc = AVCOL_TRC_IEC61966_2_1;
}

static int open_filters(oc_encoder *encoder, char *error, size_t error_size)
{
    const oc_encoder_config *config = &encoder->config;
    encoder->graph = avfilter_graph_alloc();
    if (!encoder->graph)
        return fail(error, error_size, "Out of memory");
    encoder->source = avfilter_graph_alloc_filter(encoder->graph, avfilter_get_by_name("buffer"), "in");
    if (!encoder->source)
        return fail(error, error_size, "Could not create the capture filter input");
    AVBufferSrcParameters *parameters = av_buffersrc_parameters_alloc();
    if (!parameters)
        return fail(error, error_size, "Out of memory");
    parameters->format = AV_PIX_FMT_VAAPI;
    parameters->width = (int)config->width;
    parameters->height = (int)config->height;
    parameters->time_base = (AVRational){1, config->fps};
    parameters->sample_aspect_ratio = (AVRational){1, 1};
    parameters->hw_frames_ctx = encoder->va_frames;
    parameters->color_range = AVCOL_RANGE_JPEG;
    parameters->color_space = AVCOL_SPC_RGB;
    int result = av_buffersrc_parameters_set(encoder->source, parameters);
    av_free(parameters);
    if (result < 0 || (result = avfilter_init_dict(encoder->source, NULL)) < 0)
        return fail_av(error, error_size, "Could not configure the capture filter input", result);
    char arguments[256];
    AVFilterContext *scale = NULL;
    snprintf(arguments, sizeof arguments,
             "w=%u:h=%u:format=nv12:out_color_matrix=bt709:out_range=tv:out_color_primaries=bt709:"
             "out_color_transfer=bt709",
             config->crop_width, config->crop_height);
    result = avfilter_graph_create_filter(&scale, avfilter_get_by_name("scale_vaapi"), "convert", arguments, NULL,
                                          encoder->graph);
    if (result < 0)
        return fail_av(error, error_size, "Could not create the VA-API converter", result);
    result = avfilter_graph_create_filter(&encoder->sink, avfilter_get_by_name("buffersink"), "out", NULL, NULL,
                                          encoder->graph);
    if (result < 0)
        return fail_av(error, error_size, "Could not create the capture filter output", result);
    if ((result = avfilter_link(encoder->source, 0, scale, 0)) < 0
        || (result = avfilter_link(scale, 0, encoder->sink, 0)) < 0)
        return fail_av(error, error_size, "Could not link the capture filters", result);
    if ((result = avfilter_graph_config(encoder->graph, NULL)) < 0)
        return fail_av(error, error_size, "Could not configure the capture filters", result);
    return 0;
}

static int open_codec(oc_encoder *encoder, char *error, size_t error_size)
{
    const oc_encoder_config *config = &encoder->config;
    const AVCodec *codec = avcodec_find_encoder_by_name("h264_vaapi");
    if (!codec)
        return fail(error, error_size, "FFmpeg has no h264_vaapi encoder");
    encoder->codec = avcodec_alloc_context3(codec);
    if (!encoder->codec)
        return fail(error, error_size, "Out of memory");
    AVCodecContext *context = encoder->codec;
    context->width = (int)config->crop_width;
    context->height = (int)config->crop_height;
    context->pix_fmt = AV_PIX_FMT_VAAPI;
    context->sw_pix_fmt = AV_PIX_FMT_NV12;
    context->time_base = (AVRational){1, config->fps};
    context->framerate = (AVRational){config->fps, 1};
    context->sample_aspect_ratio = (AVRational){1, 1};
    /* A one-second GOP for clean seeking, and no B-frames, so a stop writes every frame. */
    context->gop_size = config->fps;
    context->max_b_frames = 0;
    context->color_range = AVCOL_RANGE_MPEG;
    context->colorspace = AVCOL_SPC_BT709;
    context->color_primaries = AVCOL_PRI_BT709;
    context->color_trc = AVCOL_TRC_BT709;
    context->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    AVBufferRef *frames = av_buffersink_get_hw_frames_ctx(encoder->sink);
    if (!frames)
        return fail(error, error_size, "The VA-API converter gave no frames context");
    context->hw_frames_ctx = av_buffer_ref(frames);
    AVDictionary *options = NULL;
    av_dict_set(&options, "rc_mode", "CQP", 0);
    av_dict_set_int(&options, "qp", config->qp, 0);
    int result = avcodec_open2(context, codec, &options);
    av_dict_free(&options);
    if (result < 0)
        return fail_av(error, error_size, "Could not open the VA-API encoder", result);
    return 0;
}

static int open_output(oc_encoder *encoder, char *error, size_t error_size)
{
    int result = avformat_alloc_output_context2(&encoder->format, NULL, "mp4", encoder->config.path);
    if (result < 0 || !encoder->format)
        return fail_av(error, error_size, "Could not create the screen video", result);
    encoder->stream = avformat_new_stream(encoder->format, NULL);
    if (!encoder->stream)
        return fail(error, error_size, "Out of memory");
    if ((result = avcodec_parameters_from_context(encoder->stream->codecpar, encoder->codec)) < 0)
        return fail_av(error, error_size, "Could not describe the screen video", result);
    encoder->stream->time_base = encoder->codec->time_base;
    if ((result = avio_open(&encoder->format->pb, encoder->config.path, AVIO_FLAG_WRITE)) < 0)
        return fail_av(error, error_size, "Could not open the screen video", result);
    /* Fragmented, flushed per packet, so the video stays playable up to its last fragment if
     * the recorder dies; see encoder_arguments for the ffmpeg path. */
    encoder->format->flush_packets = 1;
    AVDictionary *options = NULL;
    av_dict_set(&options, "movflags", "+frag_keyframe+empty_moov+default_base_moof", 0);
    result = avformat_write_header(encoder->format, &options);
    av_dict_free(&options);
    if (result < 0)
        return fail_av(error, error_size, "Could not start the screen video", result);
    encoder->header_written = 1;
    return 0;
}

int oc_encoder_open(oc_device *device, const oc_encoder_config *config, const oc_buffer *buffers,
                    size_t count, oc_encoder **out, char *error, size_t error_size)
{
    *out = NULL;
    if (count == 0 || count > OC_MAX_BUFFERS)
        return fail(error, error_size, "Bad capture buffer count %zu", count);
    oc_encoder *encoder = calloc(1, sizeof *encoder);
    if (!encoder)
        return fail(error, error_size, "Out of memory");
    encoder->config = *config;
    encoder->config.fps = config->fps > 0 ? config->fps : 60;
    /* The daemon's standard error is the recording's log; libav's warnings stay out of it. */
    av_log_set_level(AV_LOG_ERROR);
    int result = av_hwdevice_ctx_create(&encoder->drm_device, AV_HWDEVICE_TYPE_DRM, device->path, NULL, 0);
    if (result < 0) {
        fail_av(error, error_size, "Could not open the DRM device", result);
        goto failed;
    }
    result = av_hwdevice_ctx_create_derived(&encoder->va_device, AV_HWDEVICE_TYPE_VAAPI, encoder->drm_device, 0);
    if (result < 0) {
        fail_av(error, error_size, "Could not open VA-API", result);
        goto failed;
    }
    AVHWDeviceContext *va_device = (AVHWDeviceContext *)encoder->va_device->data;
    encoder->display = ((AVVAAPIDeviceContext *)va_device->hwctx)->display;

    encoder->drm_frames = av_hwframe_ctx_alloc(encoder->drm_device);
    if (!encoder->drm_frames) {
        fail(error, error_size, "Out of memory");
        goto failed;
    }
    AVHWFramesContext *drm_frames = (AVHWFramesContext *)encoder->drm_frames->data;
    drm_frames->format = AV_PIX_FMT_DRM_PRIME;
    drm_frames->sw_format = config->format == 0x34325241 /* AR24 */ ? AV_PIX_FMT_BGRA : AV_PIX_FMT_BGR0;
    drm_frames->width = (int)config->width;
    drm_frames->height = (int)config->height;
    if ((result = av_hwframe_ctx_init(encoder->drm_frames)) < 0) {
        fail_av(error, error_size, "Could not describe the capture buffers", result);
        goto failed;
    }
    result = av_hwframe_ctx_create_derived(&encoder->va_frames, AV_PIX_FMT_VAAPI, encoder->va_device,
                                           encoder->drm_frames, AV_HWFRAME_MAP_READ);
    if (result < 0) {
        fail_av(error, error_size, "VA-API cannot read the capture buffers", result);
        goto failed;
    }
    for (size_t i = 0; i < count; i++) {
        AVFrame *source = av_frame_alloc();
        encoder->imported[i] = av_frame_alloc();
        if (!source || !encoder->imported[i]) {
            av_frame_free(&source);
            fail(error, error_size, "Out of memory");
            goto failed;
        }
        encoder->count = i + 1;
        result = drm_frame(encoder, &buffers[i], source);
        if (result >= 0) {
            encoder->imported[i]->format = AV_PIX_FMT_VAAPI;
            encoder->imported[i]->hw_frames_ctx = av_buffer_ref(encoder->va_frames);
            result = av_hwframe_map(encoder->imported[i], source, AV_HWFRAME_MAP_READ);
        }
        av_frame_free(&source);
        if (result < 0) {
            fail_av(error, error_size, "VA-API could not import a capture buffer", result);
            goto failed;
        }
    }
    encoder->filtered = av_frame_alloc();
    encoder->packet = av_packet_alloc();
    if (!encoder->filtered || !encoder->packet) {
        fail(error, error_size, "Out of memory");
        goto failed;
    }
    if (open_filters(encoder, error, error_size) < 0 || open_codec(encoder, error, error_size) < 0
        || open_output(encoder, error, error_size) < 0)
        goto failed;
    *out = encoder;
    return 0;

failed:
    oc_encoder_destroy(encoder);
    return -1;
}

/* Writes the packets the encoder has ready. */
static int write_packets(oc_encoder *encoder, char *error, size_t error_size)
{
    for (;;) {
        int result = avcodec_receive_packet(encoder->codec, encoder->packet);
        if (result == AVERROR(EAGAIN) || result == AVERROR_EOF)
            return 0;
        if (result < 0)
            return fail_av(error, error_size, "The VA-API encoder failed", result);
        /* One tick; the fragment's last packet would otherwise have its duration guessed. */
        if (encoder->packet->duration <= 0)
            encoder->packet->duration = 1;
        av_packet_rescale_ts(encoder->packet, encoder->codec->time_base, encoder->stream->time_base);
        encoder->packet->stream_index = encoder->stream->index;
        result = av_write_frame(encoder->format, encoder->packet);
        av_packet_unref(encoder->packet);
        if (result < 0)
            return fail_av(error, error_size, "Could not write the screen video", result);
    }
}

/* Encodes what the converter has ready. Each converted frame is waited for first, so the
 * converter has read its capture buffer by the time this returns. */
static int encode_filtered(oc_encoder *encoder, char *error, size_t error_size)
{
    for (;;) {
        int result = av_buffersink_get_frame(encoder->sink, encoder->filtered);
        if (result == AVERROR(EAGAIN) || result == AVERROR_EOF)
            return 0;
        if (result < 0)
            return fail_av(error, error_size, "The VA-API converter failed", result);
        VASurfaceID surface = (VASurfaceID)(uintptr_t)encoder->filtered->data[3];
        VAStatus status = vaSyncSurface(encoder->display, surface);
        if (status != VA_STATUS_SUCCESS) {
            av_frame_unref(encoder->filtered);
            return fail(error, error_size, "The VA-API converter failed: %s", vaErrorStr(status));
        }
        result = avcodec_send_frame(encoder->codec, encoder->filtered);
        av_frame_unref(encoder->filtered);
        if (result < 0)
            return fail_av(error, error_size, "The VA-API encoder refused a frame", result);
        if (write_packets(encoder, error, error_size) < 0)
            return -1;
    }
}

int oc_encoder_submit(oc_encoder *encoder, size_t index, int64_t pts, char *error, size_t error_size)
{
    if (index >= encoder->count)
        return fail(error, error_size, "Bad capture buffer %zu", index);
    AVFrame *frame = av_frame_clone(encoder->imported[index]);
    if (!frame)
        return fail(error, error_size, "Out of memory");
    const oc_encoder_config *config = &encoder->config;
    frame->crop_left = config->crop_x;
    frame->crop_top = config->crop_y;
    frame->crop_right = config->width - config->crop_x - config->crop_width;
    frame->crop_bottom = config->height - config->crop_y - config->crop_height;
    frame->pts = pts;
    describe_source(frame);
    int result = av_buffersrc_add_frame_flags(encoder->source, frame, 0);
    av_frame_free(&frame);
    if (result < 0)
        return fail_av(error, error_size, "The VA-API converter refused a frame", result);
    return encode_filtered(encoder, error, error_size);
}

int oc_encoder_finish(oc_encoder *encoder, char *error, size_t error_size)
{
    int result = av_buffersrc_add_frame_flags(encoder->source, NULL, 0);
    if (result < 0)
        return fail_av(error, error_size, "Could not flush the VA-API converter", result);
    if (encode_filtered(encoder, error, error_size) < 0)
        return -1;
    if ((result = avcodec_send_frame(encoder->codec, NULL)) < 0)
        return fail_av(error, error_size, "Could not flush the VA-API encoder", result);
    if (write_packets(encoder, error, error_size) < 0)
        return -1;
    encoder->header_written = 0;
    if ((result = av_write_trailer(encoder->format)) < 0)
        return fail_av(error, error_size, "Could not finish the screen video", result);
    if ((result = avio_closep(&encoder->format->pb)) < 0)
        return fail_av(error, error_size, "Could not close the screen video", result);
    return 0;
}

void oc_encoder_destroy(oc_encoder *encoder)
{
    if (!encoder)
        return;
    if (encoder->format) {
        if (encoder->format->pb)
            avio_closep(&encoder->format->pb);
        avformat_free_context(encoder->format);
    }
    avcodec_free_context(&encoder->codec);
    avfilter_graph_free(&encoder->graph);
    av_frame_free(&encoder->filtered);
    av_packet_free(&encoder->packet);
    for (size_t i = 0; i < OC_MAX_BUFFERS; i++)
        av_frame_free(&encoder->imported[i]);
    av_buffer_unref(&encoder->va_frames);
    av_buffer_unref(&encoder->drm_frames);
    av_buffer_unref(&encoder->va_device);
    av_buffer_unref(&encoder->drm_device);
    free(encoder);
}
