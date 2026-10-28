/* The GPU side of the screen capture: buffers the compositor renders captured frames into
 * (GBM-allocated dmabufs on the compositor's device) and an encoder that crops, converts and
 * encodes them on the GPU with VA-API, writing fragmented MP4 through libavformat. Frames never
 * reach the CPU. `screen_capture/gpu.rs` is the only caller.
 *
 * Every function that can fail returns 0 on success and a negative value on failure, with a
 * message in `error` (NUL-terminated, cut to `error_size`). */
#ifndef OMAREEL_GPU_CAPTURE_H
#define OMAREEL_GPU_CAPTURE_H

#include <stddef.h>
#include <stdint.h>

#define OC_MAX_PLANES 4

/* An open DRM render node with a GBM device on it. */
typedef struct oc_device oc_device;

/* One allocated buffer and what the compositor needs to import it. The descriptors belong to
 * the buffer and are closed by oc_buffer_destroy. */
typedef struct {
    void *bo;
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint64_t modifier;
    int planes;
    int fds[OC_MAX_PLANES];
    uint32_t offsets[OC_MAX_PLANES];
    uint32_t strides[OC_MAX_PLANES];
} oc_buffer;

typedef struct {
    /* The buffers' size and DRM format. */
    uint32_t width;
    uint32_t height;
    uint32_t format;
    /* The part of the buffer that is encoded; even sizes. */
    uint32_t crop_x;
    uint32_t crop_y;
    uint32_t crop_width;
    uint32_t crop_height;
    int fps;
    /* Constant quantiser for every frame (0-51). */
    int qp;
    const char *path;
} oc_encoder_config;

typedef struct oc_encoder oc_encoder;

/* Opens the render node of the DRM device `device` (a dev_t; a primary node's is accepted). */
int oc_device_open(uint64_t device, oc_device **out, char *error, size_t error_size);
void oc_device_close(oc_device *device);
/* The kernel driver's name ("i915", "xe", "amdgpu", "nvidia-drm", ...). */
const char *oc_device_driver(const oc_device *device);
/* The render node's path. */
const char *oc_device_path(const oc_device *device);

/* Allocates a buffer the GPU renders into with one of `modifiers` (any the driver picks when
 * `count` is 0). */
int oc_buffer_create(oc_device *device, uint32_t width, uint32_t height, uint32_t format,
                     const uint64_t *modifiers, size_t count, oc_buffer *out, char *error,
                     size_t error_size);
void oc_buffer_destroy(oc_buffer *buffer);
/* Maps a buffer for reading on the CPU; *data points at its first row, rows `*stride` apart.
 * oc_buffer_unmap gives `*mapping` back. */
int oc_buffer_map(oc_buffer *buffer, const uint8_t **data, uint32_t *stride, void **mapping);
void oc_buffer_unmap(oc_buffer *buffer, void *mapping);

/* Opens the encoder and imports `buffers` (all made by oc_buffer_create on `device`, with the
 * config's size and format) so oc_encoder_submit can name them by index. */
int oc_encoder_open(oc_device *device, const oc_encoder_config *config, const oc_buffer *buffers,
                    size_t count, oc_encoder **out, char *error, size_t error_size);
/* Encodes buffer `index` as the frame at `pts` (in 1/fps). When it returns, the GPU has finished
 * reading the buffer, so the compositor may render into it again. */
int oc_encoder_submit(oc_encoder *encoder, size_t index, int64_t pts, char *error, size_t error_size);
/* Flushes the encoder and finishes the file. */
int oc_encoder_finish(oc_encoder *encoder, char *error, size_t error_size);
void oc_encoder_destroy(oc_encoder *encoder);

#endif
