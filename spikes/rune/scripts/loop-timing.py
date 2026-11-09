#!/usr/bin/env python3
"""How quickly each window of the recording loop appears, measured the same way for Qt and Rune.
Runs inside tests/tools/nested-hyprland.sh after in-nested.sh's output setup:

    loop-timing.py <out.json> <runs> <app binary>

Both apps share the recorder (crates/omareel_record): the region picker, the daemon, the
capture and the finishing of the bundle are the same code. What differs is the toolkit
processes the recorder starts: the launcher, the recording bar (with the countdown), and the
editor it opens on the new recording. Each run:

1. starts the app with no arguments (the launcher) and times it until Hyprland maps its window;
2. presses Ctrl+R in it (the region picker, shared, timed as a check);
3. clicks the bare desktop in the picker (the whole output), and times the recording bar's
   layer surface from the click (the countdown is on, so the bar shows the countdown first);
4. records for 5 s, then runs `<app> record --stop` and times the editor's window from the
   stop: the recorder finishing the bundle (shared) plus the editor's launch;
5. closes the editor and waits for the recording state to clear.

A watcher polls `hyprctl -j clients` and `hyprctl -j layers` every few milliseconds and logs
when each window or layer surface first appears (Hyprland maps a surface on its first buffer,
so this is when it is on screen). The capture-exclusion plugin is expected
(OMAREEL_PLUGIN_PATH), so the recorder keeps the bar on the recorded output.
"""
import json
import os
import statistics
import subprocess
import sys
import threading
import time

out, runs, app = sys.argv[1], int(sys.argv[2]), os.path.realpath(sys.argv[3])
pointer = os.environ.get('SPIKE_POINTER', os.path.join(os.path.dirname(app), 'spike-pointer'))
work = os.environ.get('LOOP_DIR', os.path.join(os.environ.get('XDG_RUNTIME_DIR', '/tmp'), 'loop-timing'))
recordings = os.path.join(work, 'recordings')
os.makedirs(recordings, exist_ok=True)
state_path = os.path.join(os.environ['XDG_RUNTIME_DIR'], 'omareel', 'recording.json')
config = os.path.join(os.environ.get('XDG_CONFIG_HOME', os.path.expanduser('~/.config')), 'omareel')
os.makedirs(config, exist_ok=True)
json.dump({'systemAudio': os.environ.get('LOOP_AUDIO') == '1', 'microphone': False, 'countdownBeforeRecording': True, 'selfViewEnabled': False,
           'outputDirectory': recordings, 'webcam': {'enabled': False}}, open(os.path.join(config, 'settings.json'), 'w'))
env = dict(os.environ, OMAREEL_RECENTS_DIR=recordings)


def hyprctl(*args):
    try:
        return subprocess.run(['hyprctl', '-j', *args], capture_output=True, text=True, timeout=2).stdout
    except subprocess.TimeoutExpired:
        return ''


def step(text):
    with open(os.path.join(work, 'steps.log'), 'a') as f:
        f.write(f'{time.monotonic():.3f} {text}\n')


class Watcher(threading.Thread):
    """First-seen times (monotonic) of each client and layer surface."""

    def __init__(self):
        super().__init__(daemon=True)
        self.seen, self.events, self.running = set(), [], True

    def run(self):
        while self.running:
            now = time.monotonic()
            try:
                clients = json.loads(hyprctl('clients') or '[]')
                layers = json.loads(hyprctl('layers') or '{}')
            except json.JSONDecodeError:
                continue
            current = set()
            for c in clients:
                if c.get('mapped', True):
                    current.add(('client', c['class'], c['pid'], c['address']))
            for monitor in layers.values():
                for level in monitor.get('levels', {}).values():
                    for layer in level:
                        current.add(('layer', layer['namespace'], layer['pid'], layer['address']))
            for item in current - self.seen:
                self.events.append((now, item[0], item[1], item[2]))
            self.seen = current
            time.sleep(0.004)

    def first(self, after, kind, pred=lambda name, pid: True):
        for t, k, name, pid in self.events:
            if t >= after and k == kind and pred(name, pid):
                return t, name
        return None, None


def wait(predicate, limit):
    end = time.monotonic() + limit
    while time.monotonic() < end:
        if predicate():
            return True
        time.sleep(0.01)
    return False


def recording_state():
    try:
        return json.load(open(state_path))
    except (OSError, json.JSONDecodeError):
        return None


def omareel_processes():
    """Processes running the app's executable (not this script, whose arguments name it)."""
    found = []
    for pid in os.listdir('/proc'):
        try:
            if pid.isdigit() and os.readlink(f'/proc/{pid}/exe') == app:
                found.append(pid)
        except OSError:
            pass
    return found


results = []
watcher = Watcher()
watcher.start()
time.sleep(0.5)
for n in range(runs):
    run = {}
    # 1. The launcher.
    step(f'run {n}')
    started = time.monotonic()
    launcher = subprocess.Popen([app], env=env, stdout=subprocess.DEVNULL, stderr=open(os.path.join(work, f'launcher{n}.log'), 'w'))
    wait(lambda: watcher.first(started, 'client', lambda name, pid: pid == launcher.pid)[0], 10)
    t, _ = watcher.first(started, 'client', lambda name, pid: pid == launcher.pid)
    run['launcher_ms'] = round((t - started) * 1000, 1) if t else None
    time.sleep(2.0)
    # 2. Ctrl+R: the region picker.
    pressed = time.monotonic()
    step('ctrl+r'); subprocess.run(['wtype', '-M', 'ctrl', 'r', '-m', 'ctrl'], timeout=5)
    wait(lambda: watcher.first(pressed, 'layer')[0], 5)
    t, name = watcher.first(pressed, 'layer')
    run['picker_ms'] = round((t - pressed) * 1000, 1) if t else None
    run['picker_layer'] = name
    time.sleep(1.5)
    # 3. Click the desktop: the recording starts; the bar shows the countdown.
    clicked = time.monotonic()
    step('click'); subprocess.run([pointer, '1700', '950'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
    wait(lambda: (recording_state() or {}).get('bundle'), 10)
    run['state_ms'] = round((time.monotonic() - clicked) * 1000, 1)
    picker_pid = None
    bar = lambda name, pid: name != run['picker_layer'] and pid != launcher.pid
    wait(lambda: watcher.first(clicked, 'layer', bar)[0], 10)
    t, name = watcher.first(clicked, 'layer', bar)
    run['bar_ms'] = round((t - clicked) * 1000, 1) if t else None
    run['bar_layer'] = name
    time.sleep(6.0)
    run['layers_recording'] = sorted({(e[2], e[1]) for e in watcher.events if e[0] >= clicked})
    # 4. Stop; the recorder opens the editor.
    stopped = time.monotonic()
    step('stop'); subprocess.run([app, 'record', '--stop'], env=env, stdout=subprocess.DEVNULL, stderr=open(os.path.join(work, f'stop{n}.log'), 'w'), timeout=30)
    run['stop_command_ms'] = round((time.monotonic() - stopped) * 1000, 1)
    editor = lambda name, pid: pid != launcher.pid
    wait(lambda: watcher.first(stopped, 'client', editor)[0], 15)
    t, name = watcher.first(stopped, 'client', editor)
    run['editor_ms'] = round((t - stopped) * 1000, 1) if t else None
    # 5. Close everything; wait for the recorder to clear its state.
    time.sleep(3.0)
    wait(lambda: recording_state() is None, 15)
    for pid in omareel_processes():
        subprocess.run(['kill', pid])
    step('cleanup')
    try:
        launcher.wait(5)
    except subprocess.TimeoutExpired:
        launcher.kill()
    wait(lambda: not omareel_processes(), 5)
    for bundle in os.listdir(recordings):
        subprocess.run(['rm', '-rf', os.path.join(recordings, bundle)])
    results.append(run)
    print(n, json.dumps(run), flush=True)
    time.sleep(2.0)
watcher.running = False


def median(key):
    values = [r[key] for r in results[1:] if r.get(key) is not None] or [r[key] for r in results if r.get(key) is not None]
    return round(statistics.median(values), 1) if values else None


summary = {'app': app, 'runs': results, 'first': results[0] if results else None,
           'warm_median': {k: median(k) for k in ('launcher_ms', 'picker_ms', 'state_ms', 'bar_ms', 'stop_command_ms', 'editor_ms')},
           'events': [(round(t, 3), k, name, pid) for t, k, name, pid in watcher.events]}
json.dump(summary, open(out, 'w'), indent=1)
print(json.dumps(summary['warm_median']))
