import ctypes as C, resource, time
vk = C.CDLL('libvulkan.so.1')
P = C.c_void_p
class InstanceCI(C.Structure): _fields_ = [('sType', C.c_uint32), ('pNext', P), ('flags', C.c_uint32), ('app', P), ('nl', C.c_uint32), ('l', P), ('ne', C.c_uint32), ('e', P)]
class QueueCI(C.Structure): _fields_ = [('sType', C.c_uint32), ('pNext', P), ('flags', C.c_uint32), ('family', C.c_uint32), ('count', C.c_uint32), ('prio', C.POINTER(C.c_float))]
class DeviceCI(C.Structure): _fields_ = [('sType', C.c_uint32), ('pNext', P), ('flags', C.c_uint32), ('nq', C.c_uint32), ('q', C.POINTER(QueueCI)), ('nl', C.c_uint32), ('l', P), ('ne', C.c_uint32), ('e', P), ('feat', P)]
last = [time.monotonic(), resource.getrusage(resource.RUSAGE_SELF)]
def mark(s):
    r, w = resource.getrusage(resource.RUSAGE_SELF), time.monotonic()
    print(f'{s:28} wall {(w-last[0])*1e3:7.1f} ms  user {(r.ru_utime-last[1].ru_utime)*1e3:6.1f}  sys {(r.ru_stime-last[1].ru_stime)*1e3:6.1f}')
    last[:] = [w, r]
inst = P(); assert vk.vkCreateInstance(C.byref(InstanceCI(1)), None, C.byref(inst)) == 0; mark('vkCreateInstance')
n = C.c_uint32(8); pd = (P * 8)(); vk.vkEnumeratePhysicalDevices(inst, C.byref(n), pd); mark(f'vkEnumeratePhysicalDevices ({n.value})')
prio = C.c_float(1.0); q = QueueCI(2, None, 0, 0, 1, C.pointer(prio))
dev = P(); assert vk.vkCreateDevice(P(pd[0]), C.byref(DeviceCI(3, None, 0, 1, C.pointer(q))), None, C.byref(dev)) == 0; mark('vkCreateDevice')
vk.vkDestroyDevice(dev, None); mark('vkDestroyDevice')
vk.vkDestroyInstance(inst, None); mark('vkDestroyInstance')
