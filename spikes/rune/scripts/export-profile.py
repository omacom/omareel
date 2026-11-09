#!/usr/bin/env python3
"""Where an export's CPU goes, from a `perf record --call-graph fp --user-callchains` of
`<app> export` and the ffmpeg processes it starts (perf follows them):

    export-profile.py <perf.data> [--thread REGEX] [--top N]

Prints CPU seconds per process (the app, then each ffmpeg child, named by what it does: the
decoder links libavcodec's h264 decoder and no x264, the encoder runs libx264) and per thread,
then the inclusive cost of the busiest functions on each thread whose name matches --thread
(default: the app's main thread).
"""
import argparse
import collections
import os
import re
import subprocess

ap = argparse.ArgumentParser()
ap.add_argument('data')
ap.add_argument('--thread', default=None, help='regex on "comm tid" of the threads to break down')
ap.add_argument('--top', type=int, default=30)
ap.add_argument('--freq', type=float, default=2999)
args = ap.parse_args()
perf = os.path.expanduser('~/.cache/omareel-spikes/tools/perf')
out = subprocess.run([perf, 'script', '-i', args.data, '-F', 'comm,pid,tid,time,ip,sym,dso', '--no-inline'],
                     capture_output=True, text=True, errors='replace').stdout
header = re.compile(r'^\s*(\S.*?)\s+(\d+)/(\d+)\s+(\d+\.\d+):')
frame_re = re.compile(r'^\s+[0-9a-f]+\s+(.*?)\s+\((.*)\)$')
samples = []
for block in out.split('\n\n'):
    lines = [l for l in block.split('\n') if l.strip()]
    if not lines:
        continue
    h = header.match(lines[0])
    if not h:
        continue
    frames = []
    for l in lines[1:]:
        f = frame_re.match(l)
        if f:
            sym = re.sub(r'\+0x[0-9a-f]+$', '', f[1])
            sym = re.sub(r'::h[0-9a-f]{16}$', '', sym)
            if sym == '[unknown]':
                sym = '[' + os.path.basename(f[2]) + ']'
            frames.append(sym)
    samples.append((h[1], int(h[2]), int(h[3]), frames))
period = 1.0 / args.freq
# Name the processes: the first pid is the app; an ffmpeg with libx264 frames is the encoder.
pids = list(dict.fromkeys(s[1] for s in samples))
role = {}
for pid in pids:
    rows = [(c, fr) for c, p, t, fr in samples if p == pid]
    stacks = ' '.join(' '.join(fr) for c, fr in rows)
    comms = {c for c, fr in rows}
    if 'x264' in stacks:
        role[pid] = 'ffmpeg encoder'
    elif any(c.startswith(('dec0', 'mux0', 'demux', 'vf#', 'av:')) for c in comms) or 'ff_h264' in stacks or 'libavcodec' in stacks:
        role[pid] = 'ffmpeg decoder'
    elif pid == pids[0] or len(rows) > 0.05 * len(samples):
        role[pid] = 'app'
    else:
        role[pid] = 'other ' + sorted(comms)[0]
per_process = collections.Counter(s[1] for s in samples)
print('CPU seconds per process:')
for pid, n in per_process.most_common():
    print(f'  {n * period:7.2f}  {role[pid]} ({pid})')
per_thread = collections.Counter((s[1], s[0], s[2]) for s in samples)
print('\nCPU seconds per thread (top 25):')
for (pid, comm, tid), n in per_thread.most_common(25):
    print(f'  {n * period:7.2f}  {role[pid]:15} {comm} {tid}')
# Self time by library and inclusive functions, per process.
for pid in pids:
    chosen = [fr for c, p, t, fr in samples if p == pid and fr]
    leaf = collections.Counter(fr[0] for fr in chosen)
    print(f'\n{role[pid]} ({pid}): top self functions')
    for sym, n in leaf.most_common(12):
        print(f'  {n * period:7.2f}  {sym[:150]}')
main_tid = pids[0]
pattern = re.compile(args.thread) if args.thread else None
chosen = [fr for c, p, t, fr in samples if (pattern.search(f'{c} {t}') if pattern else (p == main_tid and t == main_tid))]
inc = collections.Counter()
for fr in chosen:
    inc.update(set(fr))
print(f'\ninclusive CPU seconds, {"threads matching " + args.thread if pattern else "the app main thread"} ({len(chosen) * period:.2f} s):')
for sym, n in inc.most_common(args.top):
    print(f'  {n * period:7.2f}  {sym[:160]}')
