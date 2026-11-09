#ifndef OMAREEL_MEDIA_H
#define OMAREEL_MEDIA_H

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

// When each frame of the first video stream starts, in seconds and in order, in `*times`
// (freed with omr_free); the count, or -1 when the file can't be read.
long omr_frame_times(const char *path, double **times);

void omr_free(void *pointer);

// `ffmpeg -ss <at> -i <path> -frames:v 1 -vf scale=W:H:force_original_aspect_ratio=increase,
// crop=W:H -q:v <quality> <output>`, decoding with `threads` threads. 0 when written.
int omr_thumbnail(const char *path, double at, int width, int height, int quality, int threads, const char *output);

#endif
