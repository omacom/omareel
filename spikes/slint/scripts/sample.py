#!/usr/bin/env python3
"""CPU, RSS and GPU of a process over a window.

sample.py <pid> <seconds> <out.json>

cpu_percent: the process's utime + stime over the window (100 = one core); children_cpu_percent:
the same for its descendants alive at the end (the ffmpeg decoders). rss_peak_mb: the largest
VmRSS seen (sampled every 250 ms), tree_rss_peak_mb with the descendants. gpu_busy_percent: the
drm-engine-* busy time the process's DRM clients report in /proc/<pid>/fdinfo (i915 and xe do),
per engine, as a share of the window.
"""
import json
import os
import sys
import time

TICK = os.sysconf('SC_CLK_TCK')


def descendants(pid):
    found, stack = [], [pid]
    while stack:
        p = stack.pop()
        try:
            for task in os.listdir(f'/proc/{p}/task'):
                with open(f'/proc/{p}/task/{task}/children') as f:
                    kids = [int(c) for c in f.read().split()]
                found += kids
                stack += kids
        except OSError:
            pass
    return found


def cpu_seconds(pid):
    try:
        with open(f'/proc/{pid}/stat') as f:
            fields = f.read().rsplit(')', 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / TICK
    except OSError:
        return None


def rss_mb(pid):
    try:
        with open(f'/proc/{pid}/status') as f:
            for line in f:
                if line.startswith('VmRSS:'):
                    return int(line.split()[1]) / 1024
    except OSError:
        pass
    return 0.0


def gpu_ns(pid):
    clients = {}
    try:
        fds = os.listdir(f'/proc/{pid}/fdinfo')
    except OSError:
        return {}
    for fd in fds:
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
            elif key.startswith('drm-engine-') and not key.startswith('drm-engine-capacity') and value.endswith(' ns'):
                busy[key[len('drm-engine-'):]] = int(value.split()[0])
        if client is not None:
            clients[client] = busy
    total = {}
    for busy in clients.values():
        for key, value in busy.items():
            total[key] = total.get(key, 0) + value
    return total


def main():
    pid, seconds, out = int(sys.argv[1]), float(sys.argv[2]), sys.argv[3]
    start = time.monotonic()
    cpu0 = {p: cpu_seconds(p) or 0.0 for p in [pid] + descendants(pid)}
    gpu0 = gpu_ns(pid)
    peak, tree_peak = 0.0, 0.0
    while time.monotonic() - start < seconds:
        procs = [pid] + descendants(pid)
        for p in procs:
            cpu0.setdefault(p, cpu_seconds(p) or 0.0)
        own = rss_mb(pid)
        peak = max(peak, own)
        tree_peak = max(tree_peak, own + sum(rss_mb(p) for p in procs[1:]))
        time.sleep(0.25)
    wall = time.monotonic() - start
    procs = [pid] + descendants(pid)
    cpu1 = {p: cpu_seconds(p) for p in procs}
    gpu1 = gpu_ns(pid)
    own = (cpu1[pid] - cpu0[pid]) / wall * 100 if cpu1.get(pid) is not None else None
    kids = sum((cpu1[p] or 0) - cpu0.get(p, 0) for p in procs[1:] if cpu1.get(p) is not None) / wall * 100
    result = {
        'wall_s': round(wall, 2),
        'cpu_percent': None if own is None else round(own, 1),
        'children_cpu_percent': round(kids, 1),
        'rss_peak_mb': round(peak, 1),
        'tree_rss_peak_mb': round(tree_peak, 1),
        'gpu_busy_percent': {k: round((gpu1.get(k, 0) - gpu0.get(k, 0)) / (wall * 1e9) * 100, 1) for k in gpu1},
        'loadavg': open('/proc/loadavg').read().split()[:3],
    }
    with open(out, 'w') as f:
        json.dump(result, f, indent=1)
    print(json.dumps(result))


main()
