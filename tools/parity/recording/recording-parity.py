#!/usr/bin/env python3
"""Recording parity of two omareel binaries, in a nested compositor:

    OMAREEL_NESTED_ROOT=/tmp/ond tests/tools/nested-hyprland.sh --directory /tmp/ond/run --timeout 1400 -- \
        python3 tools/parity/recording/recording-parity.py OLD NEW [--latency N] [--only CASE ...]

(a short root: Hyprland's socket paths must fit in 108 bytes). Every case runs with OLD and then
NEW, with logging shims first on PATH (shim.c, built here) in front of ffmpeg, ffprobe, hyprctl,
pactl, gpu-screen-recorder and the notification and indicator stubs; slurp and
omarchy-capture-region answer by themselves, and fake-gsr.py stands in for gpu-screen-recorder
where the nested output cannot be captured by it. For each case the CLI's exit codes and output,
the state file while recording, the bundle (files, capture.json and recording.json, input.jsonl's
schema, the video's streams and frame grid, the thumbnail), last-error.txt, every program's argv
by caller, and the daemon's journal entries are compared after paths, pids and times are
normalised; report.json in the artifacts directory holds both sides. --latency N then times N
alternating fullscreen starts of each without the shims."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument('old')
parser.add_argument('new')
parser.add_argument('--only', nargs='*')
parser.add_argument('--latency', type=int, default=0)
parser.add_argument('--plugin', help='the capture exclusion plugin both use (default: build/plugin of this checkout)')
parser.add_argument('--extra-output', action='store_true', help='add a headless second output first')
args = parser.parse_args()
assert os.environ.get('OMAREEL_NESTED_TEST'), 'use the nested harness'
here = Path(__file__).resolve().parent
root = Path(os.environ['OMAREEL_TEST_ARTIFACTS'])
runtime = Path(os.environ['XDG_RUNTIME_DIR'])
state_dir = runtime / 'omareel'
state_path = state_dir / 'recording.json'
repo = here.parents[2]
plugin = args.plugin or str(repo / 'build/plugin/omareel-capture-exclude.so')
apps = {'old': str(Path(args.old).resolve()), 'new': str(Path(args.new).resolve())}
report = {'cases': {}, 'diffs': {}}
pulse = 'unix:/run/user/%d/pulse/native' % os.getuid()

# ------------------------------------------------------------------------------------------
# Shims: every program the recorder runs is logged, then run for real (or faked).
shims = root / 'shims'
(shims / 'targets').mkdir(parents=True, exist_ok=True)
base_path = os.environ['PATH']
def real(name):
    found = shutil.which(name, path=base_path)
    assert found, name
    return found
targets = {name: real(name) for name in ('ffmpeg', 'ffprobe', 'hyprctl', 'pactl', 'omarchy-notification-send',
                                          'omarchy-shell', 'gpu-screen-recorder')}
targets['slurp'] = str(here / 'fake-slurp.sh')
targets['omarchy-capture-region'] = str(here / 'fake-capture-region.sh')
subprocess.run(['cc', '-O2', '-o', str(shims / 'shim'), str(here / 'shim.c')], check=True)
for name in targets:
    shutil.copy(shims / 'shim', shims / name)
def set_target(name, target):
    (shims / 'targets' / name).write_text(target + '\n')
for name, target in targets.items():
    set_target(name, target)
(root / 'region-answer').write_text('101,99 641x361\n')

env_base = os.environ.copy()
env_base.update(PATH=f'{shims}:{base_path}', PARITY_DIR=str(root), PULSE_SERVER=pulse, TZ='UTC',
                OMAREEL_PLUGIN_PATH=plugin, LANG='C.UTF-8')
env_base.pop('OMAREEL_CAPTURE', None)

def run(command, env, timeout=40):
    started = time.monotonic()
    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=timeout)
    return {'code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr,
            'seconds': time.monotonic() - started}

def ctl(*command):
    return subprocess.run(['hyprctl', *command], capture_output=True, text=True, timeout=10).stdout

def json_file(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, json.JSONDecodeError):
        return None

def read_log(path):
    if not path.exists():
        return []
    calls = []
    for record in path.read_bytes().decode('utf-8', 'replace').split('\x1e'):
        if not record:
            continue
        fields = record.split('\x1f')
        calls.append({'program': fields[0], 'role': fields[1], 'argv': fields[2:]})
    return calls

def wait_until(condition, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return True
        time.sleep(0.05)
    return False

def omareel_processes():
    found = []
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            argv = (entry / 'cmdline').read_bytes().split(b'\0')
        except OSError:
            continue
        argv = [a.decode('utf-8', 'replace') for a in argv if a]
        if len(argv) > 1 and 'omareel' in argv[0]:
            found.append((int(entry.name), argv))
    return found

def kill_leftovers():
    for pid, argv in omareel_processes():
        if argv[1] in ('edit', '__record-bar', '__record-daemon'):
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

def journal_since(seconds, pid):
    try:
        output = subprocess.run(['journalctl', '--user', '-o', 'json', '--since', '@%d' % int(seconds - 1),
                                 '_PID=%d' % pid], capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.TimeoutExpired):
        return []
    messages = []
    for line in output.splitlines():
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        messages.append({key: entry.get(key) for key in ('MESSAGE', 'PRIORITY', 'QT_CATEGORY', 'SYSLOG_IDENTIFIER')})
    return messages

# ------------------------------------------------------------------------------------------
# Normalising

class Names:
    def __init__(self, side, directory):
        self.pairs = [(apps[side], '<APP>'), (str(directory), '<DIR>'), (str(runtime), '<RUNTIME>'),
                      (str(root), '<ROOT>'), (os.path.dirname(apps[side]), '<APPDIR>')]
    def text(self, text):
        if not isinstance(text, str):
            return text
        for value, name in self.pairs:
            text = text.replace(value, name)
        text = re.sub(r'Recording \d{4}-\d\d-\d\d \d\d-\d\d-\d\d( \d+)?', 'Recording <TIME>', text)
        return text
    def value(self, value):
        if isinstance(value, dict):
            return {key: self.value(item) for key, item in value.items()}
        if isinstance(value, list):
            return [self.value(item) for item in value]
        return self.text(value)

TIME_KEYS = {'started_us', 'stopped_us', 'first_frame_us', 'daemon_started_us', 'countdown_end_us', 'pid',
             'startOffsetUs', 'host_pid'}
def shape(value):
    if isinstance(value, dict):
        return {key: shape(item) for key, item in sorted(value.items())}
    if isinstance(value, list):
        return [shape(item) for item in value[:1]]
    return type(value).__name__

def strip_times(value, approximate=()):
    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            if key in TIME_KEYS:
                result[key] = '<T:%s>' % type(item).__name__
            elif key in approximate:
                result[key] = '<~%s>' % type(item).__name__
            else:
                result[key] = strip_times(item, approximate)
        return result
    return value

def normalise_call(call, names):
    argv = [names.text(a) for a in call['argv']]
    program = call['program']
    if program == 'ffmpeg' and '-itsoffset' in argv:
        index = argv.index('-itsoffset') + 1
        argv[index] = '<OFFSET>'
        if '-t' in argv:
            argv[argv.index('-t') + 1] = '<DURATION>'
    role = call['role']
    if not role.startswith('__') and role not in ('record',):
        role = 'detached'
    if program in ('omarchy-notification-send', 'omarchy-shell'):
        role = 'any'
    if program == 'hyprctl' and argv[1:] == ['-j', 'layers']:
        argv = argv[:1] + ['-j', 'layers']
    return {'program': program, 'role': role, 'argv': argv}

def probe_video(path):
    if not Path(path).exists():
        return None
    streams = json.loads(subprocess.run(
        [targets['ffprobe'], '-v', 'error', '-show_entries',
         'stream=index,codec_type,codec_name,width,height,pix_fmt,r_frame_rate,time_base,nb_frames,sample_rate,channels:format=duration,format_name',
         '-of', 'json', path], capture_output=True, text=True).stdout or '{}')
    packets = subprocess.run([targets['ffprobe'], '-v', 'error', '-select_streams', 'v:0', '-show_entries',
                              'packet=pts_time,duration_time', '-of', 'csv=p=0', path],
                             capture_output=True, text=True).stdout.split()
    times = [float(p.split(',')[0]) for p in packets if p.split(',')[0] not in ('', 'N/A')]
    return {'streams': streams.get('streams'), 'format': streams.get('format'), 'packet_times': times}

def video_summary(info, fps):
    if not info:
        return None
    streams = []
    for stream in info['streams'] or []:
        # nb_frames and r_frame_rate follow the frame timing, which differs run to run.
        streams.append({key: value for key, value in stream.items() if key not in ('nb_frames', 'index', 'r_frame_rate')})
    times = info['packet_times']
    on_grid = all(abs(t * fps - round(t * fps)) < 0.01 for t in times)
    return {'streams': streams, 'format_name': (info['format'] or {}).get('format_name'),
            'frames_on_grid': on_grid, 'starts_at_zero': bool(times) and times[0] == 0.0}

# ------------------------------------------------------------------------------------------
# One recording, start to finish.

def record_case(name, side, start_args, stop_side=None, env_extra=None, stop='--stop', during=None,
                targets_extra=None, no_open=True, wait_editor=False):
    stop_side = stop_side or side
    directory = root / 'runs' / name / side
    shutil.rmtree(directory, ignore_errors=True)
    directory.mkdir(parents=True)
    log = directory / 'calls.log'
    env = dict(env_base, OMAREEL_PARITY_LOG=str(log), **(env_extra or {}))
    for program, target in (targets_extra or {}).items():
        set_target(program, target)
    shutil.rmtree(state_dir, ignore_errors=True)
    names = Names(side, directory / 'rec')
    stop_names = Names(stop_side, directory / 'rec')
    result = {}
    t0 = time.time()
    t0_us = time.monotonic_ns() // 1000
    command = [apps[side], 'record', *start_args, '--dir', str(directory / 'rec')]
    if no_open:
        command.append('--no-open')
    started = run(command, env)
    result['start'] = {key: names.text(value) for key, value in started.items() if key != 'seconds'}
    state = json_file(state_path)
    result['state_at_start'] = names.value(strip_times(state)) if state else None
    daemon_pid = (state or {}).get('pid')
    if during:
        during()
    time.sleep(1.0)
    state_late = json_file(state_path)
    result['state_during'] = names.value(strip_times(state_late, ('bar_width', 'bar_height'))) if state_late else None
    if started['code'] == 0:
        stopped = run([apps[stop_side], 'record', stop], env)
        result['stop'] = {key: stop_names.text(value) for key, value in stopped.items() if key != 'seconds'}
    wait_until(lambda: not state_path.exists(), 5)
    if wait_editor:
        result['editor'] = wait_until(lambda: any(argv[1] == 'edit' for _, argv in omareel_processes()), 5)
        result['editor_argv'] = [names.value(argv) for _, argv in omareel_processes() if argv[1] == 'edit']
    time.sleep(0.5)
    kill_leftovers()
    bundles = sorted((directory / 'rec').glob('*.omareel')) if (directory / 'rec').exists() else []
    result['bundles'] = [names.text(str(b)) for b in bundles]
    if bundles:
        bundle = bundles[0]
        names.pairs.insert(0, (str(bundle), '<BUNDLE>'))
        stop_names.pairs.insert(0, (str(bundle), '<BUNDLE>'))
        result['start'] = {key: names.text(value) for key, value in started.items() if key != 'seconds'}
        if 'stop' in result:
            result['stop'] = {key: stop_names.text(value) for key, value in stopped.items() if key != 'seconds'}
        files = sorted(p.name for p in bundle.iterdir())
        result['files'] = files
        result['empty_files'] = sorted(p.name for p in bundle.iterdir() if p.stat().st_size == 0)
        capture = json_file(bundle / 'capture.json')
        result['capture_shape'] = shape(capture)
        result['capture'] = strip_times(capture, ('frames', 'measuredFps', 'drops')) if capture else None
        result['recording'] = json_file(bundle / 'recording.json')
        lines = (bundle / 'input.jsonl').read_text().splitlines() if (bundle / 'input.jsonl').exists() else []
        kinds = {}
        for line in lines:
            entry = json.loads(line)
            kinds.setdefault(entry['k'], set()).add(tuple(sorted((k, type(v).__name__) for k, v in entry.items())))
        result['input_schema'] = {kind: sorted(map(list, shapes)) for kind, shapes in kinds.items()}
        result['input_count'] = len(lines)
        fps = (capture or {}).get('fps', 60)
        result['video'] = video_summary(probe_video(str(bundle / 'screen.mp4')), fps)
        thumb = subprocess.run([targets['ffprobe'], '-v', 'error', '-show_entries', 'stream=width,height', '-of',
                                'csv=p=0', str(bundle / 'thumb.jpg')], capture_output=True, text=True).stdout.strip()
        result['thumb'] = thumb
        ts = bundle / 'screen.mp4.ts'
        result['ts_header'] = ts.read_text().splitlines()[0] if ts.exists() else None
        result['raw_counts'] = {'frames': ((capture or {}).get('capture') or {}).get('frames'),
                                'packets': len((probe_video(str(bundle / 'screen.mp4')) or {}).get('packet_times', [])),
                                'first_frame_ms': ((capture or {}).get('first_frame_us', 0) - ((state or {}).get('daemon_started_us') or 0)) / 1000,
                                'ttff_ms': ((capture or {}).get('first_frame_us', 0) - t0_us) / 1000}
    last_error = state_dir / 'last-error.txt'
    result['last_error'] = names.text(last_error.read_text()) if last_error.exists() else None
    result['calls'] = [normalise_call(call, names) for call in read_log(log)]
    if daemon_pid:
        result['journal'] = journal_since(t0, daemon_pid)
    for program in (targets_extra or {}):
        set_target(program, targets[program])
    if bundles:
        shutil.rmtree(directory / 'rec', ignore_errors=True)
    return result

def compare(name, old, new, ignore=()):
    diffs = []
    for key in sorted(set(old) | set(new)):
        if key in ignore or key in ('raw_counts', 'input_count'):
            continue
        if old.get(key) != new.get(key):
            diffs.append(key)
    report['cases'][name] = {'old': old, 'new': new}
    report['diffs'][name] = diffs
    status = 'same' if not diffs else 'DIFF ' + ', '.join(diffs)
    counts = (old.get('raw_counts'), new.get('raw_counts'))
    print(f'{name}: {status}  counts old={counts[0]} new={counts[1]}', flush=True)

def call_groups(calls):
    groups = {}
    for call in calls:
        groups.setdefault((call['role'], call['program']), []).append(call['argv'])
    return {'%s %s' % key: value for key, value in groups.items()}

def case(name, *case_args, **kwargs):
    if args.only and name not in args.only:
        return
    results = {}
    for side in ('old', 'new'):
        if kwargs.get('mixed'):
            stop_side = 'new' if side == 'old' else 'old'
        else:
            stop_side = side
        options = {key: value for key, value in kwargs.items() if key != 'mixed'}
        results[side] = record_case(name, side, *case_args, stop_side=stop_side, **options)
        results[side]['calls'] = call_groups(results[side]['calls'])
    compare(name, results['old'], results['new'])

def write_preferences(values):
    path = root / 'config' / 'omareel' / 'settings.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    if values is None:
        path.unlink(missing_ok=True)
    else:
        path.write_text(json.dumps(values))

def fake_camera():
    # What the bar's camera would leave in the bundle: a short video and its first frame's time.
    state = json_file(state_path) or {}
    bundle = Path(state.get('bundle', '/nonexistent'))
    if not bundle.is_dir():
        return
    subprocess.run([targets['ffmpeg'], '-y', '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=320x180:rate=30',
                    '-t', '1', '-c:v', 'libx264', str(bundle / 'camera.mp4')], timeout=30)
    (bundle / 'camera.mp4.ts').write_text('monotonic_microsec\trealtime_microsec\n%d\t%d\n'
                                          % (time.monotonic_ns() // 1000, time.time_ns() // 1000))

def both_flags_while_recording():
    # A second record command while one runs: --stop with --cancel is a usage error.
    for side in ('old', 'new'):
        result = run([apps[side], 'record', '--stop', '--cancel'], env_base)
        report.setdefault('stop_and_cancel', {})[side] = result['code'], result['stdout'], result['stderr']

def move_cursor():
    for x, y in ((200, 150), (400, 300), (600, 200)):
        ctl('dispatch', 'movecursor', str(x), str(y))
        time.sleep(0.25)

# ------------------------------------------------------------------------------------------
# The commands that need no recording.

def cli_cases():
    commands = [
        ['record', '--stop'], ['record', '--cancel'], ['record', '--fps', '0'], ['record', '--frobnicate'],
        ['record', '--with-webcam', '--no-webcam'], ['record', '--fullscreen', '--region'],
        ['record', '--no-audio', '--with-desktop-audio'], ['record', '--webcam-device', '/tmp/x'],
        ['record', '--webcam-height', '480'], ['record', '--fps'], ['record', '--dir'],
        ['record', '--webcam-device'], ['record', '--microphone-device'],
        ['__selfview-ipc'], ['__selfview-ipc', 'show'], ['__selfview-ipc', 'a', 'b'],
        ['__record-bar-ipc'], ['__record-bar-ipc', 'bogus'], ['__record-bar-ipc', 'rotate'],
        ['__record-bar-ipc', 'flip'], ['__record-bar-ipc', 'selfview', 'show'],
        ['__record-bar-ipc', 'selfview', 'hide'], ['__record-bar-ipc', 'selfview', 'move', '10', 'x'],
        ['__record-bar-ipc', 'selfview', 'move', '10', '20'],
        ['__record-bar-ipc', 'selfview', 'drag-sim', '1', '2', '3', '4'],
        ['__record-bar-ipc', 'selfview', 'drag-sim', '1', '2', '3'],
    ]
    results = {'old': [], 'new': []}
    for side in ('old', 'new'):
        shutil.rmtree(state_dir, ignore_errors=True)
        log = root / f'cli-{side}.log'
        env = dict(env_base, OMAREEL_PARITY_LOG=str(log))
        names = Names(side, root)
        for command in commands:
            result = run([apps[side], *command], env)
            files = {}
            if state_dir.exists():
                for path in sorted(state_dir.iterdir()):
                    if path.suffix == '.json':
                        files[path.name] = strip_times(json_file(path))
            results[side].append({'command': command, 'code': result['code'], 'stdout': names.text(result['stdout']),
                                  'stderr': names.text(result['stderr']), 'files': files})
        results[side].append({'calls': [normalise_call(c, names) for c in read_log(log)]})
    diffs = [index for index, (a, b) in enumerate(zip(results['old'], results['new'])) if a != b]
    report['cases']['cli'] = results
    report['diffs']['cli'] = diffs
    print('cli:', 'same' if not diffs else 'DIFF at ' + str([results['old'][i].get('command') for i in diffs]), flush=True)

# ------------------------------------------------------------------------------------------

def latency(runs):
    measures = {'old': [], 'new': []}
    env = dict(env_base, PATH=base_path)
    for index in range(runs):
        for side in (('old', 'new') if index % 2 == 0 else ('new', 'old')):
            directory = root / 'latency' / side
            shutil.rmtree(directory, ignore_errors=True)
            shutil.rmtree(state_dir, ignore_errors=True)
            t0 = time.monotonic_ns() // 1000
            started = run([apps[side], 'record', '--fullscreen', '--no-audio', '--no-open', '--no-bar',
                           '--dir', str(directory)], env)
            t1 = time.monotonic_ns() // 1000
            state = json_file(state_path) or {}
            time.sleep(0.6)
            run([apps[side], 'record', '--stop'], env)
            bundle = next(directory.glob('*.omareel'), None)
            capture = json_file(bundle / 'capture.json') if bundle else {}
            measures[side].append({'code': started['code'], 'cli_ms': (t1 - t0) / 1000,
                                   'ttff_ms': ((capture or {}).get('first_frame_us', 0) - t0) / 1000,
                                   'daemon_to_frame_ms': ((capture or {}).get('first_frame_us', 0)
                                                          - state.get('daemon_started_us', 0)) / 1000})
            shutil.rmtree(directory, ignore_errors=True)
            time.sleep(0.3)
    summary = {}
    for side, values in measures.items():
        for key in ('cli_ms', 'ttff_ms', 'daemon_to_frame_ms'):
            numbers = sorted(v[key] for v in values)
            summary[f'{side}_{key}'] = {'median': numbers[len(numbers) // 2], 'min': numbers[0], 'max': numbers[-1]}
    report['latency'] = {'runs': measures, 'summary': summary}
    for key, value in summary.items():
        print(f'latency {key}: median {value["median"]:.1f} ms (min {value["min"]:.1f}, max {value["max"]:.1f})', flush=True)

null_sink = None
window = None
try:
    time.sleep(0.5)
    # A fixed 1600x900 output: the nested window floats in the outer compositor, so other windows
    # there cannot retile it between the two runs of a case.
    outer = dict(os.environ, XDG_RUNTIME_DIR=os.environ['OMAREEL_OUTER_RUNTIME'],
                 WAYLAND_DISPLAY=os.environ['OMAREEL_OUTER_DISPLAY'],
                 HYPRLAND_INSTANCE_SIGNATURE=os.environ['OMAREEL_OUTER_SIGNATURE'])
    instance = json.loads((root / 'instance.json').read_text())
    clients = json.loads(subprocess.run(['hyprctl', '-j', 'clients'], env=outer, capture_output=True, text=True).stdout)
    selector = 'address:' + next(c for c in clients if c['pid'] == instance['pid'])['address']
    monitor = json.loads(subprocess.run(['hyprctl', '-j', 'monitors'], env=outer, capture_output=True, text=True).stdout)[0]
    for expression in (f"hl.dispatch(hl.dsp.window.float({{action='enable',window='{selector}'}}))",
                       f"hl.dispatch(hl.dsp.window.resize({{x=1600,y=900,window='{selector}'}}))",
                       f"hl.dispatch(hl.dsp.window.move({{x={monitor['x'] + 60},y={monitor['y'] + 60},window='{selector}'}}))"):
        subprocess.run(['hyprctl', 'eval', expression], env=outer, capture_output=True, timeout=10)
    time.sleep(0.8)
    if args.extra_output:
        ctl('output', 'create', 'headless', 'PARITY-2')
        time.sleep(0.8)
    report['monitors'] = json.loads(ctl('-j', 'monitors'))
    print('nested output', [(m['name'], m['width'], m['height'], m['scale']) for m in report['monitors']], flush=True)
    module = subprocess.run(['pactl', 'load-module', 'module-null-sink', 'sink_name=omareel_parity_mic'],
                            env=dict(os.environ, PULSE_SERVER=pulse), capture_output=True, text=True)
    null_sink = module.stdout.strip() or None
    mic = ['--with-desktop-audio', '--with-microphone-audio', '--microphone-device', 'omareel_parity_mic.monitor']
    if not args.only or 'cli' in args.only:
        cli_cases()
    # Warm-up, thrown away: loads the plugin and fills the encoder probe cache (keyed on the ffmpeg
    # shim), so neither side of the first case pays for them alone.
    record_case('warm-up', 'old', ['--fullscreen', '--no-audio', '--no-bar'])
    # A window to pick, on its own for the whole run.
    window = subprocess.Popen([apps['old'], '__window-test', '--color', '#228844'], stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
    time.sleep(1.0)
    case('fullscreen-nobar', ['--fullscreen', '--no-audio', '--no-bar'], during=move_cursor)
    case('region-nobar-odd', ['--region', '--no-audio', '--no-bar'], during=move_cursor)
    case('window-nobar', ['--window', '--no-audio', '--no-bar'], during=move_cursor)
    case('fullscreen-bar', ['--fullscreen', '--no-audio'], during=move_cursor)
    case('region-bar', ['--region', '--no-audio'])
    case('cancel-nobar', ['--fullscreen', '--no-audio', '--no-bar'], stop='--cancel')
    case('cancel-bar', ['--region', '--no-audio'], stop='--cancel')
    case('audio-native', ['--fullscreen', *mic, '--no-bar'])
    case('audio-native-desktop', ['--region', '--with-desktop-audio', '--no-bar'])
    case('no-plugin-bar', ['--fullscreen', '--no-audio'], env_extra={'OMAREEL_PLUGIN_PATH': '/nonexistent'})
    case('gsr-real', ['--fullscreen', '--no-audio', '--no-bar'], env_extra={'OMAREEL_CAPTURE': 'gsr'})
    fake = {'gpu-screen-recorder': str(here / 'fake-gsr.py')}
    case('gsr-fake-region', ['--region', '--no-audio', '--no-bar'], env_extra={'OMAREEL_CAPTURE': 'gsr'},
         targets_extra=fake, during=move_cursor)
    case('gsr-fake-fullscreen-bar', ['--fullscreen', '--no-audio'], env_extra={'OMAREEL_CAPTURE': 'GSR'},
         targets_extra=fake)
    case('gsr-fake-audio', ['--fullscreen', *mic, '--no-bar'], env_extra={'OMAREEL_CAPTURE': 'gsr'},
         targets_extra=fake)
    case('gsr-fake-cancel', ['--window', '--no-audio', '--no-bar'], env_extra={'OMAREEL_CAPTURE': 'gsr'},
         targets_extra=fake, stop='--cancel')
    case('gsr-fake-fails', ['--fullscreen', '--no-audio', '--no-bar'],
         env_extra={'OMAREEL_CAPTURE': 'gsr', 'PARITY_GSR_FAIL': '1'}, targets_extra=fake)
    case('mixed-stop', ['--fullscreen', '--no-audio', '--no-bar'], mixed=True)
    case('mixed-cancel', ['--region', '--no-audio'], mixed=True, stop='--cancel')
    case('stop-and-cancel-usage', ['--fullscreen', '--no-audio', '--no-bar'], during=both_flags_while_recording)
    case('native-encoder-fails', ['--fullscreen', '--no-audio', '--no-bar'],
         targets_extra={'ffmpeg': str(here / 'ffmpeg-encoder-fails.sh')})
    case('native-falls-back-to-gsr', ['--fullscreen', '--no-audio'],
         targets_extra={'hyprctl': str(here / 'hyprctl-rename.sh'), **fake})
    case('gsr-not-capturable-note', ['--fullscreen', '--no-audio', '--no-bar'],
         env_extra={'OMAREEL_CAPTURE': 'gsr', 'PARITY_GSR_OPTIONS': 'DP-9|1920x1080\\nregion'}, targets_extra=fake)
    case('webcam-missing-device', ['--fullscreen', '--no-audio', '--with-webcam', '--webcam-device', '/dev/video99',
                                   '--webcam-height', '720'], during=fake_camera)
    write_preferences({'countdownBeforeRecording': True, 'systemAudio': False, 'microphone': False})
    case('countdown-nobar', ['--fullscreen', '--no-bar'])
    case('countdown-bar', ['--region'])
    write_preferences({'captureBackend': 'gsr', 'selfViewSize': 'l', 'systemAudio': True, 'microphone': False,
                       'webcam': {'rotation': 90, 'flipHorizontal': True, 'captureHeight': 720,
                                  'device': '/dev/video98'}})
    case('preferences-gsr', ['--window', '--no-bar'], targets_extra=fake)
    write_preferences(None)
    case('open-editor', ['--fullscreen', '--no-audio', '--no-bar'], no_open=False, wait_editor=True)
    if args.latency:
        latency(args.latency)
finally:
    (root / 'report.json').write_text(json.dumps(report, indent=1, default=str))
    kill_leftovers()
    if window:
        window.kill()
    if null_sink:
        subprocess.run(['pactl', 'unload-module', null_sink], env=dict(os.environ, PULSE_SERVER=pulse))
    (root / 'report.json').write_text(json.dumps(report, indent=1, default=str))
