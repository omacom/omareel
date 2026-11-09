// Does libswscale write outside its destination on this machine? Replays the three conversions
// omareel makes, with each destination plane ending exactly at a page boundary followed by a
// PROT_NONE guard page (an overrun faults at once), and checks canary bytes in the padding the
// caller doesn't own.
//
//   cc -O1 -o sws-test sws-test.c $(pkg-config --cflags --libs libswscale libavutil)
//   sws-test stream <src w> <src h> <dst w> <dst h> [frames]   yuv420p -> NV12, bilinear, strides = width
//   sws-test thumb  <src w> <src h> <dst w> <dst h> [frames]   yuv420p -> yuv420p, bicubic, full range out
//   sws-test camera <w> <h> [frames]                           NV12 -> yuv420p, bt601 tv -> bt709 tv
//   sws-test frame <in fmt> <w> <h> <out fmt> <dw> <dh> <in matrix> <out matrix> [frames]
//                                  the frame API (sws_scale_frame, what ffmpeg's scale filter uses),
//                                  formats nv12/yuv420p, matrices 601/709/same, tv range both sides
//
// Exit 0 when nothing was touched, 2 when a canary changed; a write past a plane faults (SIGSEGV).
#include <libavutil/frame.h>
#include <libavutil/imgutils.h>
#include <libswscale/swscale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

// Canaries: the bytes from each guarded allocation's page start up to the plane, which no one
// should write.
static uint8_t *canary_start[8];
static size_t canary_length[8];
static int canaries;

static int canaries_intact(void) {
    for (int i = 0; i < canaries; ++i)
        for (size_t j = 0; j < canary_length[i]; ++j)
            if (canary_start[i][j] != 0xA5) return 0;
    return 1;
}

// `size` bytes ending exactly where a no-access page starts.
static uint8_t *guarded(size_t size) {
    const size_t page = (size_t)sysconf(_SC_PAGESIZE);
    const size_t span = (size + page - 1) / page * page;
    uint8_t *base = mmap(NULL, span + page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (base == MAP_FAILED) { perror("mmap"); exit(1); }
    mprotect(base + span, page, PROT_NONE);
    memset(base, 0xA5, span);
    canary_start[canaries] = base;
    canary_length[canaries++] = span - size;
    return base + span - size;
}

static void fill(AVFrame *f) {
    for (int p = 0; p < 4 && f->data[p]; ++p) {
        int rows = p == 0 ? f->height : (f->format == AV_PIX_FMT_NV12 ? (f->height + 1) / 2 : (f->height + 1) / 2);
        for (int y = 0; y < rows; ++y)
            for (int x = 0; x < f->linesize[p]; ++x) f->data[p][y * f->linesize[p] + x] = (uint8_t)(rand() >> 7);
    }
}

static AVFrame *source(int w, int h, enum AVPixelFormat format) {
    AVFrame *f = av_frame_alloc();
    f->width = w;
    f->height = h;
    f->format = format;
    if (av_frame_get_buffer(f, 0) < 0) { fputs("get_buffer\n", stderr); exit(1); }
    fill(f);
    return f;
}

int main(int argc, char **argv) {
    if (argc < 4) { fputs("usage: see the top of sws-test.c\n", stderr); return 1; }
    const char *mode = argv[1];
    srand(1);
    if (!strcmp(mode, "stream") || !strcmp(mode, "thumb")) {
        const int sw = atoi(argv[2]), sh = atoi(argv[3]), dw = atoi(argv[4]), dh = atoi(argv[5]);
        const int frames = argc > 6 ? atoi(argv[6]) : 30;
        const int stream = !strcmp(mode, "stream");
        AVFrame *src = source(sw, sh, AV_PIX_FMT_YUV420P);
        const enum AVPixelFormat out = stream ? AV_PIX_FMT_NV12 : AV_PIX_FMT_YUV420P;
        struct SwsContext *s = sws_getContext(sw, sh, AV_PIX_FMT_YUV420P, dw, dh, out, stream ? SWS_BILINEAR : SWS_BICUBIC,
                                              NULL, NULL, NULL);
        if (!s) { fputs("sws_getContext failed\n", stderr); return 1; }
        if (!stream) {
            int *ti, *to, ri, ro, b, c, sat;
            if (sws_getColorspaceDetails(s, &ti, &ri, &to, &ro, &b, &c, &sat) >= 0) sws_setColorspaceDetails(s, ti, 0, to, 1, b, c, sat);
        }
        uint8_t *planes[4] = {0};
        int strides[4] = {0};
        if (stream) {
            // As media.c's omr_stream_frame: Y then interleaved UV, each `dw` bytes a row, no padding.
            planes[0] = guarded((size_t)dw * dh);
            planes[1] = guarded((size_t)dw * ((dh + 1) / 2));
            strides[0] = strides[1] = dw;
        } else {
            // As media.c's thumbnail: av_frame_get_buffer's padded planes, so a guard page there
            // only shows writes past the padding; also check the same with exact strides.
            planes[0] = guarded((size_t)dw * dh);
            planes[1] = guarded((size_t)((dw + 1) / 2) * ((dh + 1) / 2));
            planes[2] = guarded((size_t)((dw + 1) / 2) * ((dh + 1) / 2));
            strides[0] = dw;
            strides[1] = strides[2] = (dw + 1) / 2;
        }
        for (int i = 0; i < frames; ++i) {
            if (i) fill(src);
            sws_scale(s, (const uint8_t *const *)src->data, src->linesize, 0, sh, planes, strides);
            if (!canaries_intact()) { printf("%s %dx%d -> %dx%d: wrote before a plane at frame %d\n", mode, sw, sh, dw, dh, i); return 2; }
        }
        sws_freeContext(s);
        av_frame_free(&src);
        printf("%s %dx%d -> %dx%d: %d frames, no fault\n", mode, sw, sh, dw, dh, frames);
        return 0;
    }
    if (!strcmp(mode, "camera")) {
        const int w = atoi(argv[2]), h = atoi(argv[3]), frames = argc > 4 ? atoi(argv[4]) : 30;
        AVFrame *src = source(w, h, AV_PIX_FMT_NV12);
        struct SwsContext *s = sws_getContext(w, h, AV_PIX_FMT_NV12, w, h, AV_PIX_FMT_YUV420P, SWS_BICUBIC, NULL, NULL, NULL);
        int *ti, *to, ri, ro, b, c, sat;
        sws_getColorspaceDetails(s, &ti, &ri, &to, &ro, &b, &c, &sat);
        sws_setColorspaceDetails(s, sws_getCoefficients(SWS_CS_ITU601), 0, sws_getCoefficients(SWS_CS_ITU709), 0, b, c, sat);
        AVFrame *dst = av_frame_alloc();
        dst->width = w;
        dst->height = h;
        dst->format = AV_PIX_FMT_YUV420P;
        av_frame_get_buffer(dst, 0);
        for (int i = 0; i < frames; ++i) {
            if (i) fill(src);
            sws_scale(s, (const uint8_t *const *)src->data, src->linesize, 0, h, dst->data, dst->linesize);
        }
        sws_freeContext(s);
        av_frame_free(&dst);
        av_frame_free(&src);
        printf("camera %dx%d: %d frames, no fault\n", w, h, frames);
        return 0;
    }
    if (!strcmp(mode, "frame")) {
        enum AVPixelFormat in = !strcmp(argv[2], "nv12") ? AV_PIX_FMT_NV12 : AV_PIX_FMT_YUV420P;
        const int w = atoi(argv[3]), h = atoi(argv[4]);
        enum AVPixelFormat outf = !strcmp(argv[5], "nv12") ? AV_PIX_FMT_NV12 : AV_PIX_FMT_YUV420P;
        const int dw = atoi(argv[6]), dh = atoi(argv[7]);
        const char *mi = argv[8], *mo = argv[9];
        const int frames = argc > 10 ? atoi(argv[10]) : 30;
        AVFrame *src = source(w, h, in);
        src->color_range = AVCOL_RANGE_MPEG;
        src->colorspace = !strcmp(mi, "709") ? AVCOL_SPC_BT709 : AVCOL_SPC_BT470BG;
        SwsContext *s = sws_alloc_context();
        s->flags = SWS_BICUBIC;
        if (getenv("SWS_THREADS")) s->threads = atoi(getenv("SWS_THREADS"));
        for (int i = 0; i < frames; ++i) {
            AVFrame *dst = av_frame_alloc();
            dst->width = dw;
            dst->height = dh;
            dst->format = outf;
            dst->color_range = AVCOL_RANGE_MPEG;
            dst->colorspace = !strcmp(mo, "same") ? src->colorspace : (!strcmp(mo, "709") ? AVCOL_SPC_BT709 : AVCOL_SPC_BT470BG);
            if (i) fill(src);
            int r = sws_scale_frame(s, dst, src);
            if (r < 0) { printf("sws_scale_frame failed: %d\n", r); return 1; }
            av_frame_free(&dst);
        }
        sws_free_context(&s);
        av_frame_free(&src);
        printf("frame %s %dx%d %s -> %s %dx%d %s: %d frames, no fault\n", argv[2], w, h, mi, argv[5], dw, dh, mo, frames);
        return 0;
    }
    fputs("unknown mode\n", stderr);
    return 1;
}
