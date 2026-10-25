#!/usr/bin/env python3
"""Runs one windowed client inside tests/tools/nested-hyprland.sh (never directly).

The nested compositor's own output follows the outer display (a 30 Hz TV here), so a 60 Hz
headless output is added and the client opens there. From launch until the client exits (or
the timeout), this samples the process tree's CPU time and RSS and its DRM clients' GPU engine
time (i915 fdinfo) four times a second into NAME.samples.json; summary.py picks the measured
window afterwards. A screenshot is saved SHOT seconds after the client prints the start marker.

  nested-body.py --name NAME [--shot 3] [--timeout 25] [--env K=V]... -- command...
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--name', required=True)
parser.add_argument('--shot', type=float, default=3.0, help='screenshot this long after the start marker')
parser.add_argument('--timeout', type=float, default=25, help='stop the client this long after launch')
parser.add_argument('--start-marker', default='preview state=1')
parser.add_argument('--env', action='append', default=[])
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
                pids.extend(int(c) for c in (task / 'children').read_text().split())
        except OSError:
            pass
    return pids

def cpu_seconds(pid, children=False):
    """utime + stime, and with `children` the reaped children's too (cutime + cstime), so a
    decoder that exits between samples keeps counting."""
    try:
        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        return sum(int(f) for f in fields[11:15 if children else 13]) / ticks
    except (OSError, IndexError):
        return 0.0

def threads(pid):
    """CPU seconds per thread name (threads of one name summed)."""
    result = {}
    try:
        tasks = list(Path(f'/proc/{pid}/task').iterdir())
    except OSError:
        return result
    for task in tasks:
        try:
            comm = (task / 'comm').read_text().strip()
            fields = (task / 'stat').read_text().rsplit(')', 1)[1].split()
            result[comm] = result.get(comm, 0) + (int(fields[11]) + int(fields[12])) / ticks
        except (OSError, IndexError):
            pass
    return result

def rss_mb(pid):
    try:
        return int(Path(f'/proc/{pid}/statm').read_text().split()[1]) * page / 1e6
    except (OSError, IndexError):
        return 0.0

def gpu_ns(pid):
    """drm-engine-* summed over the process's DRM clients, each client once."""
    engines, seen = {}, set()
    try:
        fds = list(Path(f'/proc/{pid}/fdinfo').iterdir())
    except OSError:
        return engines
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

stderr_path = root / f'{args.name}.stderr'
stdout = open(root / f'{args.name}.stdout', 'w')
stderr = open(stderr_path, 'w')
launched = time.monotonic()
process = subprocess.Popen(command, env=env, stdout=stdout, stderr=stderr)
samples, marker, shot = [], None, False
while process.poll() is None and time.monotonic() < launched + args.timeout:
    pids = tree(process.pid)
    samples.append({
        'epoch': time.time(),
        'cpu_main': cpu_seconds(process.pid),
        'cpu_tree': sum(cpu_seconds(p, children=True) for p in pids),
        'rss_main': rss_mb(process.pid),
        'rss_tree': sum(rss_mb(p) for p in pids),
        'gpu': gpu_ns(process.pid),
        'threads': threads(process.pid),
    })
    if marker is None and args.start_marker in stderr_path.read_text(errors='replace'):
        marker = time.time()
    if marker is not None and not shot and time.time() >= marker + args.shot:
        subprocess.run(['grim', '-o', 'SPIKE', str(root / f'{args.name}.png')], timeout=10)
        shot = True
    time.sleep(0.25)
(root / f'{args.name}-clients.json').write_text(ctl('-j', 'clients'))
if process.poll() is None:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
(root / f'{args.name}.samples.json').write_text(json.dumps({'marker': marker, 'samples': samples}))
