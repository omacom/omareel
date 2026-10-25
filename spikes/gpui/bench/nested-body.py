#!/usr/bin/env python3
"""Runs one windowed measurement inside tests/tools/nested-hyprland.sh (never directly).

The nested compositor's own output follows the outer display (a 30 Hz TV here), so a 60 Hz
headless output is added and the client opens there. While it plays, this samples the
process tree's CPU time and RSS and its DRM clients' GPU engine time (i915 fdinfo), saves a
screenshot, and writes everything to result.json in the artifacts directory.

  nested-body.py --name NAME --start 1.5 --seconds 10 --shot 4 -- command...
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--name', required=True)
parser.add_argument('--start', type=float, default=1.5, help='seconds until playback starts')
parser.add_argument('--seconds', type=float, default=10)
parser.add_argument('--shot', type=float, default=4.0, help='screenshot this long after the start')
parser.add_argument('--env', action='append', default=[])
parser.add_argument('--wait', type=float, default=0, help='let the client quit by itself this long after the end')
parser.add_argument('command', nargs=argparse.REMAINDER)
args = parser.parse_args()
assert os.environ.get('OMAREEL_NESTED_TEST'), 'use the nested harness'
root = Path(os.environ['OMAREEL_TEST_ARTIFACTS'])
command = args.command[1:] if args.command[:1] == ['--'] else args.command

def ctl(*words):
    return subprocess.run(['hyprctl', *words], capture_output=True, text=True, timeout=10).stdout

ctl('output', 'create', 'headless', 'SPIKE')
time.sleep(0.5)
ctl('keyword', 'monitor', 'SPIKE,1920x1080@60,5000x0,1')
time.sleep(0.5)
ctl('dispatch', 'focusmonitor', 'SPIKE')
(root / 'monitors.txt').write_text(ctl('monitors'))

env = os.environ.copy()
for item in args.env:
    key, value = item.split('=', 1)
    env[key] = value
ticks = os.sysconf('SC_CLK_TCK')
page = os.sysconf('SC_PAGE_SIZE')

def tree(pid):
    pids = [pid]
    for p in pids:
        try:
            for task in Path(f'/proc/{p}/task').iterdir():
                children = (task / 'children').read_text().split()
                pids.extend(int(c) for c in children)
        except OSError:
            pass
    return pids

def cpu_seconds(pid):
    try:
        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / ticks
    except (OSError, IndexError):
        return 0.0

def rss_mb(pid):
    try:
        return int(Path(f'/proc/{pid}/statm').read_text().split()[1]) * page / 1e6
    except (OSError, IndexError):
        return 0.0

def gpu_ns(pid):
    """Sum of drm-engine-* over the process's DRM clients, each client once."""
    engines = {}
    try:
        fds = list(Path(f'/proc/{pid}/fdinfo').iterdir())
    except OSError:
        return engines
    seen = set()
    for fd in fds:
        try:
            text = fd.read_text()
        except OSError:
            continue
        if 'drm-client-id' not in text:
            continue
        info = dict(line.split(':', 1) for line in text.splitlines() if ':' in line)
        client = info['drm-client-id'].strip()
        if client in seen:
            continue
        seen.add(client)
        for key, value in info.items():
            if key.startswith('drm-engine-') and not key.startswith('drm-engine-capacity'):
                engines[key[11:]] = engines.get(key[11:], 0) + int(value.split()[0])
    return engines

stdout = open(root / f'{args.name}.stdout', 'w')
stderr = open(root / f'{args.name}.stderr', 'w')
launched = time.monotonic()
epoch_start = time.time() + args.start
process = subprocess.Popen(command, env=env, stdout=stdout, stderr=stderr)
start = launched + args.start
end = start + args.seconds
samples = []
first = None
shot_taken = False
while process.poll() is None and time.monotonic() < end + 0.5:
    now = time.monotonic()
    if now >= start:
        pids = tree(process.pid)
        sample = {
            't': now,
            'cpu_main': cpu_seconds(process.pid),
            'cpu_tree': sum(cpu_seconds(p) for p in pids),
            'rss_main': rss_mb(process.pid),
            'rss_tree': sum(rss_mb(p) for p in pids),
            'gpu': gpu_ns(process.pid),
        }
        samples.append(sample)
    if not shot_taken and now >= start + args.shot:
        subprocess.run(['grim', '-o', 'SPIKE', str(root / f'{args.name}.png')], timeout=10)
        shot_taken = True
    time.sleep(0.25)
clients = ctl('-j', 'clients')
(root / f'{args.name}-clients.json').write_text(clients)
try:
    process.wait(timeout=args.wait)
except subprocess.TimeoutExpired:
    pass
if process.poll() is None:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
measured = [s for s in samples if s['t'] <= end + 0.01]
result = {'name': args.name, 'command': command, 'epoch_start': epoch_start, 'epoch_end': epoch_start + args.seconds}
if len(measured) >= 2:
    a, b = measured[0], measured[-1]
    span = b['t'] - a['t']
    result.update(
        span=span,
        cpu_main_pct=100 * (b['cpu_main'] - a['cpu_main']) / span,
        cpu_tree_pct=100 * (b['cpu_tree'] - a['cpu_tree']) / span,
        rss_main_mb_max=max(s['rss_main'] for s in measured),
        rss_tree_mb_max=max(s['rss_tree'] for s in measured),
        gpu_pct={k: 100 * (b['gpu'].get(k, 0) - a['gpu'].get(k, 0)) / (span * 1e9) for k in b['gpu']},
    )
(root / f'{args.name}.json').write_text(json.dumps(result, indent=2))
print(json.dumps(result))
