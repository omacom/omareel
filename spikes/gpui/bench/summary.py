#!/usr/bin/env python3
"""Summarises one nested-body.py run: DIR NAME [SECONDS] -> JSON on stdout.

The measured window is SECONDS (10) from the last "preview state=1" the client printed: the
GPUI spike prints it once its decoders have the first frame and its clock starts; the Qt
editor (OMAREEL_PREVIEW_STATS=1) prints it whenever its player starts playing, which it does
once 1.5 s after loading QML and again after its loading timer re-seeks.
Frame intervals are the wl_surface.commit times of the client's busiest surface from its
WAYLAND_DEBUG=client log (libwayland stamps each line with the UTC time of day). CPU, RSS and
GPU come from the samples nearest the window's ends.
"""
import json
import re
import sys
from collections import defaultdict

directory, name = sys.argv[1], sys.argv[2]
seconds = float(sys.argv[3]) if len(sys.argv) > 3 else 10.0
run = json.load(open(f'{directory}/{name}.samples.json'))
samples, marker = run['samples'], run['marker']

pattern = re.compile(r'^\[(\d+):(\d+):(\d+\.\d+)\].*-> wl_surface#(\d+)\.commit\(\)')
stamp = re.compile(r'^\[(\d+):(\d+):(\d+\.\d+)\]')
commits = defaultdict(list)
day = int(marker // 86400) * 86400
last, start = None, None
for line in open(f'{directory}/{name}.stderr', errors='replace'):
    m = stamp.match(line)
    if m:
        last = day + int(m.group(1)) * 3600 + int(m.group(2)) * 60 + float(m.group(3))
        c = pattern.match(line)
        if c:
            commits[c.group(4)].append(last)
    elif line.startswith('preview state=1') and last is not None:
        start = last
surface, times = max(commits.items(), key=lambda kv: len(kv[1]))
end = start + seconds
window = [t for t in times if start <= t <= end]
intervals = sorted((b - a) * 1000 for a, b in zip(window, window[1:]))
pick = lambda q: round(intervals[min(len(intervals) - 1, round((len(intervals) - 1) * q))], 2)

def at(epoch):
    return min(samples, key=lambda s: abs(s['epoch'] - epoch))

a, b = at(start), at(end)
span = b['epoch'] - a['epoch']
inside = [s for s in samples if a['epoch'] <= s['epoch'] <= b['epoch']]
stdout = open(f'{directory}/{name}.stdout').read().strip().splitlines()
app = json.loads(stdout[-1]) if stdout and stdout[-1].startswith('{') else None
stats = [line for line in open(f'{directory}/{name}.stderr', errors='replace') if line.startswith('preview fps=')]
print(json.dumps({
    'name': name,
    'window_after_first_marker_s': round(start - marker, 2),
    'commits': len(window),
    'fps': round(len(window) / seconds, 1),
    'interval_ms': {'median': pick(.5), 'p95': pick(.95), 'max': round(intervals[-1], 2)},
    'intervals_over_20ms': sum(1 for i in intervals if i > 20),
    'cpu_main_pct': round(100 * (b['cpu_main'] - a['cpu_main']) / span, 1),
    'cpu_tree_pct': round(100 * (b['cpu_tree'] - a['cpu_tree']) / span, 1),
    'rss_main_mb': round(max(s['rss_main'] for s in inside)),
    'rss_tree_mb': round(max(s['rss_tree'] for s in inside)),
    'gpu_pct': {k: round(100 * (b['gpu'].get(k, 0) - a['gpu'].get(k, 0)) / (span * 1e9), 1)
                for k in b['gpu'] if b['gpu'].get(k, 0) != a['gpu'].get(k, 0)},
    'app': app,
    'qt_preview_stats_drops': sum(int(l.rsplit('drops=', 1)[1]) for l in stats) if stats else None,
}))
