#!/usr/bin/env python3
"""Frame intervals from a client's WAYLAND_DEBUG=client log: the wl_surface.commit times of
its busiest surface between two wall-clock times (libwayland stamps each line with the UTC
time of day, [HH:MM:SS.micro]).

  frames.py LOG START_EPOCH END_EPOCH   ->  JSON on stdout
"""
import json
import re
import sys
from collections import defaultdict

log, start, end = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
wrap = 86400.0
pattern = re.compile(r'^\[(\d+):(\d+):(\d+\.\d+)\].*-> wl_surface#(\d+)\.commit\(\)')
commits = defaultdict(list)
for line in open(log, errors='replace'):
    m = pattern.match(line)
    if m:
        commits[m.group(4)].append(int(m.group(1)) * 3600 + int(m.group(2)) * 60 + float(m.group(3)))

def stamp(epoch):
    return epoch % 86400.0

lo, hi = stamp(start), stamp(end)
def inside(t):
    return (lo <= t <= hi) if lo <= hi else (t >= lo or t <= hi)

best = max(commits.items(), key=lambda kv: sum(1 for t in kv[1] if inside(t)), default=(None, []))
times = [t for t in best[1] if inside(t)]
intervals = sorted(((b - a) % wrap) * 1000 for a, b in zip(times, times[1:]))
pick = lambda q: intervals[min(len(intervals) - 1, round((len(intervals) - 1) * q))] if intervals else 0
seconds = (hi - lo) % wrap
print(json.dumps({
    'surface': best[0],
    'commits': len(times),
    'fps': len(times) / seconds if seconds else 0,
    'interval_ms': {'median': round(pick(.5), 2), 'p95': round(pick(.95), 2), 'max': round(intervals[-1], 2) if intervals else 0},
    'over_25ms': sum(1 for i in intervals if i > 25),
}))
