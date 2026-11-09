import ctypes as C, resource, time
egl = C.CDLL('libEGL.so.1'); gl = None
P = C.c_void_p
egl.eglGetProcAddress.restype = P
egl.eglGetPlatformDisplay.restype = P; egl.eglGetPlatformDisplay.argtypes = [C.c_uint, P, P]
egl.eglCreateContext.restype = P; egl.eglCreateContext.argtypes = [P, P, P, P]
egl.eglInitialize.argtypes = [P, P, P]; egl.eglMakeCurrent.argtypes = [P, P, P, P]; egl.eglBindAPI.argtypes = [C.c_uint]
egl.eglChooseConfig.argtypes = [P, P, P, C.c_int, P]
last = [time.monotonic(), resource.getrusage(resource.RUSAGE_SELF)]
def mark(s):
    r, w = resource.getrusage(resource.RUSAGE_SELF), time.monotonic()
    print(f'{s:26} wall {(w-last[0])*1e3:7.1f} ms  user {(r.ru_utime-last[1].ru_utime)*1e3:6.1f}  sys {(r.ru_stime-last[1].ru_stime)*1e3:6.1f}')
    last[:] = [w, r]
# EGL_PLATFORM_DEVICE_EXT with the first device (no window system).
qd = C.CFUNCTYPE(C.c_uint, C.c_int, P, C.POINTER(C.c_int))(egl.eglGetProcAddress(b'eglQueryDevicesEXT'))
devs = (P * 4)(); n = C.c_int(); qd(4, devs, C.byref(n)); mark(f'eglQueryDevicesEXT ({n.value})')
d = egl.eglGetPlatformDisplay(0x313F, devs[0], None); mark('eglGetPlatformDisplay')
assert egl.eglInitialize(d, None, None); mark('eglInitialize')
egl.eglBindAPI(0x30A2)  # EGL_OPENGL_ES_API
attrs = (C.c_int * 5)(0x3040, 0x40, 0x3038, 0, 0)  # RENDERABLE_TYPE=ES3
cfg = P(); nc = C.c_int(); egl.eglChooseConfig(d, attrs, C.byref(cfg), 1, C.byref(nc)); mark('eglChooseConfig')
ca = (C.c_int * 3)(0x3098, 3, 0x3038)
ctx = egl.eglCreateContext(d, cfg, None, ca); assert ctx; mark('eglCreateContext (ES 3)')
assert egl.eglMakeCurrent(d, None, None, ctx); mark('eglMakeCurrent (surfaceless)')
glc = C.CFUNCTYPE(None)(egl.eglGetProcAddress(b'glFinish')); glc(); mark('glFinish')
ctx2 = egl.eglCreateContext(d, cfg, None, ca); assert ctx2; mark("second eglCreateContext")
assert egl.eglMakeCurrent(d, None, None, ctx2); mark("second eglMakeCurrent")
import threading
def other():
    c3 = egl.eglCreateContext(d, cfg, None, ca); mark("thread: eglCreateContext"); egl.eglMakeCurrent(d, None, None, c3); mark("thread: eglMakeCurrent")
t = threading.Thread(target=other); t.start(); t.join()
