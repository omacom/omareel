#ifndef OMAREEL_FFMPEG_DL_H
#define OMAREEL_FFMPEG_DL_H

/* FFmpeg's libraries, opened the first time omareel needs them instead of linked.
 *
 * libavcodec, libavformat and libavfilter bring about 140 libraries with them (every codec's),
 * and the dynamic loader binding them was most of the 35 to 45 ms every omareel process took
 * before main: `omareel record --stop`, the recorder, the launcher, the bar. media.c and
 * gpu_capture.c include this after FFmpeg's headers: each FFmpeg function they call becomes a
 * pointer in `omr_ff`, which omr_ff_load fills (once, from any thread) and their entry points
 * call first. Kept in step with the functions those files use; one missing fails the link. */

#include <libavcodec/avcodec.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavformat/avformat.h>
#include <libavutil/display.h>
#include <libavutil/hwcontext.h>
#include <libavutil/pixdesc.h>
#include <libswscale/swscale.h>

#define OMR_FF_FUNCTIONS(X) \
    X(avutil, av_buffer_create) \
    X(avutil, av_buffer_ref) \
    X(avutil, av_buffer_unref) \
    X(avutil, av_dict_free) \
    X(avutil, av_dict_get) \
    X(avutil, av_dict_set) \
    X(avutil, av_dict_set_int) \
    X(avutil, av_display_rotation_get) \
    X(avutil, av_frame_alloc) \
    X(avutil, av_frame_clone) \
    X(avutil, av_frame_free) \
    X(avutil, av_frame_get_buffer) \
    X(avutil, av_frame_unref) \
    X(avutil, av_free) \
    X(avutil, av_hwdevice_ctx_create) \
    X(avutil, av_hwdevice_ctx_create_derived) \
    X(avutil, av_hwframe_ctx_alloc) \
    X(avutil, av_hwframe_ctx_create_derived) \
    X(avutil, av_hwframe_ctx_init) \
    X(avutil, av_hwframe_map) \
    X(avutil, av_hwframe_transfer_data) \
    X(avutil, av_log_set_level) \
    X(avutil, av_mallocz) \
    X(avutil, av_rescale_q) \
    X(avutil, av_strerror) \
    X(avutil, av_color_space_name) \
    X(swscale, sws_freeContext) \
    X(swscale, sws_getColorspaceDetails) \
    X(swscale, sws_getContext) \
    X(swscale, sws_scale) \
    X(swscale, sws_setColorspaceDetails) \
    X(avcodec, av_packet_alloc) \
    X(avcodec, av_packet_free) \
    X(avcodec, av_packet_rescale_ts) \
    X(avcodec, av_packet_side_data_get) \
    X(avcodec, av_packet_unref) \
    X(avcodec, avcodec_alloc_context3) \
    X(avcodec, avcodec_find_decoder) \
    X(avcodec, avcodec_find_encoder) \
    X(avcodec, avcodec_find_encoder_by_name) \
    X(avcodec, avcodec_flush_buffers) \
    X(avcodec, avcodec_free_context) \
    X(avcodec, avcodec_open2) \
    X(avcodec, avcodec_parameters_from_context) \
    X(avcodec, avcodec_parameters_to_context) \
    X(avcodec, avcodec_receive_frame) \
    X(avcodec, avcodec_receive_packet) \
    X(avcodec, avcodec_send_frame) \
    X(avcodec, avcodec_send_packet) \
    X(avformat, av_guess_sample_aspect_ratio) \
    X(avformat, av_index_search_timestamp) \
    X(avformat, av_read_frame) \
    X(avformat, av_write_frame) \
    X(avformat, av_write_trailer) \
    X(avformat, avformat_alloc_output_context2) \
    X(avformat, avformat_close_input) \
    X(avformat, avformat_find_stream_info) \
    X(avformat, avformat_free_context) \
    X(avformat, avformat_index_get_entries_count) \
    X(avformat, avformat_index_get_entry) \
    X(avformat, avformat_new_stream) \
    X(avformat, avformat_open_input) \
    X(avformat, avformat_seek_file) \
    X(avformat, avformat_write_header) \
    X(avformat, avio_closep) \
    X(avformat, avio_open) \
    X(avfilter, av_buffersink_get_frame) \
    X(avfilter, av_buffersink_get_hw_frames_ctx) \
    X(avfilter, av_buffersrc_add_frame_flags) \
    X(avfilter, av_buffersrc_parameters_alloc) \
    X(avfilter, av_buffersrc_parameters_set) \
    X(avfilter, avfilter_get_by_name) \
    X(avfilter, avfilter_graph_alloc) \
    X(avfilter, avfilter_graph_alloc_filter) \
    X(avfilter, avfilter_graph_config) \
    X(avfilter, avfilter_graph_create_filter) \
    X(avfilter, avfilter_graph_free) \
    X(avfilter, avfilter_init_dict) \
    X(avfilter, avfilter_link) \

struct omr_ff_table {
#define OMR_FF_POINTER(lib, name) __typeof__(name) *name;
    OMR_FF_FUNCTIONS(OMR_FF_POINTER)
#undef OMR_FF_POINTER
};
extern struct omr_ff_table omr_ff;

/* 0 once every library and function is in `omr_ff`, else -1 (the same answer every call). */
int omr_ff_load(void);
/* What failed, for an error message ("" when loaded). */
const char *omr_ff_error(void);

#ifndef OMR_FF_NO_MACROS
#define av_buffer_create (omr_ff.av_buffer_create)
#define av_buffer_ref (omr_ff.av_buffer_ref)
#define av_buffer_unref (omr_ff.av_buffer_unref)
#define av_dict_free (omr_ff.av_dict_free)
#define av_dict_get (omr_ff.av_dict_get)
#define av_dict_set (omr_ff.av_dict_set)
#define av_dict_set_int (omr_ff.av_dict_set_int)
#define av_display_rotation_get (omr_ff.av_display_rotation_get)
#define av_frame_alloc (omr_ff.av_frame_alloc)
#define av_frame_clone (omr_ff.av_frame_clone)
#define av_frame_free (omr_ff.av_frame_free)
#define av_frame_get_buffer (omr_ff.av_frame_get_buffer)
#define av_frame_unref (omr_ff.av_frame_unref)
#define av_free (omr_ff.av_free)
#define av_hwdevice_ctx_create (omr_ff.av_hwdevice_ctx_create)
#define av_hwdevice_ctx_create_derived (omr_ff.av_hwdevice_ctx_create_derived)
#define av_hwframe_ctx_alloc (omr_ff.av_hwframe_ctx_alloc)
#define av_hwframe_ctx_create_derived (omr_ff.av_hwframe_ctx_create_derived)
#define av_hwframe_ctx_init (omr_ff.av_hwframe_ctx_init)
#define av_hwframe_map (omr_ff.av_hwframe_map)
#define av_hwframe_transfer_data (omr_ff.av_hwframe_transfer_data)
#define av_log_set_level (omr_ff.av_log_set_level)
#define av_mallocz (omr_ff.av_mallocz)
#define av_rescale_q (omr_ff.av_rescale_q)
#define av_strerror (omr_ff.av_strerror)
#define av_color_space_name (omr_ff.av_color_space_name)
#define sws_freeContext (omr_ff.sws_freeContext)
#define sws_getColorspaceDetails (omr_ff.sws_getColorspaceDetails)
#define sws_getContext (omr_ff.sws_getContext)
#define sws_scale (omr_ff.sws_scale)
#define sws_setColorspaceDetails (omr_ff.sws_setColorspaceDetails)
#define av_packet_alloc (omr_ff.av_packet_alloc)
#define av_packet_free (omr_ff.av_packet_free)
#define av_packet_rescale_ts (omr_ff.av_packet_rescale_ts)
#define av_packet_side_data_get (omr_ff.av_packet_side_data_get)
#define av_packet_unref (omr_ff.av_packet_unref)
#define avcodec_alloc_context3 (omr_ff.avcodec_alloc_context3)
#define avcodec_find_decoder (omr_ff.avcodec_find_decoder)
#define avcodec_find_encoder (omr_ff.avcodec_find_encoder)
#define avcodec_find_encoder_by_name (omr_ff.avcodec_find_encoder_by_name)
#define avcodec_flush_buffers (omr_ff.avcodec_flush_buffers)
#define avcodec_free_context (omr_ff.avcodec_free_context)
#define avcodec_open2 (omr_ff.avcodec_open2)
#define avcodec_parameters_from_context (omr_ff.avcodec_parameters_from_context)
#define avcodec_parameters_to_context (omr_ff.avcodec_parameters_to_context)
#define avcodec_receive_frame (omr_ff.avcodec_receive_frame)
#define avcodec_receive_packet (omr_ff.avcodec_receive_packet)
#define avcodec_send_frame (omr_ff.avcodec_send_frame)
#define avcodec_send_packet (omr_ff.avcodec_send_packet)
#define av_guess_sample_aspect_ratio (omr_ff.av_guess_sample_aspect_ratio)
#define av_index_search_timestamp (omr_ff.av_index_search_timestamp)
#define av_read_frame (omr_ff.av_read_frame)
#define av_write_frame (omr_ff.av_write_frame)
#define av_write_trailer (omr_ff.av_write_trailer)
#define avformat_alloc_output_context2 (omr_ff.avformat_alloc_output_context2)
#define avformat_close_input (omr_ff.avformat_close_input)
#define avformat_find_stream_info (omr_ff.avformat_find_stream_info)
#define avformat_free_context (omr_ff.avformat_free_context)
#define avformat_index_get_entries_count (omr_ff.avformat_index_get_entries_count)
#define avformat_index_get_entry (omr_ff.avformat_index_get_entry)
#define avformat_new_stream (omr_ff.avformat_new_stream)
#define avformat_open_input (omr_ff.avformat_open_input)
#define avformat_seek_file (omr_ff.avformat_seek_file)
#define avformat_write_header (omr_ff.avformat_write_header)
#define avio_closep (omr_ff.avio_closep)
#define avio_open (omr_ff.avio_open)
#define av_buffersink_get_frame (omr_ff.av_buffersink_get_frame)
#define av_buffersink_get_hw_frames_ctx (omr_ff.av_buffersink_get_hw_frames_ctx)
#define av_buffersrc_add_frame_flags (omr_ff.av_buffersrc_add_frame_flags)
#define av_buffersrc_parameters_alloc (omr_ff.av_buffersrc_parameters_alloc)
#define av_buffersrc_parameters_set (omr_ff.av_buffersrc_parameters_set)
#define avfilter_get_by_name (omr_ff.avfilter_get_by_name)
#define avfilter_graph_alloc (omr_ff.avfilter_graph_alloc)
#define avfilter_graph_alloc_filter (omr_ff.avfilter_graph_alloc_filter)
#define avfilter_graph_config (omr_ff.avfilter_graph_config)
#define avfilter_graph_create_filter (omr_ff.avfilter_graph_create_filter)
#define avfilter_graph_free (omr_ff.avfilter_graph_free)
#define avfilter_init_dict (omr_ff.avfilter_init_dict)
#define avfilter_link (omr_ff.avfilter_link)
#endif

#endif
