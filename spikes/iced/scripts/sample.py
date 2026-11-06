#!/usr/bin/env python3
"""Samples a process and its descendants: CPU %, RSS and GPU busy time.

sample.py <pid> <seconds> <out.json>

CPU is utime + stime from /proc/<pid>/stat over the window (100 % = one core), for the process
itself and for its descendants (the spike's ffmpeg decoders), each summed over the processes
alive at the end. RSS is the peak VmRSS seen. GPU is the drm-engine-* time the process's DRM file
descriptors report in fdinfo (i915 and xe both do), over the window, as a share of wall time.
"""
import json
import os
import sys
import time

TICK = os.sysconf('SC_CLK_TCK')


def children(pid):
    out = []
    try:
        for task in os.listdir(f'/proc/{pid}/task'):
            with open(f'/proc/{pid}/task/{task}/children') as f:
                out += [int(c) for c in f.read().split()]
    except OSError:
        pass
    return out


def tree(pid):
    result, stack = [], children(pid)
    while stack:
        child = stack.pop()
        result.append(child)
        stack += children(child)
    return result


def cpu(pid):
    try:
        with open(f'/proc/{pid}/stat') as f:
            fields = f.read().rsplit(')', 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / TICK
    except OSError:
        return None


def rss(pid):
    try:
        with open(f'/proc/{pid}/status') as f:
            for line in f:
                if line.startswith('VmRSS:'):
                    return int(line.split()[1]) / 1024
    except OSError:
        pass
    return 0.0


def gpu_ns(pid, prefix='drm-engine-', unit=' ns'):
    """Sum of drm-engine-* (or another fdinfo key family) over this process's DRM clients,
    each client counted once."""
    clients = {}
    try:
        for fd in os.listdir(f'/proc/{pid}/fdinfo'):
            try:
                with open(f'/proc/{pid}/fdinfo/{fd}') as f:
                    text = f.read()
            except OSError:
                continue
            if 'drm-client-id' not in text:
                continue
            client, busy = None, {}
            for line in text.splitlines():
                key, _, value = line.partition(':')
                value = value.strip()
                if key == 'drm-client-id':
                    client = value
                elif key.startswith(prefix) and not key.startswith('drm-engine-capacity') and value.endswith(unit):
                    amount = int(value.split()[0])
                    busy[key] = amount * {'KiB': 1024, 'MiB': 1 << 20}.get(value.split()[-1], 1)
            if client is not None:
                clients[client] = busy
    except OSError:
        pass
    total = {}
    for busy in clients.values():
        for key, value in busy.items():
            total[key] = total.get(key, 0) + value
    return total


def main():
    pid, seconds, out = int(sys.argv[1]), float(sys.argv[2]), sys.argv[3]
    start = time.monotonic()
    cpu0 = {p: cpu(p) for p in [pid] + tree(pid)}
    gpu0 = gpu_ns(pid)
    peak_rss, peak_tree_rss, samples, peak_gpu_memory = 0.0, 0.0, [], 0.0
    while time.monotonic() - start < seconds:
        procs = [pid] + tree(pid)
        for p in procs:
            if p not in cpu0:
                cpu0[p] = cpu(p) or 0.0
        own = rss(pid)
        total = own + sum(rss(p) for p in procs[1:])
        peak_rss, peak_tree_rss = max(peak_rss, own), max(peak_tree_rss, total)
        samples.append(round(own, 1))
        # GPU buffers in system memory (i915/xe report them per client): textures and staging
        # buffers that VmRSS does not show.
        resident = gpu_ns(pid, 'drm-resident-', 'iB')
        peak_gpu_memory = max(peak_gpu_memory, sum(resident.values()) / (1 << 20))
        time.sleep(0.25)
    wall = time.monotonic() - start
    procs = [pid] + tree(pid)
    cpu1 = {p: cpu(p) for p in procs}
    gpu1 = gpu_ns(pid)
    own_cpu = (cpu1[pid] - cpu0[pid]) / wall * 100 if cpu1.get(pid) is not None else None
    child_cpu = sum((cpu1[p] or 0) - cpu0.get(p, 0) for p in procs[1:] if cpu1.get(p) is not None) / wall * 100
    gpu = {key: round((gpu1.get(key, 0) - gpu0.get(key, 0)) / (wall * 1e9) * 100, 1) for key in gpu1}
    result = {
        'wall_s': round(wall, 2),
        'cpu_percent': round(own_cpu, 1) if own_cpu is not None else None,
        'children_cpu_percent': round(child_cpu, 1),
        'children': len(procs) - 1,
        'rss_peak_mb': round(peak_rss, 1),
        'tree_rss_peak_mb': round(peak_tree_rss, 1),
        'gpu_busy_percent': gpu,
        'gpu_resident_peak_mb': round(peak_gpu_memory, 1),
        'loadavg': open('/proc/loadavg').read().split()[:3],
    }
    with open(out, 'w') as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result))


main()
