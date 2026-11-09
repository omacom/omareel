#!/usr/bin/env python3
"""Where the editor's startup time goes. Runs inside tests/tools/nested-hyprland.sh after
in-nested.sh's output setup:

    startup-profile.py <out dir> <runs> <bundle template> -- <app> edit

Each run copies the bundle template afresh (so a recording with no project.json opens as a new
one, as it does after Stop), starts `<app> edit <copy>` under `perf record` with
WAYLAND_DEBUG=client, and stops it after SECONDS (default 4). Qt builds also log their scene
graph timings (QT_LOGGING_RULES), stamped with the time since the process started.

Written per run, in <out dir>/<n>/: perf.data, wayland.log, start.json (CLOCK_MONOTONIC and UTC
seconds of day at the launch, to line the perf samples up with the Wayland log), and
thumbs.json (when each new filmstrip or camera-strip frame appeared in the cache). PERF=0 runs
without perf (for the true timings); REOPEN=1 opens the same copy every run (the second and later
runs find their project.json and cached frames, as reopening a recording does). WAIT=<s> starts
`<app> edit --wait <pid> <copy>` instead, the editor the recorder starts while it records, and
SIGUSR1s it after that many seconds: times are then from the signal (as from Stop). Prints the
median first frame and filmstrip times over the runs after the first.
"""
import json
import re
import statistics
import os
import shutil
import signal
import subprocess
import sys
import time

out, runs, template = sys.argv[1], int(sys.argv[2]), sys.argv[3]
command = sys.argv[sys.argv.index('--') + 1:]
perf = os.environ.get('PERF_BIN', os.path.expanduser('~/.cache/omareel-spikes/tools/perf'))
seconds = float(os.environ.get('SECONDS_RUN', '4'))
use_perf = os.environ.get('PERF', '1') == '1'
freq = os.environ.get('PERF_FREQ', '2999')
reopen = os.environ.get('REOPEN', '0') == '1'
wait = float(os.environ.get('WAIT') or 0)
os.makedirs(out, exist_ok=True)
cache = os.path.join(os.environ.get('XDG_CACHE_HOME', os.path.expanduser('~/.cache')), 'omareel', 'omareel')
stamp = re.compile(r'^\[(\d+):(\d+):(\d+\.\d+)\]\s*(?:\{[^}]*\}\s*)?(.*)')


def frames_in_cache():
    found = set()
    for kind in ('clip-filmstrips-v2', 'camera-filmstrips'):
        for root, _, files in os.walk(os.path.join(cache, kind)):
            found.update(os.path.join(root, f) for f in files if f.endswith('.jpg') and '.tmp.' not in f)
    return found


def first_frame(log, utc):
    attached = False
    for text in open(log, errors='replace'):
        m = stamp.match(text)
        if not m:
            continue
        if re.search(r'-> wl_surface[#@]\d+\.attach\(wl_buffer', m[4]):
            attached = True
        elif attached and re.search(r'-> wl_surface[#@]\d+\.commit', m[4]):
            return round(int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3]) - utc, 4)
    return None


results = []

for n in range(runs):
    run = os.path.join(out, str(n))
    shutil.rmtree(run, ignore_errors=True)
    os.makedirs(run)
    bundle = os.path.join(os.environ.get('XDG_RUNTIME_DIR', '/tmp'), 'profile-bundle' if reopen else f'profile-bundle-{n}')
    if not (reopen and n > 0):
        shutil.rmtree(bundle, ignore_errors=True)
        subprocess.run(['cp', '-r', '--reflink=auto', template, bundle], check=True)
    before = frames_in_cache()
    env = dict(os.environ, WAYLAND_DEBUG='client',
               QT_MESSAGE_PATTERN='[%{time process}] %{category}: %{message}',
               QT_LOGGING_RULES='qt.scenegraph.time.*=true;qt.scenegraph.general=true;qt.rhi.general=true')
    holder = subprocess.Popen(['sleep', '1000']) if wait else None
    # NO_BUNDLE=1: the command takes no bundle (the launcher: `<app>` alone).
    argv = command + (['--wait', str(holder.pid)] if wait else []) + ([] if os.environ.get('NO_BUNDLE') == '1' else [bundle])
    if use_perf:
        argv = [perf, 'record', '-q', '-k', 'mono'] + (os.environ['PERF_EVENT'].split() if os.environ.get('PERF_EVENT') else ['-F', freq]) + [ '--call-graph', os.environ.get('CALLGRAPH', 'dwarf,24576'), '-o',
                os.path.join(run, 'perf.data'), '--'] + argv
    started_mono, started_utc = time.monotonic(), time.time() % 86400
    process = subprocess.Popen(argv, env=env, stdout=open(os.path.join(run, 'stdout.log'), 'w'),
                               stderr=open(os.path.join(run, 'wayland.log'), 'w'), start_new_session=True)
    if wait:
        # The editor is perf's child; once it is waiting, the signal is the start.
        time.sleep(wait)
        editor = int(open(f'/proc/{process.pid}/task/{process.pid}/children').read().split()[0]) if use_perf else process.pid
        started_mono, started_utc = time.monotonic(), time.time() % 86400
        os.kill(editor, signal.SIGUSR1)
    json.dump({'mono': started_mono, 'utc': started_utc, 'argv': argv}, open(os.path.join(run, 'start.json'), 'w'))
    appeared = {}
    while time.monotonic() - started_mono < seconds:
        now = time.monotonic() - started_mono
        for f in frames_in_cache() - before - appeared.keys():
            appeared[f] = round(now, 3)
        time.sleep(0.02)
    # End the app (perf's child), then let perf write its data.
    try:
        kids = open(f'/proc/{process.pid}/task/{process.pid}/children').read().split() if use_perf else [str(process.pid)]
    except OSError:
        kids = []
    for kid in kids:
        try:
            os.kill(int(kid), signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        process.wait(30)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
    if holder:
        holder.kill()
    if not reopen:
        shutil.rmtree(bundle, ignore_errors=True)
    times = sorted(appeared.values())
    film = sorted(t for f, t in appeared.items() if 'clip-filmstrips' in f)
    result = {'first_frame_s': first_frame(os.path.join(run, 'wayland.log'), started_utc),
              'filmstrip_frames': len(film), 'filmstrip_first_s': film[0] if film else None,
              'filmstrip_last_s': film[-1] if film else None,
              'camera_frames': len(times) - len(film), 'camera_last_s': max((t for f, t in appeared.items() if 'camera' in f), default=None)}
    json.dump(dict(result, frames=appeared), open(os.path.join(run, 'thumbs.json'), 'w'), indent=1)
    results.append(result)
    print(n, json.dumps(result), flush=True)
    time.sleep(1.5)


def median(key):
    values = [r[key] for r in results[1:] if r.get(key) is not None] or [r[key] for r in results if r.get(key) is not None]
    return round(statistics.median(values), 3) if values else None


summary = {k: median(k) for k in results[0]} if results else {}
json.dump({'runs': results, 'median_after_first': summary}, open(os.path.join(out, 'summary.json'), 'w'), indent=1)
print('median after the first run:', json.dumps(summary))
