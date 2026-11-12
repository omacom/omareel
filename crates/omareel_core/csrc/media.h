#ifndef OMAREEL_MEDIA_H
#define OMAREEL_MEDIA_H

#include <stddef.h>

// See media.c. Kept in step with src/media_av.rs.

struct omr_media_info {
    int has_video;
    int has_audio;
    int width;
    int height;
    int sar_num;
    int sar_den;
    int r_num;
    int r_den;
    int avg_num;
    int avg_den;
    double stream_duration; // seconds; below 0 when the stream has none
    double format_duration; // seconds; below 0 when the file has none
    int color_space;        // AVColorSpace
    int has_rotation;       // a display matrix, turned by `rotation` degrees
    int rotation;
    int has_rotate_tag;     // the old "rotate" tag, as written
    char rotate_tag[32];
    char error[512];        // "<path>: <reason>", as ffprobe prints it
};

// What ffprobe's -show_entries stream=...:format=duration would say. 0, or a negative AVERROR
// with `error` set.
int omr_probe(const char *path, struct omr_media_info *info);

// The length in seconds of the first audio stream's sound (what `-map 0:a:0` plays), in
// `*duration`: the stream's own length, not the file's (a video's sound can end before its
// picture). Read from the packets when the header gives no length (a WebM written to a pipe)
// or only one guessed from the bitrate (an MP3 without a header). 1 when the file has an audio
// stream (`*duration` below 0 when it holds no sound), 0 when it has none, or a negative
// AVERROR with `error` set.
int omr_audio_duration(const char *path, double *duration, char *error, size_t error_size);

// When each frame of the first video stream starts, in seconds and in order, in `*times`
// (freed with omr_free); the count, or -1 when the file can't be read.
long omr_frame_times(const char *path, double **times);

// When each keyframe of the first video stream starts, in seconds from the file's start (as
// omr_thumbnail's `at`), from the index alone; the count, or -1. Freed with omr_free.
long omr_keyframe_times(const char *path, double **times);

void omr_free(void *pointer);

// av_color_space_name, or NULL.
const char *omr_color_space_name(int space);

// Opens FFmpeg's libraries now (they are opened the first time they are needed otherwise; see
// ffmpeg_dl.h), so a thread can do it while the caller gets on: 0 when they are.
int omr_ff_preload(void);

// `ffmpeg -ss <at> -i <path> -frames:v 1 -vf scale=W:H:force_original_aspect_ratio=increase,
// crop=W:H -q:v <quality> <output>`, decoding with `threads` threads. 0 when written.
int omr_thumbnail(const char *path, double at, int width, int height, int quality, int threads, const char *output);

// A video decoded for the preview, kept open between seeks (see media.c).
#include <stdint.h>
struct omr_stream;
struct omr_stream *omr_stream_open(const char *path, const char *vaapi_device, int width, int height, int threads);
// Positions the stream so the next frames are for start, start + 1/fps, ... Decodes on from
// where it is when no keyframe lies between. With `preview`, the keyframe a seek starts from is
// handed over first (at its own time) when the frame for `start` is further on.
int omr_stream_seek(struct omr_stream *stream, double start, double fps, int preview);
// The frame for the next slot as NV12 at the stream's size (`y` width*height bytes, `uv` half
// that) and its time: 1, or 0 at the end, or a negative error.
int omr_stream_next(struct omr_stream *stream, uint8_t *y, uint8_t *uv, double *time);
void omr_stream_close(struct omr_stream *stream);

#endif
