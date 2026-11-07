#!/usr/bin/env python3
"""Frame times from a WAYLAND_DEBUG=client log: the intervals between the busiest surface's
commits, which is when each frame reaches the compositor, for Qt Quick (EGL) and Slint (wgpu on
Vulkan WSI) alike.

frames.py <log> [seconds] [skip]

Takes the longest run of commits without a gap over 250 ms (the playback), skips `skip` seconds
of it (default 0.5) and measures the next `seconds` (default 10). When the log holds the Rune
spike's "spike: measured playback starts" line, only commits after it count: the spike's paused
window before playback commits a frame every 30 to 50 ms, which would otherwise join the run.
Only commits with a buffer attached since the surface's last commit count: NVIDIA's Vulkan WSI
(wp_fifo_v1) follows each frame's commit with a second, empty one.
"""
import json
import re
import sys

# libwayland prints "[1234567.890]" (ms) or, since 1.23, "[HH:MM:SS.mmm]".
pattern = re.compile(r'^\[\s*(?:(\d+):(\d+):(\d+\.\d+)|(\d+\.\d+))\].*-> (wl_surface[#@]\d+)\.commit\(\)')
attach = re.compile(r'-> (wl_surface[#@]\d+)\.attach\(wl_buffer')
attached = set()
commits = {}
lines = open(sys.argv[1], errors='replace').read().splitlines()
marker = next((i for i, line in enumerate(lines) if 'spike: measured playback starts' in line), None)
if marker is not None:
    lines = lines[marker:]
for line in lines:
    a = attach.search(line)
    if a:
        attached.add(a.group(1))
        continue
    m = pattern.match(line)
    if not m or m.group(5) not in attached:
        continue
    attached.discard(m.group(5))
    if m.group(4):
        t = float(m.group(4)) / 1000.0
    else:
        t = int(m.group(1)) * 3600 + int(m.group(2)) * 60 + float(m.group(3))
    commits.setdefault(m.group(5), []).append(t)
seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 10.0
skip = float(sys.argv[3]) if len(sys.argv) > 3 else 0.5
times = max(commits.values(), key=len) if commits else []
runs, start = [], 0
for i in range(1, len(times) + 1):
    if i == len(times) or times[i] - times[i - 1] > 0.25:
        runs.append(times[start:i])
        start = i
run = max(runs, key=lambda r: r[-1] - r[0]) if runs else []
run = [t for t in run if run[0] + skip <= t <= run[0] + skip + seconds]
gaps = sorted((b - a) * 1000 for a, b in zip(run, run[1:]))
span = run[-1] - run[0] if len(run) > 1 else 0


def pick(p):
    return round(gaps[min(len(gaps) - 1, round((len(gaps) - 1) * p))], 2) if gaps else None


print(json.dumps({
    'window_s': round(span, 2),
    'commits': len(run),
    'fps': round((len(run) - 1) / span, 2) if span else None,
    'frame_ms_median': pick(0.5),
    'frame_ms_p95': pick(0.95),
    'frame_ms_max': round(gaps[-1], 2) if gaps else None,
    'missed_vsyncs': sum(1 for g in gaps if g > 25.0),
}))
