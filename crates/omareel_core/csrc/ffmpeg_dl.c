/* See ffmpeg_dl.h. */
#define OMR_FF_NO_MACROS
#include "ffmpeg_dl.h"

#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>

struct omr_ff_table omr_ff;

#define OMR_FF_STRING_(x) #x
#define OMR_FF_STRING(x) OMR_FF_STRING_(x)

enum { OMR_FF_LIB_avutil, OMR_FF_LIB_swscale, OMR_FF_LIB_avcodec, OMR_FF_LIB_avformat, OMR_FF_LIB_avfilter, OMR_FF_LIBRARIES };
static struct {
    const char *name;
    void *handle;
} libraries[OMR_FF_LIBRARIES] = {
    {"libavutil.so." OMR_FF_STRING(LIBAVUTIL_VERSION_MAJOR), NULL},
    {"libswscale.so." OMR_FF_STRING(LIBSWSCALE_VERSION_MAJOR), NULL},
    {"libavcodec.so." OMR_FF_STRING(LIBAVCODEC_VERSION_MAJOR), NULL},
    {"libavformat.so." OMR_FF_STRING(LIBAVFORMAT_VERSION_MAJOR), NULL},
    {"libavfilter.so." OMR_FF_STRING(LIBAVFILTER_VERSION_MAJOR), NULL},
};

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int status = -1;
static char error[256];

static void load(void)
{
    for (int i = 0; i < OMR_FF_LIBRARIES; ++i) {
        if (!(libraries[i].handle = dlopen(libraries[i].name, RTLD_NOW | RTLD_LOCAL))) {
            snprintf(error, sizeof error, "could not load %s: %s", libraries[i].name, dlerror());
            return;
        }
    }
#define OMR_FF_RESOLVE(lib, function) \
    if (!(*(void **)&omr_ff.function = dlsym(libraries[OMR_FF_LIB_##lib].handle, #function))) { \
        snprintf(error, sizeof error, "%s has no %s", libraries[OMR_FF_LIB_##lib].name, #function); \
        return; \
    }
    OMR_FF_FUNCTIONS(OMR_FF_RESOLVE)
#undef OMR_FF_RESOLVE
    status = 0;
}

int omr_ff_load(void)
{
    pthread_once(&once, load);
    return status;
}

const char *omr_ff_error(void)
{
    omr_ff_load();
    return error;
}
