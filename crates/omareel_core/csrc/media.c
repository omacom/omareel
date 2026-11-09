// The editor's questions about its videos, answered in process with FFmpeg's libraries rather
// than by starting ffprobe or ffmpeg (each 30-70 ms of process start and probing before any
// work): what a file holds, when each of its frames starts, and one small JPEG frame of it.
// src/media_av.rs wraps these; any failure there falls back to the programs, so their answers
// and error messages stay what they were.

#define _POSIX_C_SOURCE 200809L
#include "media.h"

#include <libavutil/imgutils.h>
#include "ffmpeg_dl.h"

#include <limits.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void set_error(char *error, size_t size, const char *path, int code)
{
    char text[AV_ERROR_MAX_STRING_SIZE] = {0};
    av_strerror(code, text, sizeof text);
    snprintf(error, size, "%s: %s", path, text);
}

static double seconds(int64_t value, AVRational base)
{
    return value == AV_NOPTS_VALUE ? -1.0 : (double)value * av_q2d(base);
}

int omr_probe(const char *path, struct omr_media_info *info)
{
    memset(info, 0, sizeof *info);
    info->stream_duration = -1.0;
    info->format_duration = -1.0;
    if (omr_ff_load() != 0) {
        snprintf(info->error, sizeof info->error, "%s: %s", path, omr_ff_error());
        return AVERROR(ENOSYS);
    }
    AVFormatContext *format = NULL;
    int code = avformat_open_input(&format, path, NULL, NULL);
    if (code < 0) {
        set_error(info->error, sizeof info->error, path, code);
        return code;
    }
    // ffprobe's r_frame_rate and avg_frame_rate are what this works out.
    code = avformat_find_stream_info(format, NULL);
    if (code < 0) {
        set_error(info->error, sizeof info->error, path, code);
        avformat_close_input(&format);
        return code;
    }
    if (format->duration != AV_NOPTS_VALUE) info->format_duration = (double)format->duration / AV_TIME_BASE;
    for (unsigned i = 0; i < format->nb_streams; ++i) {
        AVStream *stream = format->streams[i];
        const AVCodecParameters *codec = stream->codecpar;
        if (codec->codec_type == AVMEDIA_TYPE_AUDIO) info->has_audio = 1;
        if (codec->codec_type != AVMEDIA_TYPE_VIDEO || info->has_video) continue;
        info->has_video = 1;
        info->width = codec->width;
        info->height = codec->height;
        AVRational aspect = av_guess_sample_aspect_ratio(format, stream, NULL);
        info->sar_num = aspect.num;
        info->sar_den = aspect.den;
        info->r_num = stream->r_frame_rate.num;
        info->r_den = stream->r_frame_rate.den;
        info->avg_num = stream->avg_frame_rate.num;
        info->avg_den = stream->avg_frame_rate.den;
        info->stream_duration = seconds(stream->duration, stream->time_base);
        info->color_space = codec->color_space;
        const AVDictionaryEntry *rotate = av_dict_get(stream->metadata, "rotate", NULL, 0);
        if (rotate) {
            info->has_rotate_tag = 1;
            snprintf(info->rotate_tag, sizeof info->rotate_tag, "%s", rotate->value);
        }
        const AVPacketSideData *matrix = av_packet_side_data_get(codec->coded_side_data, codec->nb_coded_side_data,
                                                                 AV_PKT_DATA_DISPLAYMATRIX);
        if (matrix && matrix->size >= 9 * sizeof(int32_t)) {
            double rotation = av_display_rotation_get((const int32_t *)matrix->data);
            info->has_rotation = 1;
            info->rotation = isnan(rotation) ? 0 : (int)rotation;
        }
    }
    avformat_close_input(&format);
    return 0;
}

static int compare_doubles(const void *a, const void *b)
{
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

long omr_frame_times(const char *path, double **times)
{
    *times = NULL;
    if (omr_ff_load() != 0) return -1;
    AVFormatContext *format = NULL;
    // The index and the packets need no decoding, so no avformat_find_stream_info.
    if (avformat_open_input(&format, path, NULL, NULL) < 0) return -1;
    int index = -1;
    // ffprobe -select_streams v:0: the first video stream, which a recording's only one is.
    for (unsigned i = 0; i < format->nb_streams; ++i) {
        if (format->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
            index = (int)i;
            break;
        }
    }
    if (index < 0) {
        avformat_close_input(&format);
        return -1;
    }
    AVStream *stream = format->streams[index];
    const AVRational base = stream->time_base;
    long capacity = 1024, count = 0;
    double *out = malloc(sizeof(double) * (size_t)capacity);
    if (!out) {
        avformat_close_input(&format);
        return -1;
    }
    // An MP4's index holds every frame's time without reading the file (a recording is
    // gigabytes). It gives decode times; they are the presentation times, which ffprobe
    // prints, when no frame is reordered: checked on the first packets, else every packet is
    // read as ffprobe does.
    const int entries = avformat_index_get_entries_count(stream);
    int trust_index = entries > 0;
    AVPacket *packet = av_packet_alloc();
    for (int checked = 0; trust_index && checked < 32 && checked < entries;) {
        if (av_read_frame(format, packet) < 0) break;
        if (packet->stream_index == index) {
            const AVIndexEntry *entry = avformat_index_get_entry(stream, checked);
            if (!entry || packet->pts == AV_NOPTS_VALUE || packet->pts != entry->timestamp) trust_index = 0;
            ++checked;
        }
        av_packet_unref(packet);
    }
    if (trust_index) {
        for (int i = 0; i < entries; ++i) {
            const AVIndexEntry *entry = avformat_index_get_entry(stream, i);
            if (!entry || (entry->flags & AVINDEX_DISCARD_FRAME)) continue;
            if (count == capacity) {
                capacity *= 2;
                double *grown = realloc(out, sizeof(double) * (size_t)capacity);
                if (!grown) goto fail;
                out = grown;
            }
            out[count++] = (double)entry->timestamp * av_q2d(base);
        }
    } else {
        avformat_seek_file(format, -1, INT64_MIN, 0, 0, AVSEEK_FLAG_BACKWARD);
        count = 0;
        while (av_read_frame(format, packet) >= 0) {
            if (packet->stream_index == index && packet->pts != AV_NOPTS_VALUE) {
                if (count == capacity) {
                    capacity *= 2;
                    double *grown = realloc(out, sizeof(double) * (size_t)capacity);
                    if (!grown) goto fail;
                    out = grown;
                }
                out[count++] = (double)packet->pts * av_q2d(base);
            }
            av_packet_unref(packet);
        }
    }
    av_packet_free(&packet);
    avformat_close_input(&format);
    qsort(out, (size_t)count, sizeof(double), compare_doubles);
    *times = out;
    return count;
fail:
    av_packet_free(&packet);
    avformat_close_input(&format);
    free(out);
    return -1;
}

void omr_free(void *pointer)
{
    free(pointer);
}

// The frame `-ss <seconds> -i <path> -frames:v 1` gives: from the keyframe before the time,
// the first frame at or after it.
long omr_keyframe_times(const char *path, double **times)
{
    *times = NULL;
    if (omr_ff_load() != 0) return -1;
    AVFormatContext *format = NULL;
    if (avformat_open_input(&format, path, NULL, NULL) < 0) return -1;
    int index = -1;
    for (unsigned i = 0; i < format->nb_streams; ++i) {
        if (format->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
            index = (int)i;
            break;
        }
    }
    if (index < 0) {
        avformat_close_input(&format);
        return -1;
    }
    AVStream *stream = format->streams[index];
    const double base = av_q2d(stream->time_base);
    const double start = format->start_time == AV_NOPTS_VALUE ? 0 : (double)format->start_time / AV_TIME_BASE;
    const int entries = avformat_index_get_entries_count(stream);
    double *out = malloc(sizeof(double) * (size_t)(entries > 0 ? entries : 1));
    long count = 0;
    for (int i = 0; out && i < entries; ++i) {
        const AVIndexEntry *entry = avformat_index_get_entry(stream, i);
        if (entry && (entry->flags & AVINDEX_KEYFRAME)) out[count++] = (double)entry->timestamp * base - start;
    }
    avformat_close_input(&format);
    if (!out) return -1;
    *times = out;
    return count;
}

static AVFrame *frame_at(AVFormatContext *format, AVCodecContext *decoder, int index, double at)
{
    AVStream *stream = format->streams[index];
    int64_t start = format->start_time == AV_NOPTS_VALUE ? 0 : format->start_time;
    int64_t timestamp = (int64_t)llround(at * AV_TIME_BASE) + start;
    if (avformat_seek_file(format, -1, INT64_MIN, timestamp, timestamp, 0) < 0) return NULL;
    const int64_t target = av_rescale_q(timestamp, AV_TIME_BASE_Q, stream->time_base);
    // Frames before the one wanted are decoded only to reach it: they skip the deblocking
    // filter (a quarter of the decoding), which leaves the frame wanted a little blockier,
    // invisibly at 160x90. The last quarter second's frames before it are filtered again.
    const int64_t near = target - av_rescale_q(1, (AVRational){1, 4}, stream->time_base);
    AVPacket *packet = av_packet_alloc();
    AVFrame *frame = av_frame_alloc();
    AVFrame *found = NULL;
    int ended = 0;
    while (!found) {
        int code = avcodec_receive_frame(decoder, frame);
        if (code == 0) {
            int64_t time = frame->best_effort_timestamp;
            if (time == AV_NOPTS_VALUE || time >= target) {
                found = frame;
                frame = NULL;
            }
            continue;
        }
        if (code != AVERROR(EAGAIN) || ended) break;
        code = av_read_frame(format, packet);
        if (code < 0) {
            avcodec_send_packet(decoder, NULL);
            ended = 1;
            continue;
        }
        if (packet->stream_index == index) {
            decoder->skip_loop_filter = packet->dts != AV_NOPTS_VALUE && packet->dts < near ? AVDISCARD_ALL : AVDISCARD_DEFAULT;
            avcodec_send_packet(decoder, packet);
        }
        av_packet_unref(packet);
    }
    av_packet_free(&packet);
    av_frame_free(&frame);
    return found;
}

int omr_thumbnail(const char *path, double at, int width, int height, int quality, int threads, const char *output)
{
    int result = -1;
    if (omr_ff_load() != 0) return -1;
    AVFormatContext *format = NULL;
    AVCodecContext *decoder = NULL, *encoder = NULL;
    AVFrame *frame = NULL, *small = NULL;
    AVPacket *packet = NULL;
    struct SwsContext *scaler = NULL;
    FILE *file = NULL;
    if (avformat_open_input(&format, path, NULL, NULL) < 0) goto done;
    int index = -1;
    for (unsigned i = 0; i < format->nb_streams; ++i) {
        if (format->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
            index = (int)i;
            break;
        }
    }
    if (index < 0) goto done;
    for (unsigned i = 0; i < format->nb_streams; ++i)
        if ((int)i != index) format->streams[i]->discard = AVDISCARD_ALL;
    const AVCodec *codec = avcodec_find_decoder(format->streams[index]->codecpar->codec_id);
    if (!codec || !(decoder = avcodec_alloc_context3(codec))) goto done;
    if (avcodec_parameters_to_context(decoder, format->streams[index]->codecpar) < 0) goto done;
    decoder->thread_count = threads;
    decoder->pkt_timebase = format->streams[index]->time_base;
    decoder->flags2 |= AV_CODEC_FLAG2_FAST;
    if (avcodec_open2(decoder, codec, NULL) < 0) goto done;
    if (!(frame = frame_at(format, decoder, index, at))) goto done;

    // scale=W:H:force_original_aspect_ratio=increase,crop=W:H: cover the box, keep the middle.
    int cover_w = width, cover_h = height;
    if ((int64_t)frame->width * height > (int64_t)frame->height * width)
        cover_w = (int)llround((double)frame->width * height / frame->height);
    else
        cover_h = (int)llround((double)frame->height * width / frame->width);
    cover_w = cover_w < width ? width : cover_w;
    cover_h = cover_h < height ? height : cover_h;
    scaler = sws_getContext(frame->width, frame->height, frame->format, cover_w, cover_h, AV_PIX_FMT_YUV420P,
                            SWS_BICUBIC, NULL, NULL, NULL);
    small = av_frame_alloc();
    if (!scaler || !small) goto done;
    // JPEG's full range out, from the video's own (limited, as recordings are, unless it says).
    {
        int *table_in, *table_out, range_in, range_out, brightness, contrast, saturation;
        if (sws_getColorspaceDetails(scaler, &table_in, &range_in, &table_out, &range_out, &brightness, &contrast,
                                     &saturation) >= 0)
            sws_setColorspaceDetails(scaler, table_in, frame->color_range == AVCOL_RANGE_JPEG, table_out, 1, brightness,
                                     contrast, saturation);
    }
    small->format = AV_PIX_FMT_YUV420P;
    small->color_range = AVCOL_RANGE_JPEG;
    small->width = cover_w;
    small->height = cover_h;
    if (av_frame_get_buffer(small, 0) < 0) goto done;
    sws_scale(scaler, (const uint8_t *const *)frame->data, frame->linesize, 0, frame->height, small->data, small->linesize);
    // The crop: offsets even, as the chroma planes are half size.
    const int left = ((cover_w - width) / 2) & ~1, top = ((cover_h - height) / 2) & ~1;
    small->data[0] += top * small->linesize[0] + left;
    small->data[1] += (top / 2) * small->linesize[1] + left / 2;
    small->data[2] += (top / 2) * small->linesize[2] + left / 2;
    small->width = width;
    small->height = height;

    // -q:v <quality>: MJPEG at a fixed quantiser.
    const AVCodec *jpeg = avcodec_find_encoder(AV_CODEC_ID_MJPEG);
    if (!jpeg || !(encoder = avcodec_alloc_context3(jpeg))) goto done;
    encoder->width = width;
    encoder->height = height;
    encoder->pix_fmt = AV_PIX_FMT_YUV420P;
    encoder->color_range = AVCOL_RANGE_JPEG;
    encoder->time_base = (AVRational){1, 25};
    encoder->flags |= AV_CODEC_FLAG_QSCALE;
    encoder->global_quality = FF_QP2LAMBDA * quality;
    if (avcodec_open2(encoder, jpeg, NULL) < 0) goto done;
    small->quality = encoder->global_quality;
    small->pict_type = AV_PICTURE_TYPE_NONE;
    small->pts = 0;
    if (avcodec_send_frame(encoder, small) < 0) goto done;
    packet = av_packet_alloc();
    if (!packet || avcodec_receive_packet(encoder, packet) < 0) goto done;
    if (!(file = fopen(output, "wb"))) goto done;
    if (fwrite(packet->data, 1, (size_t)packet->size, file) != (size_t)packet->size) goto done;
    if (fclose(file) != 0) {
        file = NULL;
        goto done;
    }
    file = NULL;
    result = 0;
done:
    if (file) fclose(file);
    // The crop moved the plane pointers inside the buffer; av_frame_free frees by its buffer
    // references, not by them.
    av_packet_free(&packet);
    av_frame_free(&small);
    av_frame_free(&frame);
    sws_freeContext(scaler);
    avcodec_free_context(&encoder);
    avcodec_free_context(&decoder);
    avformat_close_input(&format);
    return result;
}

// ---------------------------------------------------------------------------------------------
// A playing video: what `ffmpeg [-hwaccel vaapi] -ss <start> -noaccurate_seek -i <path>
// -vf fps=<fps>:start_time=0,scale=<w>:<h>:flags=bilinear,format=nv12 -f rawvideo -` hands the
// preview, but kept open, so a seek is a flush and a decode from the keyframe rather than a new
// process opening the file and the GPU again.

struct omr_stream {
    AVFormatContext *format;
    AVCodecContext *decoder;
    AVBufferRef *device;
    struct SwsContext *scaler;
    int index;
    int width, height;
    int scaler_format, scaler_width, scaler_height;
    AVPacket *packet;
    AVFrame *current;   // the newest frame at or before the slot
    AVFrame *pending;   // the next decoded frame, past the slot
    AVFrame *download;  // a GPU frame copied back to memory
    double current_time, pending_time;
    int has_current, has_pending, ended;
    double start, fps;
    long slot;
    int positioned;     // a seek has been made: current_time says where decoding is
    int preview;        // hand over the first decoded frame (the keyframe) before the slot's
};

static enum AVPixelFormat pick_vaapi(AVCodecContext *context, const enum AVPixelFormat *formats)
{
    (void)context;
    for (const enum AVPixelFormat *f = formats; *f != AV_PIX_FMT_NONE; ++f)
        if (*f == AV_PIX_FMT_VAAPI) return *f;
    return formats[0];
}

struct omr_stream *omr_stream_open(const char *path, const char *vaapi_device, int width, int height, int threads)
{
    // NV12's colour plane is half the width in pairs: an odd size would overrun its rows.
    if (width < 2 || height < 2 || (width & 1) || (height & 1)) return NULL;
    if (omr_ff_load() != 0) return NULL;
    struct omr_stream *s = calloc(1, sizeof *s);
    if (!s) return NULL;
    s->width = width;
    s->height = height;
    s->index = -1;
    if (avformat_open_input(&s->format, path, NULL, NULL) < 0) goto fail;
    for (unsigned i = 0; i < s->format->nb_streams; ++i) {
        if (s->format->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO && s->index < 0) s->index = (int)i;
        else s->format->streams[i]->discard = AVDISCARD_ALL;
    }
    if (s->index < 0) goto fail;
    const AVCodec *codec = avcodec_find_decoder(s->format->streams[s->index]->codecpar->codec_id);
    if (!codec || !(s->decoder = avcodec_alloc_context3(codec))) goto fail;
    if (avcodec_parameters_to_context(s->decoder, s->format->streams[s->index]->codecpar) < 0) goto fail;
    s->decoder->pkt_timebase = s->format->streams[s->index]->time_base;
    if (vaapi_device && *vaapi_device) {
        if (av_hwdevice_ctx_create(&s->device, AV_HWDEVICE_TYPE_VAAPI, vaapi_device, NULL, 0) < 0) goto fail;
        s->decoder->hw_device_ctx = av_buffer_ref(s->device);
        s->decoder->get_format = pick_vaapi;
        // The frame shown and the next one stay held beside the decoder's own.
        s->decoder->extra_hw_frames = 4;
    } else {
        s->decoder->thread_count = threads;
    }
    if (avcodec_open2(s->decoder, codec, NULL) < 0) goto fail;
    s->packet = av_packet_alloc();
    s->current = av_frame_alloc();
    s->pending = av_frame_alloc();
    s->download = av_frame_alloc();
    if (!s->packet || !s->current || !s->pending || !s->download) goto fail;
    return s;
fail:
    omr_stream_close(s);
    return NULL;
}

void omr_stream_close(struct omr_stream *s)
{
    if (!s) return;
    av_frame_free(&s->current);
    av_frame_free(&s->pending);
    av_frame_free(&s->download);
    av_packet_free(&s->packet);
    sws_freeContext(s->scaler);
    avcodec_free_context(&s->decoder);
    av_buffer_unref(&s->device);
    avformat_close_input(&s->format);
    free(s);
}

// Whether decoding on from where it is reaches `start` without passing a keyframe (from which a
// seek would start for less).
static int continues_to(struct omr_stream *s, double start)
{
    if (!s->positioned || !s->has_current || s->ended || start + 1e-6 < s->current_time) return 0;
    AVStream *stream = s->format->streams[s->index];
    const double origin = s->format->start_time == AV_NOPTS_VALUE ? 0 : (double)s->format->start_time / AV_TIME_BASE;
    int64_t target = (int64_t)llround((start + origin) / av_q2d(stream->time_base));
    int key = av_index_search_timestamp(stream, target, AVSEEK_FLAG_BACKWARD);
    if (key < 0) return start - s->current_time < 1.0;
    const AVIndexEntry *entry = avformat_index_get_entry(stream, key);
    if (!entry) return 0;
    double key_time = entry->timestamp * av_q2d(stream->time_base) - origin;
    return key_time <= s->current_time + 1e-6;
}

int omr_stream_seek(struct omr_stream *s, double start, double fps, int preview)
{
    s->fps = fps > 0 ? fps : 60;
    s->slot = 0;
    s->start = start;
    if (continues_to(s, start)) {
        // The frames before the new start are passed over by the slot logic.
        s->preview = 0;
        return 0;
    }
    int64_t origin = s->format->start_time == AV_NOPTS_VALUE ? 0 : s->format->start_time;
    int64_t timestamp = (int64_t)llround(start * AV_TIME_BASE) + origin;
    int code = avformat_seek_file(s->format, -1, INT64_MIN, timestamp, timestamp, 0);
    if (code < 0) return code;
    avcodec_flush_buffers(s->decoder);
    av_frame_unref(s->current);
    av_frame_unref(s->pending);
    s->has_current = s->has_pending = s->ended = 0;
    s->positioned = 1;
    s->preview = preview;
    return 0;
}

// The next decoded frame into `pending`; 0 at the end.
static int decode_next(struct omr_stream *s)
{
    AVStream *stream = s->format->streams[s->index];
    const double origin = s->format->start_time == AV_NOPTS_VALUE ? 0 : (double)s->format->start_time / AV_TIME_BASE;
    for (;;) {
        int code = avcodec_receive_frame(s->decoder, s->pending);
        if (code == 0) {
            int64_t pts = s->pending->best_effort_timestamp;
            s->pending_time = pts == AV_NOPTS_VALUE ? s->current_time : pts * av_q2d(stream->time_base) - origin;
            s->has_pending = 1;
            return 1;
        }
        if (code == AVERROR_EOF || s->ended == 2 || code != AVERROR(EAGAIN)) {
            s->ended = 2;
            return 0;
        }
        if (s->ended) {
            s->ended = 2;
            continue;
        }
        code = av_read_frame(s->format, s->packet);
        if (code < 0) {
            avcodec_send_packet(s->decoder, NULL);
            s->ended = 1;
            continue;
        }
        if (s->packet->stream_index == s->index) avcodec_send_packet(s->decoder, s->packet);
        av_packet_unref(s->packet);
    }
}

int omr_stream_next(struct omr_stream *s, uint8_t *y, uint8_t *uv, double *time)
{
    const double slot_time = s->start + (double)s->slot / s->fps;
    const double half = 0.5 / s->fps;
    int preview = 0;
    if (s->preview) {
        // After a seek while paused: the first frame decoded (the keyframe) goes out at once,
        // at its own time, if the frame wanted is more than two frames on.
        s->preview = 0;
        if (!s->has_pending && !decode_next(s)) return 0;
        if (s->pending_time < slot_time - 2.0 / s->fps) {
            AVFrame *swap = s->current;
            s->current = s->pending;
            s->pending = swap;
            av_frame_unref(s->pending);
            s->current_time = s->pending_time;
            s->has_current = 1;
            s->has_pending = 0;
            preview = 1;
        }
    }
    if (!preview) {
        // The fps filter's choice: the newest frame at or before the slot (to half a frame).
        for (;;) {
            if (!s->has_pending && !decode_next(s)) break;
            if (s->pending_time > slot_time + half && s->has_current) break;
            AVFrame *swap = s->current;
            s->current = s->pending;
            s->pending = swap;
            av_frame_unref(s->pending);
            s->current_time = s->pending_time;
            s->has_current = 1;
            s->has_pending = 0;
            if (s->current_time > slot_time + half) break;
        }
        if (!s->has_current) return 0;
        // Past the last frame by more than a frame: the end.
        if (!s->has_pending && s->ended == 2 && slot_time > s->current_time + 1.0 / s->fps) return 0;
    }
    AVFrame *source = s->current;
    if (source->format == AV_PIX_FMT_VAAPI) {
        av_frame_unref(s->download);
        if (av_hwframe_transfer_data(s->download, source, 0) < 0) return -1;
        source = s->download;
    }
    if (!s->scaler || s->scaler_format != source->format || s->scaler_width != source->width
        || s->scaler_height != source->height) {
        sws_freeContext(s->scaler);
        s->scaler = sws_getContext(source->width, source->height, source->format, s->width, s->height, AV_PIX_FMT_NV12,
                                   SWS_BILINEAR, NULL, NULL, NULL);
        if (!s->scaler) return -1;
        s->scaler_format = source->format;
        s->scaler_width = source->width;
        s->scaler_height = source->height;
    }
    uint8_t *planes[4] = {y, uv, NULL, NULL};
    int strides[4] = {s->width, s->width, 0, 0};
    sws_scale(s->scaler, (const uint8_t *const *)source->data, source->linesize, 0, source->height, planes, strides);
    if (preview) {
        *time = s->current_time;
        return 1;
    }
    *time = slot_time;
    ++s->slot;
    return 1;
}

const char *omr_color_space_name(int space)
{
    if (omr_ff_load() != 0) return NULL;
    return av_color_space_name((enum AVColorSpace)space);
}

int omr_ff_preload(void)
{
    return omr_ff_load();
}
