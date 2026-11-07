#!/usr/bin/env python3
"""Tables of measure.sh's results: the median over runs of each number, and its range.

summary.py <results dir>"""
import glob
import json
import os
import statistics
import sys

results = sys.argv[1]


def runs(pattern):
    return [json.load(open(p)) for p in sorted(glob.glob(os.path.join(results, pattern)))]


def cell(values, digits=1):
    values = [v for v in values if v is not None]
    if not values:
        return '–'
    m = statistics.median(values)
    text = f'{m:.{digits}f}'
    if len(values) > 1 and max(values) - min(values) > 10 ** -digits:
        text += f' ({min(values):.{digits}f}–{max(values):.{digits}f})'
    return text


def gpu(sample, engine='render', key='gpu_busy_percent'):
    busy = sample.get(key) or {}
    return busy.get(engine)


print('## Preview\n')
print('| | frames/s | frame ms median | p95 | max | missed vsyncs | dropped (app) | CPU % | decoder CPU % | RSS MB | GPU render % | GPU video % | whole GPU % (nvidia-smi) | UI-thread frame ms | load |')
print('|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|')
for bundle in ('b1080', 'b2160'):
    for app, pattern in (('Rune (VA-API, NV12 surface)', f'rune-vaapi-surface-preview-{bundle}-[0-9].json'),
                         ('Qt', f'qt-preview-{bundle}-[0-9].json'),
                         ('Rune (software, NV12 surface)', f'rune-software-surface-preview-{bundle}-[0-9].json'),
                         ('Rune (software, RGBA pixels)', f'rune-software-pixels-preview-{bundle}-[0-9].json'),
                         ('Rune (software, NV12 surface, decoded at 1080)', f'rune-software-surface-1080-preview-{bundle}-[0-9].json')):
        rs = runs(pattern)
        if not rs:
            continue
        f = [r['frames'] for r in rs]
        s = [r['sample'] for r in rs]
        print(f"| {app} {bundle[1:]}p ({len(rs)}) | {cell([x['fps'] for x in f])} | {cell([x['frame_ms_median'] for x in f], 2)} | "
              f"{cell([x['frame_ms_p95'] for x in f], 2)} | {cell([x['frame_ms_max'] for x in f])} | {cell([x['missed_vsyncs'] for x in f], 0)} | "
              f"{cell([r['app'].get('dropped') for r in rs], 0)} | {cell([x['cpu_percent'] for x in s])} | {cell([x['children_cpu_percent'] for x in s])} | "
              f"{cell([x['rss_peak_mb'] for x in s], 0)} | {cell([gpu(x) for x in s])} | {cell([gpu(x, 'video') for x in s])} | {cell([gpu(x, key='gpu_whole_percent') for x in s])} | "
              f"{cell([r['app'].get('frame_cpu_ms_median') for r in rs], 2)} | {', '.join(x['loadavg'][0] for x in s)} |")

print('\n## Export (600 frames, 1920x1080, quality social)\n')
print('| | seconds | frames/s | load at end |')
print('|---|---|---|---|')
for bundle in ('b1080', 'b2160'):
    for app in ('rune', 'qt'):
        rs = runs(f'{app}-export-{bundle}-[0-9].json')
        if rs:
            print(f"| {'Rune' if app == 'rune' else 'Qt'} {bundle[1:]}p ({len(rs)}) | {cell([r['seconds'] for r in rs], 2)} | "
                  f"{cell([r['fps'] for r in rs])} | {', '.join(r['load'][0] for r in rs)} |")
