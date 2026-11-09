#!/usr/bin/env python3
"""The QML profiler's view of the Qt editor's start. Runs inside tests/tools/nested-hyprland.sh
(after in-nested.sh) with a build that enables QML debugging (-DQT_QML_DEBUG):

    qml-profile.py <out.qtd> <bundle template> <seconds> -- <omareel> edit

copies the bundle, starts the app under `qmlprofiler --interactive`, flushes the trace after
<seconds> and quits. Summarise it with `qml-profile.py --summary <out.qtd> [--until S]`: the
compiling, creating, binding and JavaScript time by QML file and line, up to S seconds after the
engine started, and when the first scene graph frame was rendered.
"""
import collections
import os
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET


def summary(path, until):
    root = ET.parse(path).getroot()
    events = {}
    for e in root.iter('event'):
        index = int(e.get('index'))
        events[index] = {k.tag: (k.text or '') for k in e}
    # The GUI thread's ranges (compiling, creating, bindings, JavaScript, signal handlers) nest;
    # each range's self time is its duration less its direct children's.
    ranges = []
    for r in root.iter('range'):
        if r.get('duration') is None:
            continue
        ev = events.get(int(r.get('eventIndex')), {})
        kind = ev.get('type', '?')
        if kind not in ('Compiling', 'Creating', 'Binding', 'Javascript', 'HandlingSignal'):
            continue
        start = int(r.get('startTime'))
        if start / 1e9 > until:
            continue
        where = f"{os.path.basename(ev.get('filename', ''))}:{ev.get('line', '')}"
        ranges.append([start, int(r.get('duration')), kind, f"{kind:<12} {where:<30} {ev.get('details', '')[:60]}", where])
    ranges.sort(key=lambda r: (r[0], -r[1]))
    self_time = [r[1] for r in ranges]
    stack = []
    for i, (start, duration, *_rest) in enumerate(ranges):
        while stack and ranges[stack[-1]][0] + ranges[stack[-1]][1] <= start:
            stack.pop()
        if stack:
            self_time[stack[-1]] -= duration
        stack.append(i)
    by_label, counts, by_kind, by_file = collections.Counter(), collections.Counter(), collections.Counter(), collections.Counter()
    for r, own in zip(ranges, self_time):
        by_label[r[3]] += own
        counts[r[3]] += 1
        by_kind[r[2]] += own
        by_file[r[4].split(':')[0]] += own
    print(f'{path}: up to {until} s after the engine started; {sum(self_time) / 1e6:.1f} ms of QML work (self time)')
    print('by kind (ms):')
    for kind, ns in by_kind.most_common():
        print(f'  {ns / 1e6:8.1f}  {kind}')
    print('by file (ms):')
    for name, ns in by_file.most_common(20):
        print(f'  {ns / 1e6:8.1f}  {name}')
    print('top ranges by self time (ms, count):')
    for label, ns in by_label.most_common(40):
        print(f'  {ns / 1e6:8.1f} {counts[label]:5}  {label}')


if sys.argv[1] == '--summary':
    until = float(sys.argv[sys.argv.index('--until') + 1]) if '--until' in sys.argv else 1.0
    summary(sys.argv[2], until)
    sys.exit()

out, template, seconds = sys.argv[1], sys.argv[2], float(sys.argv[3])
command = sys.argv[sys.argv.index('--') + 1:]
bundle = os.path.join(os.environ.get('XDG_RUNTIME_DIR', '/tmp'), 'qml-profile-bundle')
shutil.rmtree(bundle, ignore_errors=True)
subprocess.run(['cp', '-r', '--reflink=auto', template, bundle], check=True)
profiler = subprocess.Popen(['qmlprofiler', '--interactive', '-o', out, '--', *command, bundle], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
time.sleep(seconds)
profiler.stdin.write('f\n')
profiler.stdin.flush()
time.sleep(3)
profiler.stdin.write('q\n')
profiler.stdin.flush()
try:
    output, _ = profiler.communicate(timeout=20)
except subprocess.TimeoutExpired:
    profiler.kill()
    output, _ = profiler.communicate()
print(output[-2000:])
shutil.rmtree(bundle, ignore_errors=True)
