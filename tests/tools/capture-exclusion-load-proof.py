#!/usr/bin/env python3
"""Nested-only load, status, prefix, unload, and mismatched-header checks."""
import json
import os
from pathlib import Path
import subprocess
import time

assert os.environ.get('OMAREEL_NESTED_TEST'), 'Use the nested harness'
root = Path(os.environ['OMAREEL_TEST_ARTIFACTS'])
repo = Path(__file__).resolve().parents[2]
plugin = repo / 'build/plugin/omareel-capture-exclude.so'
fake = Path(os.environ.get('OMAREEL_FAKE_PLUGIN', '/tmp/omareel-nested/fake-build/omareel-capture-exclude.so'))
instance = json.loads((root / 'instance.json').read_text())
assert os.environ['HYPRLAND_INSTANCE_SIGNATURE'] == instance['HYPRLAND_INSTANCE_SIGNATURE']
transcript = []

def ctl(*args, success=True):
    result = subprocess.run(['hyprctl', *map(str, args)], text=True, capture_output=True, timeout=5)
    transcript.append({'command': ['hyprctl', *map(str, args)], 'code': result.returncode,
                       'stdout': result.stdout, 'stderr': result.stderr})
    (root / 'commands.json').write_text(json.dumps(transcript, indent=2))
    if success:
        assert result.returncode == 0, transcript[-1]
    else:
        assert 'could not be loaded' in result.stdout, transcript[-1]
    return result.stdout

ctl('plugin', 'load', plugin)
plugins = json.loads(ctl('-j', 'plugin', 'list'))
assert any(p['name'] == 'omareel-capture-exclude' for p in plugins)
status = json.loads(ctl('omareel-exclude', 'status'))
version = json.loads(ctl('-j', 'version'))
assert status['built_hash'] == version['commit'] == Path(str(plugin) + '.hash').read_text().strip()
assert 'test-extra-' in json.loads(ctl('omareel-exclude', 'add', 'test-extra-'))['prefixes']
assert 'test-extra-' not in json.loads(ctl('omareel-exclude', 'remove', 'test-extra-'))['prefixes']
ctl('plugin', 'unload', plugin)
assert not json.loads(ctl('-j', 'plugin', 'list'))
failure = ctl('plugin', 'load', fake, success=False)
assert 'commit mismatch' in failure and 'threw' in failure
assert not json.loads(ctl('-j', 'plugin', 'list'))
os.kill(instance['pid'], 0)
with (root / 'launcher.log').open('w') as log:
    launcher = subprocess.Popen([str(repo / 'build/omareel')], stdout=log, stderr=log)
    try:
        for attempt in range(60):
            assert launcher.poll() is None, 'launcher exited'
            if json.loads(ctl('-j', 'plugin', 'list')):
                break
            time.sleep(.05)
        assert json.loads(ctl('omareel-exclude', 'status'))['built_hash'] == version['commit']
        time.sleep(.5)
    finally:
        launcher.terminate()
        try:
            launcher.wait(timeout=3)
        except subprocess.TimeoutExpired:
            launcher.kill()
            launcher.wait(timeout=2)
(root / 'report.json').write_text(json.dumps({'passed': True, 'status': status, 'mismatch_refusal': failure, 'launcher_auto_load': True}, indent=2))
