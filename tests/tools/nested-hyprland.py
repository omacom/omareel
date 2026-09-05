#!/usr/bin/env python3
"""Run a bounded test body in an isolated nested compositor; clean up on failure."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('--directory', required=True)
parser.add_argument('--timeout', type=int, default=180)
parser.add_argument('body', nargs=argparse.REMAINDER)
args = parser.parse_args()
# Adopt detached recorder descendants so this harness can also reap them.
if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:
    raise OSError(ctypes.get_errno(), 'could not enable child subreaping')
root = Path(args.directory).resolve()
if not root.is_relative_to(os.environ.get('OMAREEL_NESTED_ROOT', '/tmp/omareel-nested')):
    parser.error('artifacts must be under ' + os.environ.get('OMAREEL_NESTED_ROOT', '/tmp/omareel-nested'))
root.mkdir(parents=True, exist_ok=True)
outer = os.environ.copy()
env = outer.copy()
env.pop('HYPRLAND_INSTANCE_SIGNATURE', None)
env.pop('QT_WAYLAND_DISPLAY', None)
env.pop('DISPLAY', None)
env['QT_QPA_PLATFORM'] = 'wayland'
marker = uuid.uuid4().hex
env['OMAREEL_NESTED_TEST'] = marker
# Absolute outer socket permits a private runtime directory (including app state).
outer_socket = Path(outer['WAYLAND_DISPLAY'])
if not outer_socket.is_absolute():
    outer_socket = Path(outer['XDG_RUNTIME_DIR']) / outer_socket
runtime = Path(os.environ.get('OMAREEL_NESTED_ROOT', '/tmp/omareel-nested')) / ('r' + marker[:5])
runtime.mkdir(mode=0o700, exist_ok=True)
env.update(XDG_RUNTIME_DIR=str(runtime), WAYLAND_DISPLAY=str(outer_socket),
           XDG_CONFIG_HOME=str(root / 'config'), XDG_CACHE_HOME=str(Path(os.environ.get('OMAREEL_NESTED_ROOT', '/tmp/omareel-nested')) / 'cache'),
           OMAREEL_TEST_ARTIFACTS=str(root), OMAREEL_OUTER_RUNTIME=outer['XDG_RUNTIME_DIR'],
           OMAREEL_OUTER_DISPLAY=outer['WAYLAND_DISPLAY'],
           OMAREEL_OUTER_SIGNATURE=outer.get('HYPRLAND_INSTANCE_SIGNATURE', ''))
(root / 'nested.env').unlink(missing_ok=True)
# Keep test notifications and indicator refreshes away from the desktop bus.
stubs = root / 'bin'
stubs.mkdir(exist_ok=True)
for name in ('omarchy-notification-send', 'omarchy-shell'):
    stub = stubs / name
    stub.write_text('#!/bin/sh\nexit 0\n')
    stub.chmod(0o755)
env['PATH'] = str(stubs) + ':' + env['PATH']
env['OMAREEL_DEBUG'] = '0'
conf = root / 'hyprland.conf'
conf.write_text("monitor = ,1600x900,auto," + os.environ.get("OMAREEL_NESTED_SCALE", "1") + """
misc {
    disable_hyprland_logo = true
}
debug {
    disable_logs = false
}
animations {
    enabled = false
}
ecosystem {
    enforce_permissions = false
}
""" + 'exec-once = /usr/bin/env > ' + str(root / 'nested.env') + '\n')

process = None
body = None
nested = None

def marked_pids():
    result = []
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit() or int(entry.name) == os.getpid():
            continue
        try:
            if ('OMAREEL_NESTED_TEST=' + marker).encode() in (entry / 'environ').read_bytes().split(b'\0'):
                result.append(int(entry.name))
        except (OSError, ProcessLookupError):
            pass
    return result

def cleanup():
    if nested and process and process.poll() is None:
        try:
            subprocess.run(['hyprctl', '-i', nested['HYPRLAND_INSTANCE_SIGNATURE'], 'dispatch', 'exit'],
                           env=nested, timeout=3, capture_output=True)
        except (OSError, subprocess.TimeoutExpired):
            pass
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid in marked_pids():
            try:
                os.kill(pid, sig)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 3
        for attempt in range(30):
            if not marked_pids() or time.monotonic() >= deadline:
                break
            time.sleep(.1)
    for child in (body, process):
        if child:
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=2)
    for attempt in range(1000):
        try:
            pid, _ = os.waitpid(-1, os.WNOHANG)
            if pid == 0:
                break
        except ChildProcessError:
            break

def interrupted(signum, frame):
    raise KeyboardInterrupt

signal.signal(signal.SIGTERM, interrupted)
signal.signal(signal.SIGINT, interrupted)
try:
    with (root / 'compositor.log').open('w') as log:
        process = subprocess.Popen(['Hyprland', '-c', str(conf)], env=env,
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    for attempt in range(150):
        if process.poll() is not None:
            raise RuntimeError('nested compositor exited; see compositor.log')
        envfile = root / 'nested.env'
        if envfile.exists():
            discovered = dict(line.split('=', 1) for line in envfile.read_text().splitlines() if '=' in line)
            sig = discovered.get('HYPRLAND_INSTANCE_SIGNATURE', '')
            if sig and sig != outer.get('HYPRLAND_INSTANCE_SIGNATURE') and (runtime / 'hypr' / sig / '.socket.sock').exists():
                socket = Path(discovered.get('WAYLAND_DISPLAY', ''))
                if not socket.is_absolute():
                    socket = runtime / socket
                if socket.resolve() == outer_socket.resolve() or not socket.is_socket() or socket.parent.resolve() != runtime.resolve():
                    raise RuntimeError('Refusing test body: display socket is not owned by the nested runtime')
                nested = env | {key: discovered[key] for key in ('HYPRLAND_INSTANCE_SIGNATURE', 'WAYLAND_DISPLAY')}
                envfile.write_text('\n'.join(f'{key}={nested[key]}' for key in ('HYPRLAND_INSTANCE_SIGNATURE', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR')) + '\n')
                break
        time.sleep(.1)
    if not nested:
        raise RuntimeError('nested instance discovery timed out')
    (root / 'instance.json').write_text(json.dumps({'pid': process.pid, **{key: nested[key] for key in
        ('HYPRLAND_INSTANCE_SIGNATURE', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR')}}, indent=2))
    command = args.body[1:] if args.body[:1] == ['--'] else args.body
    if not command:
        parser.error('a test body is required')
    body = subprocess.Popen(command, env=nested, start_new_session=True)
    deadline = time.monotonic() + args.timeout
    for attempt in range(args.timeout * 10 + 1):
        result = body.poll()
        if result is not None:
            break
        if time.monotonic() >= deadline:
            raise subprocess.TimeoutExpired(command, args.timeout)
        # Reap adopted detached children during long stress tests. Leave the
        # two Popen-owned children to their own wait/poll implementations.
        children_file = Path(f'/proc/self/task/{os.getpid()}/children')
        for child in children_file.read_text().split()[:1000]:
            pid = int(child)
            if pid not in (body.pid, process.pid):
                try:
                    os.waitpid(pid, os.WNOHANG)
                except ChildProcessError:
                    pass
        time.sleep(.1)
    else:
        raise subprocess.TimeoutExpired(command, args.timeout)
    if process.poll() is not None:
        raise RuntimeError('nested compositor died during test')
    raise SystemExit(result)
finally:
    cleanup()
