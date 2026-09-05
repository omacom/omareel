#!/usr/bin/env python3
"""Pixel and lifecycle checks. Invoke through nested-hyprland.sh, never directly."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
import numpy as np
from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument('mode', choices=['pixels', 'hygiene', 'overlays', 'fallback', 'fallback-two', 'host-plugin', 'stability', 'recorder-cycles'])
args = parser.parse_args()
assert os.environ.get('OMAREEL_NESTED_TEST'), 'Use the nested harness'
root = Path(os.environ['OMAREEL_TEST_ARTIFACTS'])
repo = Path(__file__).resolve().parents[2]
app = str(repo / 'build/omareel')
plugin = str(repo / 'build/plugin/omareel-capture-exclude.so')
instance = json.loads((root / 'instance.json').read_text())
assert instance['HYPRLAND_INSTANCE_SIGNATURE'] == os.environ['HYPRLAND_INSTANCE_SIGNATURE']
outer = os.environ.copy()
outer.update(XDG_RUNTIME_DIR=os.environ['OMAREEL_OUTER_RUNTIME'], WAYLAND_DISPLAY=os.environ['OMAREEL_OUTER_DISPLAY'],
             HYPRLAND_INSTANCE_SIGNATURE=os.environ['OMAREEL_OUTER_SIGNATURE'])
commands = (root / 'commands.jsonl').open('w')
clients_log = (root / 'clients.log').open('w')
children = []
report = {'mode': args.mode}

def run(*command, env=None, timeout=30, check=True):
    started = time.monotonic()
    result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=timeout)
    commands.write(json.dumps({'command': command, 'outer': env is outer, 'returncode': result.returncode,
                              'stdout': result.stdout, 'stderr': result.stderr, 'seconds': time.monotonic() - started}) + '\n')
    commands.flush()
    if check:
        assert result.returncode == 0, (command, result.stdout, result.stderr)
    return result.stdout

def ctl(*command):
    return run('hyprctl', *command)

def spawn(*command):
    process = subprocess.Popen(command, stdout=clients_log, stderr=clients_log)
    children.append(process)
    return process

def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)

def pause_until(deadline):
    time.sleep(max(0, deadline - time.monotonic()))

def json_file(path):
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return {}

def state():
    return json_file(Path(os.environ['XDG_RUNTIME_DIR']) / 'omareel/recording.json')

def screenshot(name):
    client = next(c for c in json.loads(run('hyprctl', '-j', 'clients', env=outer)) if c['pid'] == instance['pid'])
    box = f"{client['at'][0]},{client['at'][1]} {client['size'][0]}x{client['size'][1]}"
    path = root / (name + '.png')
    run('grim', '-g', box, str(path), env=outer)
    return np.asarray(Image.open(path).convert('RGB'))

def magenta(image):
    return np.all(np.abs(image.astype(np.int16) - [255, 0, 255]) <= 8, axis=2)

def start_record(*extra):
    started = time.monotonic()
    run(app, 'record', '--fullscreen', '--no-audio', '--no-open', '--dir', str(root / 'recordings'), *extra)
    current = state()
    assert current.get('capture_started'), current
    report['startup_seconds'] = time.monotonic() - started
    report['recording_state'] = current
    return Path(current['bundle'])

def stop_record(bundle):
    run(app, 'record', '--stop', timeout=30)
    metadata = json_file(bundle / 'capture.json')
    report['capture'] = metadata
    report['first_frame_ms'] = (metadata['first_frame_us'] - report['recording_state']['daemon_started_us']) / 1000
    report['probe'] = json.loads(run(app, 'probe', str(bundle)))
    return metadata

def inspect_frames(bundle, rects):
    frames = root / 'frames'
    frames.mkdir(exist_ok=True)
    run('ffmpeg', '-v', 'error', '-i', str(bundle / 'screen.mp4'), str(frames / '%04d.png'), timeout=60)
    pixels = []
    contaminated = []
    rectangle_errors = []
    paths = sorted(frames.glob('*.png'))
    for index, path in enumerate(paths):
        frame = np.asarray(Image.open(path).convert('RGB'))
        count = int(magenta(frame).sum())
        if count:
            contaminated.append([index, count])
        samples = []
        for x, y, w, h in rects:
            rect = frame[y+8:y+h-8, x+8:x+w-8]
            samples.append(np.median(rect.reshape(-1, 3), axis=0).tolist())
        reference = np.asarray(samples[-1], dtype=np.int16)
        mismatches = [int(np.any(np.abs(frame[y:y+h, x:x+w].astype(np.int16) - reference) > 8, axis=2).sum())
                      for x, y, w, h in rects[:-1]]
        if any(mismatches):
            rectangle_errors.append({'frame': index, 'pixels': mismatches})
        pixels.append(samples)
    report.update(frames=len(paths), magenta_frames=contaminated, rectangle_errors=rectangle_errors, rect_colors=pixels)
    assert paths and not contaminated, contaminated[:10]
    assert not rectangle_errors, rectangle_errors[:10]
    return np.asarray(pixels)

def layer(scope='omareel-test', *extra):
    return spawn(app, '__layer-test', '--namespace', scope, '--x', '400', '--y', '300', '--w', '300', '--h', '200', '--color', '#ff00ff', *extra)

try:
    # The nested config sets the output mode; wait for the initial window configure.
    time.sleep(.5)
    client = next(c for c in json.loads(run('hyprctl', '-j', 'clients', env=outer)) if c['pid'] == instance['pid'])
    selector = 'address:' + client['address']
    # Only the window created by this harness is changed in the parent compositor.
    run('hyprctl', 'eval', f"hl.dispatch(hl.dsp.window.float({{action='enable',window='{selector}'}}))", env=outer)
    run('hyprctl', 'eval', f"hl.dispatch(hl.dsp.window.resize({{x=1600,y=900,window='{selector}'}}))", env=outer)
    monitor = next(m for m in json.loads(run('hyprctl', '-j', 'monitors', env=outer)) if m['name'] == 'DP-3')
    run('hyprctl', 'eval', f"hl.dispatch(hl.dsp.window.move({{x={monitor['x']+100},y={monitor['y']+150},window='{selector}'}}))", env=outer)
    for prop in ('opacity', 'opacity_inactive', 'opacity_override', 'opacity_inactive_override'):
        run('hyprctl', 'eval', f"hl.dispatch(hl.dsp.window.set_prop({{prop='{prop}',value='1',window='{selector}'}}))", env=outer)
    time.sleep(.5)
    if args.mode.startswith('fallback'):
        os.environ['OMAREEL_PLUGIN_PATH'] = '/nonexistent'
    else:
        ctl('plugin', 'load', plugin)
        report['plugin'] = json.loads(ctl('omareel-exclude', 'status'))
    ctl('dismissnotify')
    if args.mode == 'host-plugin':
        # Reproduce an upgrade with retired redaction rules still present.
        for scope in ('omareel-selfview', 'omareel-record-bar'):
            ctl('eval', f"hl.layer_rule({{name='{scope}-private',match={{namespace='{scope}'}},no_screen_share=true}})")
    if args.mode == 'hygiene':
        ctl('keyword', 'render:keep_unmodified_copy', '1')
        report['mrt_option'] = ctl('getoption', 'render:keep_unmodified_copy')
    ctl('dispatch', 'workspace', '1')
    green = spawn(app, '__window-test', '--color', '#228844', *(['--animate'] if args.mode == 'hygiene' else []))
    time.sleep(.5)

    if args.mode in ('pixels', 'hygiene', 'overlays'):
        ctl('dispatch', 'workspace', '2')
        blue = spawn(app, '__window-test', '--color', '#2266cc')
        time.sleep(.4)
        ctl('dispatch', 'workspace', '1')
        (root / 'move-trigger').write_text('')
        moving = layer('omareel-selfview' if args.mode == 'overlays' else 'omareel-test',
                       *(['--move-to', '900,550', 'after', '60000', '--move-trigger', str(root / 'move-trigger')] if args.mode == 'pixels' else []))
        if args.mode == 'overlays':
            layer('omareel-record-bar', '--x', '650', '--y', '20', '--w', '300', '--h', '40')
        time.sleep(1.2)
        report["layers_before"] = json.loads(ctl("-j", "layers"))
        report["animation_option"] = ctl("getoption", "animations:enabled")
        before = screenshot('display-before')
        report['display_before_magenta'] = int(magenta(before).sum())
        assert report['display_before_magenta'] > 1000
        run('grim', str(root / 'screencopy.png'))
        inside = np.asarray(Image.open(root / 'screencopy.png').convert('RGB'))
        report['screencopy_magenta'] = int(magenta(inside).sum())
        assert report['screencopy_magenta'] == 0
        assert np.max(np.abs(inside[350, 450].astype(int) - [34, 136, 68])) <= 8
        bundle = start_record('--no-webcam', '--no-bar')
        start = time.monotonic()
        report['capturing_status'] = json.loads(ctl('omareel-exclude', 'status'))
        report['start_display_magenta'] = [int(magenta(screenshot(f'display-start-{i}')).sum()) for i in range(3)]
        pause_until(start + (2 if args.mode == 'overlays' else 1.5))
        if args.mode != 'hygiene':
            report['switch_us'] = time.monotonic_ns() // 1000
            ctl('dispatch', 'workspace', '2')
        if args.mode == 'pixels':
            pause_until(start + 2.5)
            report['move_after_record_ready_seconds'] = time.monotonic() - start
            (root / 'move-trigger').write_text('move')
        pause_until(start + 3.2)
        after_move = screenshot('display-during')
        report['display_during_magenta'] = int(magenta(after_move).sum())
        assert report['display_during_magenta'] > 1000
        if args.mode == 'pixels':
            report['old_display_magenta'] = int(np.logical_and(magenta(before), magenta(after_move)).sum())
            assert report['old_display_magenta'] == 0, report['old_display_magenta']
        pause_until(start + {'pixels': 4, 'hygiene': 8, 'overlays': 6}[args.mode])
        metadata = stop_record(bundle)
        report['stop_display_magenta'] = [int(magenta(screenshot(f'display-stop-{i}')).sum()) for i in range(3)]
        assert min(report['start_display_magenta'] + report['stop_display_magenta']) > 1000
        samples = inspect_frames(bundle, [(400, 300, 300, 200), (900, 550, 300, 200), (650, 20, 300, 40), (50, 600, 100, 100)])
        if args.mode != 'hygiene':
            first_blue = int(np.where(samples[:, 0, 2] > 150)[0][0])
            reference_blue = int(np.where(samples[:, -1, 2] > 150)[0][0])
            report.update(first_blue_frame=first_blue, reference_switch_frame=reference_blue,
                          switch_delay_frames=first_blue-reference_blue)
            assert 0 <= first_blue - reference_blue <= 3, report['switch_delay_frames']
            assert np.max(np.abs(samples[:first_blue] - [34, 136, 68])) <= 8
            assert np.max(np.abs(samples[first_blue:] - [34, 102, 204])) <= 8
        assert metadata['overlay_exclusion'] == 'plugin' and metadata['backend'] == 'ext-image-copy-capture'

    elif args.mode.startswith('fallback') or args.mode == 'host-plugin':
        if args.mode == 'fallback-two':
            ctl('output', 'create', 'wayland')
            time.sleep(.5)
            ctl('dispatch', 'focusmonitor', 'WAYLAND-1')
            ctl('dispatch', 'movecursor', '800 450')
            time.sleep(.1)
        prefs = Path(os.environ['XDG_CONFIG_HOME']) / 'omareel/settings.json'
        prefs.parent.mkdir(parents=True, exist_ok=True)
        prefs.write_text(json.dumps({'webcam': {'device': '/dev/video999', 'enabled': True}, 'selfViewEnabled': True}))
        host = spawn(app, '__record-bar', '--standby', '--owner', str(os.getpid()))
        for attempt in range(100):
            host_info = json_file(Path(os.environ['XDG_RUNTIME_DIR']) / 'omareel/selfview-host.json')
            if host_info.get('pid') and host_info.get('camera_error'): break
            assert host.poll() is None, 'standby host exited'
            time.sleep(.05)
        time.sleep(.2)
        report['standby_layers'] = json.loads(ctl('-j', 'layers'))
        host_path = Path(os.environ['XDG_RUNTIME_DIR']) / 'omareel/selfview-host.json'
        report['standby_host'] = json_file(host_path)
        bundle = start_record('--webcam-device', '/dev/video999')
        capture_start = time.monotonic()
        time.sleep(1)
        report['during_layers'] = json.loads(ctl('-j', 'layers'))
        recorded = state()['monitor']
        def scopes(output):
            return [layer['namespace'] for level in output.get('levels', {}).values() for layer in level if layer['namespace'].startswith('omareel-')]
        if args.mode != 'host-plugin':
            assert not scopes(report['during_layers'][recorded]), report['during_layers']
        if args.mode == 'fallback-two':
            other = next(name for name in report['during_layers'] if name != recorded)
            assert {'omareel-selfview', 'omareel-record-bar'} <= set(scopes(report['during_layers'][other]))
        if args.mode == 'host-plugin':
            assert {'omareel-selfview', 'omareel-record-bar'} <= set(scopes(report['during_layers'][recorded]))
            screenshot('actual-overlays-during')
            pause_until(capture_start + 2)
            ctl('dispatch', 'workspace', '2')
            spawn(app, '__window-test', '--color', '#2266cc')
            pause_until(capture_start + 6)
        metadata = stop_record(bundle)
        if args.mode == 'host-plugin':
            rects = [(l['x'], l['y'], l['w'], l['h']) for level in report['during_layers'][recorded]['levels'].values()
                     for l in level if l['namespace'].startswith('omareel-')]
            samples = inspect_frames(bundle, rects + [(50, 600, 100, 100)])
            assert metadata['overlay_exclusion'] == 'plugin'
            assert np.max(np.abs(samples[:, :-1] - samples[:, -1:])) <= 8
            assert samples[0, -1, 2] < 100 and samples[-1, -1, 2] > 150
        time.sleep(.5)
        report['restored_host'] = json_file(host_path)
        report['restored_layers'] = json.loads(ctl('-j', 'layers'))
        exclusion = 'plugin' if args.mode == 'host-plugin' else 'fallback'
        assert metadata['overlay_exclusion'] == exclusion
        assert json_file(bundle / 'recording.json')['overlay_exclusion'] == exclusion
        assert report['restored_host']['monitor'] == report['standby_host']['monitor']
        assert report['restored_host']['x'] == report['standby_host']['x']
        assert report['restored_host']['y'] == report['standby_host']['y']
        assert 'omareel-selfview' in scopes(report['restored_layers'][report['standby_host']['monitor']])

    elif args.mode == 'recorder-cycles':
        overlay = layer()
        layer_started = time.monotonic()
        time.sleep(.3)
        deadline = time.monotonic() + 270
        cycles = []
        for index in range(200):
            assert time.monotonic() < deadline, 'recorder cycle deadline'
            if time.monotonic() - layer_started > 50:
                stop(overlay)
                stop(green)
                green = spawn(app, '__window-test', '--color', '#228844')
                overlay = layer()
                layer_started = time.monotonic()
                time.sleep(.1)
            assert overlay.poll() is None
            bundle = start_record('--no-webcam', '--no-bar')
            metadata = stop_record(bundle)
            assert metadata['backend'] == 'ext-image-copy-capture' and metadata['overlay_exclusion'] == 'plugin'
            assert metadata['capture']['frames'] > 0
            cycles.append({'bundle': str(bundle), 'frames': metadata['capture']['frames'], 'first_frame_ms': report['first_frame_ms']})
            report['recorder_cycles'] = cycles
            if index % 25 == 0:
                (root / 'report.json').write_text(json.dumps(report, indent=2))
        os.kill(instance['pid'], 0)
        report['completed_recorder_cycles'] = len(cycles)

    elif args.mode == 'stability':
        overlay = layer()
        time.sleep(.3)
        deadline = time.monotonic() + 90
        for index in range(200):
            assert time.monotonic() < deadline, 'capture cycle deadline'
            run('grim', '-o', 'WAYLAND-1', str(root / 'cycle.png'), timeout=5)
            assert int(magenta(np.asarray(Image.open(root / 'cycle.png').convert('RGB'))).sum()) == 0
        report['capture_cycles'] = 200
        ctl('plugin', 'unload', plugin)
        run('grim', '-o', 'WAYLAND-1', str(root / 'unloaded.png'))
        assert int(magenta(np.asarray(Image.open(root / 'unloaded.png').convert('RGB'))).sum()) > 1000
        ctl('plugin', 'load', plugin)
        ctl('output', 'create', 'wayland')
        time.sleep(.3)
        second = layer('omareel-secondary', '--monitor', 'WAYLAND-2')
        time.sleep(.3)
        ctl('output', 'remove', 'WAYLAND-2')
        time.sleep(.3)
        report['final_status'] = json.loads(ctl('omareel-exclude', 'status'))
        os.kill(instance['pid'], 0)
    report['passed'] = True
finally:
    (root / 'report.json').write_text(json.dumps(report, indent=2))
    for child in children:
        stop(child)
    commands.close()
    clients_log.close()
