#!/usr/bin/env python3
"""Turns measure.sh's results into the report's tables: the median over the runs of each
number, with the range in brackets when the runs differ.

summarize.py <results dir>
"""
import glob
import json
import os
import statistics
import sys

root = sys.argv[1]


def runs(pattern):
    return [json.load(open(p)) for p in sorted(glob.glob(os.path.join(root, pattern)))
            if not p.endswith(('.frames.json', '.sample.json', '.stats.json'))]


def cell(values, digits=1, suffix=''):
    values = [v for v in values if v is not None]
    if not values:
        return 'n/a'
    median = statistics.median(values)
    text = f'{median:.{digits}f}{suffix}'
    if len(values) > 1 and max(values) != min(values):
        text += f' ({min(values):.{digits}f}–{max(values):.{digits}f})'
    return text


print('### Preview\n')
print('| | Qt 1080p | iced 1080p | Qt 4K | iced 4K |')
print('|---|---|---|---|---|')
data = {(app, b): runs(f'{app}-preview-{b}-*.json') for app in ('qt', 'iced') for b in ('b1080', 'b2160')}
order = [('qt', 'b1080'), ('iced', 'b1080'), ('qt', 'b2160'), ('iced', 'b2160')]


def row(name, get, digits=1, suffix=''):
    print(f'| {name} | ' + ' | '.join(cell([get(r) for r in data[k]], digits, suffix) for k in order) + ' |')


print('| runs | ' + ' | '.join(str(len(data[k])) for k in order) + ' |')
row('presented fps (commits)', lambda r: r['frames']['fps'], 1)
row('frame time median (ms)', lambda r: r['frames']['frame_ms_median'], 2)
row('frame time p95 (ms)', lambda r: r['frames']['frame_ms_p95'], 2)
row('frame time max (ms)', lambda r: r['frames']['frame_ms_max'], 1)
row('missed vsyncs (gap > 25 ms)', lambda r: r['frames']['missed_vsyncs'], 0)


def dropped(r):
    app = r['app']
    return app.get('screen_frames_dropped', app.get('dropped'))


row('dropped frames in 10 s (app\'s count)', dropped, 0)
row('CPU % (process)', lambda r: r['sample']['cpu_percent'], 0)
row('CPU % (ffmpeg children)', lambda r: r['sample']['children_cpu_percent'], 0)
row('RSS peak (MB, process)', lambda r: r['sample']['rss_peak_mb'], 0)
row('RSS peak (MB, with children)', lambda r: r['sample']['tree_rss_peak_mb'], 0)
row('GPU buffers resident peak (MB, fdinfo)', lambda r: r['sample'].get('gpu_resident_peak_mb'), 0)
row('GPU render engine busy %', lambda r: r['sample']['gpu_busy_percent'].get('drm-engine-render'), 0)
row('GPU video engine busy %', lambda r: r['sample']['gpu_busy_percent'].get('drm-engine-video'), 0)
row('load average at end', lambda r: float(r['sample']['loadavg'][0]), 1)

print('\n### Export (1920x1080, libx264 "social")\n')
print('| | Qt 1080p | iced 1080p | Qt 4K source | iced 4K source |')
print('|---|---|---|---|---|')
exports = {(app, b): runs(f'{app}-export-{b}-*.json') for app in ('qt', 'iced') for b in ('b1080', 'b2160')}
print('| frames/s (600 frames, wall clock incl. startup) | ' + ' | '.join(cell([r['fps'] for r in exports[k]], 1) for k in order) + ' |')
print('| seconds | ' + ' | '.join(cell([r['seconds'] for r in exports[k]], 2) for k in order) + ' |')
print('| load average at end | ' + ' | '.join(cell([float(r['load'][0]) for r in exports[k]], 1) for k in order) + ' |')


def timing(app, bundle):
    """ExportJob::timing_report's lines, which both exporters print with --timing."""
    import re
    out = []
    for path in sorted(glob.glob(os.path.join(root, f'{app}-export-{bundle}-*.log'))):
        text = open(path, errors='replace').read()
        stages = dict((k.strip(), float(v)) for k, v in re.findall(r'^\s+([a-z ]+):\s+([\d.]+) ms/frame', text, re.M))
        total = re.search(r'total:\s+([\d.]+) s \(([\d.]+) frames/s\)', text)
        if total:
            stages['fps'] = float(total.group(2))
        out.append(stages)
    return out


report = {k: timing(*k) for k in order}
print('| frames/s (the job\'s own timer) | ' + ' | '.join(cell([t.get('fps') for t in report[k]], 1) for k in order) + ' |')
for stage in ('decode wait', 'upload', 'render', 'readback', 'encode write'):
    print(f'| {stage} (ms/frame) | ' + ' | '.join(cell([t.get(stage) for t in report[k]], 2) for k in order) + ' |')
