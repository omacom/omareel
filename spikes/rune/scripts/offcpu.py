#!/usr/bin/env python3
"""What a thread waited on: reads a startup-profile.py run recorded with
PERF_EVENT="-F 9999 --switch-events" CALLGRAPH=fp, pairs each switch out of the
thread (with the stack it blocked in) with its next switch in, and lists the waits longer than
--min ms up to --until s, then the total by stack.

    offcpu.py <run dir> [--tid main|N] [--until S] [--min MS]
"""
import argparse
import collections
import json
import os
import re
import subprocess

ap = argparse.ArgumentParser()
ap.add_argument('run')
ap.add_argument('--tid', default='main')
ap.add_argument('--until', type=float, default=1.0)
ap.add_argument('--min', type=float, default=2.0)
args = ap.parse_args()
perf = os.path.expanduser('~/.cache/omareel-spikes/tools/perf')
start = json.load(open(os.path.join(args.run, 'start.json')))['mono']
out = subprocess.run([perf, 'script', '-i', os.path.join(args.run, 'perf.data'), '--show-switch-events', '-F',
                      'comm,pid,tid,time,ip,sym,dso', '--no-inline'], capture_output=True, text=True, errors='replace').stdout
header = re.compile(r'^\s*(\S.*?)\s+(\d+)/(\d+)\s+(\d+\.\d+):\s*(.*)$')
events = []  # (t, tid, kind, frames)
current = None
for line in out.split('\n'):
    if not line.strip():
        continue
    h = header.match(line)
    if h and not line.startswith('\t'):
        current = None
        pid, tid, t, rest = int(h[2]), int(h[3]), float(h[4]) - start, h[5]
        if rest.startswith('PERF_RECORD_SWITCH'):
            events.append([t, pid, tid, 'out' if ' OUT' in rest else 'in', 'preempt' in rest, []])
        elif not rest.startswith('PERF_RECORD'):
            current = [t, pid, tid, 'stack', False, []]
            events.append(current)
        continue
    if current is not None:
        f = re.match(r'^\s+[0-9a-f]+\s+(.*?)\s+\((.*)\)$', line)
        if f:
            current[5].append(re.sub(r'\+0x[0-9a-f]+$', '', f[1]) + ' [' + os.path.basename(f[2]) + ']')
main_pid = next(e[1] for e in events if e[3] == 'stack')
tid = main_pid if args.tid == 'main' else int(args.tid)
interesting = re.compile(r'Omareel|omareel|QQml|QQuick|QSG|QWayland|QMedia|QAudio|QPlatform|QGuiApplication|QFont|Editor|Theme|main \[|egl|wl_display|pw_|QProcess|join|wait|Fc[A-Z]|poll|read|futex|_dl_|dlopen', re.I)


def label(frames):
    names = [f for f in frames if interesting.search(f) and '[unknown]' not in f.split(' [')[0]]
    leaf = next((f for f in frames if '[unknown]' not in f.split(' [')[0]), frames[0] if frames else '?')
    chain = [re.sub(r'\(.*?\)( const)?', '', n.split(' [')[0])[:70] for n in reversed(names)]
    chain = [c for c in chain if not re.match(r'^(main|__libc_start_main|_start)$', c)][:4]
    return ' > '.join(chain) + '  || ' + re.sub(r'\(.*', '', leaf)[:60]


mine = [e for e in events if e[2] == tid]
waits, pending, last_stack = [], None, None
for e in mine:
    if e[3] == 'stack':
        last_stack = e
    elif e[3] == 'out':
        pending = (e[0], e[4], last_stack[5] if last_stack and abs(last_stack[0] - e[0]) < 0.002 else [])
    elif e[3] == 'in' and pending:
        waits.append((pending[0], e[0] - pending[0], pending[1], pending[2]))
        pending = None
print(f'thread {tid}: waits over {args.min} ms before {args.until} s (start, ms, where):')
total = collections.Counter()
for t, d, preempt, frames in waits:
    if t > args.until:
        continue
    key = 'preempted' if preempt else label(frames)
    total[key] += d
    if d * 1000 >= args.min:
        print(f'  {t:6.3f} {d * 1000:6.1f}  {key[:230]}')
print('\ntotal waited, by stack:')
for key, d in total.most_common(15):
    print(f'  {d * 1000:6.1f}  {key[:230]}')
