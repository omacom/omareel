#!/usr/bin/env python3
"""Cursor sampler checks. Invoke through nested-hyprland.sh, never directly.

  probe     the sampler alone (examples/cursor_probe), with hardware and software cursors,
            moving the pointer with `hyprctl dispatch movecursor`, onto a second (headless)
            output and back, then with permissions enforced (polling fallback)
  record    a short fullscreen recording with the built omareel, checking input.jsonl
  crash     a recorder killed while it holds hardware cursors, and the next start restoring

Environment: OMAREEL_CURSOR_PROBE (the cursor_probe binary), OMAREEL_APP (omareel).
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('mode', choices=['probe', 'record', 'crash'])
args = parser.parse_args()
assert os.environ.get('OMAREEL_NESTED_TEST'), 'Use the nested harness'
root = Path(os.environ['OMAREEL_TEST_ARTIFACTS'])
report = {'mode': args.mode}


def ctl(*command):
    result = subprocess.run(['hyprctl', *command], text=True, capture_output=True, timeout=10)
    assert result.returncode == 0, (command, result.stdout, result.stderr)
    return result.stdout


def monotonic_us():
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1000


def monitors():
    return json.loads(ctl('-j', 'monitors'))


def first_monitor():
    """The nested output, once the parent compositor has configured its window."""
    for attempt in range(100):
        found = monitors()
        if found and found[0]['width'] > 0:
            time.sleep(0.5)
            return monitors()[0]
        time.sleep(0.1)
    raise RuntimeError('the nested output never appeared')


def move(x, y):
    """Moves the pointer and says when (CLOCK_MONOTONIC, as the samples)."""
    before = monotonic_us()
    ctl('dispatch', 'movecursor', str(x), str(y))
    return before


def first_at(samples, x, y, after):
    """The first sample at (x, y) from `after` on."""
    return next((s for s in samples if s[0] >= after and s[1] == x and s[2] == y), None)


def largest_gap(samples, start, end):
    times = [s[0] for s in samples if start <= s[0] <= end]
    return max((b - a for a, b in zip(times, times[1:])), default=None)


def probe_run(label, monitor, targets, settle=0.4):
    probe = subprocess.Popen([os.environ['OMAREEL_CURSOR_PROBE'], monitor, '60'], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(0.5)
    moves = []
    for x, y in targets:
        moves.append((move(x, y), x, y))
        time.sleep(settle)
    still_start = monotonic_us()
    time.sleep(1.0)
    still_end = monotonic_us()
    out, err = probe.communicate(timeout=10)
    result = json.loads(out)
    samples = result['samples']
    rows = []
    for at, x, y in moves:
        sample = first_at(samples, x, y, at)
        rows.append({'target': [x, y], 'latency_ms': None if sample is None else (sample[0] - at) / 1000})
    setting = json.loads(ctl('-j', 'getoption', 'cursor:no_hardware_cursors'))['int']
    entry = {'label': label, 'no_hardware_cursors': setting, 'source': result['source'], 'samples': len(samples),
             'seconds': round(result['seconds'], 2), 'ipc_requests': result['ipc_requests'], 'stderr': err.strip(),
             'moves': rows, 'still_gap_ms': (largest_gap(samples, still_start, still_end) or 0) / 1000}
    report.setdefault('runs', []).append(entry)
    (root / f'probe-{label}.json').write_text(json.dumps(result))
    return entry


try:
    if args.mode == 'probe':
        first = first_monitor()
        name, ox, oy = first['name'], first['x'], first['y']
        report['monitor'] = first['name']
        targets = [(ox + 100, oy + 100), (ox + 800, oy + 450), (ox + 1500, oy + 800), (ox + 3, oy + 7)]
        for setting, label in ((1, 'software'), (0, 'hardware')):
            ctl('keyword', 'cursor:no_hardware_cursors', str(setting))
            time.sleep(0.2)
            probe_run(label, name, targets)
        # Small moves over one surface (no cursor shape or focus change), hardware cursor.
        if os.environ.get('OMAREEL_APP'):
            layer = subprocess.Popen([os.environ['OMAREEL_APP'], '__layer-test', '--namespace', 'cursor-probe',
                                      '--x', '400', '--y', '300', '--w', '300', '--h', '200', '--color', '#336699'],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(1.0)
            move(ox + 500, oy + 400)
            time.sleep(0.3)
            inside = [(ox + 500 + 7 * i, oy + 400 + 3 * i) for i in range(1, 9)]
            for setting, label in ((0, 'hardware-over-surface'), (1, 'software-over-surface')):
                ctl('keyword', 'cursor:no_hardware_cursors', str(setting))
                time.sleep(0.2)
                move(ox + 500, oy + 400)
                time.sleep(0.3)
                probe_run(label, name, inside, settle=0.15)
            layer.terminate()
            layer.wait(timeout=5)
        # A second output to leave to: the session goes silent, IPC says where the pointer went.
        ctl('output', 'create', 'headless', 'CURSOR-PROBE-2')
        time.sleep(0.5)
        second = next(m for m in monitors() if m['name'] == 'CURSOR-PROBE-2')
        report['second_monitor'] = {k: second[k] for k in ('x', 'y', 'width', 'height', 'scale')}
        ctl('keyword', 'cursor:no_hardware_cursors', '1')
        away = (second['x'] + 200, second['y'] + 200)
        probe_run('leave', name, [(ox + 400, oy + 300), away, (ox + 600, oy + 500)])
        # The pointer starting off the recorded output.
        move(*away)
        time.sleep(0.2)
        probe_run('start-away', name, [(second['x'] + 300, second['y'] + 250), (ox + 700, oy + 200)])
        ctl('output', 'remove', 'CURSOR-PROBE-2')
        # Permissions enforced: the session is not tried, polling follows the pointer.
        ctl('keyword', 'ecosystem:enforce_permissions', '1')
        time.sleep(0.2)
        probe_run('enforced', name, targets)
        ctl('keyword', 'ecosystem:enforce_permissions', '0')
    elif args.mode == 'crash':
        first_monitor()
        app = os.environ['OMAREEL_APP']
        plugin = Path(app).parent / 'plugin/omareel-capture-exclude.so'
        if plugin.exists():
            ctl('plugin', 'load', str(plugin))
            time.sleep(0.3)
        ctl('keyword', 'cursor:no_hardware_cursors', '1')
        runtime = Path(os.environ['XDG_RUNTIME_DIR']) / 'omareel'
        marker = runtime / 'hardware-cursor-hold.json'
        setting = lambda: json.loads(ctl('-j', 'getoption', 'cursor:no_hardware_cursors'))['int']
        record = [app, 'record', '--fullscreen', '--no-audio', '--no-open', '--dir', str(root / 'recordings')]
        result = subprocess.run(record, text=True, capture_output=True, timeout=60)
        assert result.returncode == 0, (result.stdout, result.stderr)
        daemon = json.loads((runtime / 'recording.json').read_text())['pid']
        report['first'] = {'daemon': daemon, 'setting': setting(), 'marker': json.loads(marker.read_text())}
        os.kill(daemon, 9)
        time.sleep(1.0)
        report['after_kill'] = {'setting': setting(), 'marker': marker.exists()}
        # The next recording puts the user's value back before holding again.
        result = subprocess.run(record, text=True, capture_output=True, timeout=60)
        assert result.returncode == 0, (result.stdout, result.stderr)
        daemon = json.loads((runtime / 'recording.json').read_text())['pid']
        report['second'] = {'daemon': daemon, 'setting': setting(), 'marker': json.loads(marker.read_text())}
        result = subprocess.run([app, 'record', '--stop'], text=True, capture_output=True, timeout=60)
        report['after_stop'] = {'code': result.returncode, 'setting': setting(), 'marker': marker.exists()}
        assert report['after_kill'] == {'setting': 0, 'marker': True}, report
        assert report['second']['marker']['previous'] == 1, report
        assert report['after_stop'] == {'code': 0, 'setting': 1, 'marker': False}, report
    else:
        first = first_monitor()
        ox, oy = first['x'], first['y']
        app = os.environ['OMAREEL_APP']
        # Loading the plugin reloads the configuration, which would undo a keyword set before it:
        # load it first, so the recorder finds it loaded and the hold starts from software cursors.
        plugin = Path(app).parent / 'plugin/omareel-capture-exclude.so'
        if plugin.exists():
            report['plugin_load'] = ctl('plugin', 'load', str(plugin)).strip()
            time.sleep(0.3)
        ctl('keyword', 'cursor:no_hardware_cursors', '1')
        report['setting_before'] = json.loads(ctl('-j', 'getoption', 'cursor:no_hardware_cursors'))
        result = subprocess.run([app, 'record', '--fullscreen', '--no-audio', '--no-open', '--dir',
                                 str(root / 'recordings')], text=True, capture_output=True, timeout=60)
        assert result.returncode == 0, (result.stdout, result.stderr)
        state = json.loads((Path(os.environ['XDG_RUNTIME_DIR']) / 'omareel/recording.json').read_text())
        bundle = Path(state['bundle'])
        report['held_setting'] = json.loads(ctl('-j', 'getoption', 'cursor:no_hardware_cursors'))
        marker = Path(os.environ['XDG_RUNTIME_DIR']) / 'omareel/hardware-cursor-hold.json'
        report['marker_while_recording'] = json.loads(marker.read_text()) if marker.exists() else None
        moves = []
        for x, y in [(ox + 100, oy + 100), (ox + 800, oy + 450), (ox + 1500, oy + 800), (ox + 3, oy + 7)]:
            moves.append((move(x, y), x, y))
            time.sleep(0.4)
        time.sleep(0.6)
        result = subprocess.run([app, 'record', '--stop'], text=True, capture_output=True, timeout=60)
        assert result.returncode == 0, (result.stdout, result.stderr)
        report['restored_setting'] = json.loads(ctl('-j', 'getoption', 'cursor:no_hardware_cursors'))
        report['marker_after'] = marker.exists()
        capture = json.loads((bundle / 'capture.json').read_text())
        region, scale = capture['region'], capture['scale']
        report['region'], report['scale'] = region, scale
        lines = [json.loads(line) for line in (bundle / 'input.jsonl').read_text().splitlines()]
        # Back to global logical coordinates (input.jsonl holds capture pixels).
        samples = [[line['t'], round(line['x'] / scale + region['x'], 6), round(line['y'] / scale + region['y'], 6)]
                   for line in lines if line['k'] == 'm']
        report['samples'] = len(samples)
        report['moves'] = []
        for at, x, y in moves:
            sample = first_at(samples, x, y, at)
            report['moves'].append({'target': [x, y],
                                    'latency_ms': None if sample is None else (sample[0] - at) / 1000})
        report['largest_gap_ms'] = (largest_gap(samples, samples[0][0], samples[-1][0]) or 0) / 1000
finally:
    (root / f'report-{args.mode}.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
