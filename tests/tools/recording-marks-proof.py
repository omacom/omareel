#!/usr/bin/env python3
"""Markers, a pause and a retake in a real recording, in a nested compositor:

    OMAREEL_NESTED_ROOT=~/.cache/o tests/tools/nested-hyprland.sh --directory ~/.cache/o/marks --timeout 240 -- \
        python3 tests/tools/recording-marks-proof.py BUILD/omareel

(a short root: Hyprland's socket paths must fit in 108 bytes, and ~/.cache/omn is already too
long for a 63-character instance signature). Recorded with the gpu-screen-recorder fallback
(OMAREEL_CAPTURE=gsr), which keeps capturing while paused, it skips the frame check; that needs a
real session, as gpu-screen-recorder captures DRM connectors and a nested output is not one.

It records the nested output with an animated window on it, drops a labelled marker, pauses and
resumes, asks for a retake (which cuts back to the resume), drops another marker, pauses again,
drops a marker while paused (it belongs to the pause's start and keeps when it was made in `at`),
resumes, asks for a three-second retake a second later (which counts recorded time only, so it
reaches back past the pause) and stops. Then it checks markers.jsonl, that the video has no
frames while paused (the native capture holds its last frame), that input.jsonl has nothing from
the pauses, and that the editor, opened offscreen, makes a project whose clips skip the pauses
and the retakes and whose markers are the recorded ones. report.json and the editor's
screenshot are left in the artifacts directory."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

assert os.environ.get('OMAREEL_NESTED_TEST'), 'use the nested harness'
app = sys.argv[1]
root = Path(os.environ['OMAREEL_TEST_ARTIFACTS'])
report = {}
children = []


def run(*command, timeout=20, env=None):
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, env=env)
    report.setdefault('commands', []).append({'argv': list(command[1:]), 'code': result.returncode,
                                              'out': result.stdout.strip(), 'err': result.stderr.strip()[-400:]})
    assert result.returncode == 0, (command, result.returncode, result.stdout, result.stderr)
    return result.stdout


def spawn(*command):
    child = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    children.append(child)
    return child


def state():
    try:
        return json.loads((Path(os.environ['XDG_RUNTIME_DIR']) / 'omareel/recording.json').read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def wait_for(predicate, seconds=3):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(.02)
    return False


try:
    spawn(app, '__window-test', '--color', '#228844', '--animate')
    time.sleep(1)
    # Marks need a recording.
    refused = subprocess.run([app, 'record', '--marker'], capture_output=True, text=True, timeout=10)
    report['marker_without_recording'] = [refused.returncode, refused.stderr.strip()]
    assert refused.returncode == 1, refused
    run(app, 'record', '--fullscreen', '--no-audio', '--no-webcam', '--no-open', '--dir', str(root / 'recordings'),
        timeout=30)
    current = state()
    report['state_at_start'] = current
    assert current.get('capture_started'), current
    bundle = Path(current['bundle'])
    time.sleep(1.5)
    run(app, 'record', '--marker', 'good take')
    time.sleep(1)
    print(run(app, 'record', '--pause').strip())
    assert wait_for(lambda: state().get('paused') is True), state()
    report['state_paused'] = state()
    time.sleep(1.5)
    print(run(app, 'record', '--pause').strip())
    assert wait_for(lambda: state().get('paused') is False), state()
    time.sleep(1.5)
    print(run(app, 'record', '--retake').strip())
    time.sleep(1)
    run(app, 'record', '--marker')
    time.sleep(1.5)
    print(run(app, 'record', '--pause').strip())
    assert wait_for(lambda: state().get('paused') is True), state()
    time.sleep(0.5)
    run(app, 'record', '--marker', 'while paused')
    time.sleep(1)
    print(run(app, 'record', '--pause').strip())
    assert wait_for(lambda: state().get('paused') is False), state()
    time.sleep(1)
    retake_message = run(app, 'record', '--retake', '3').strip()
    print(retake_message)
    report['retake_after_pause_message'] = retake_message
    assert retake_message.endswith('the last 3.0 s recorded will be cut'), retake_message
    time.sleep(1.5)
    run(app, 'record', '--stop', timeout=60)

    marks = [json.loads(line) for line in (bundle / 'markers.jsonl').read_text().splitlines()]
    report['marks'] = marks
    names = ['marker', 'pause', 'resume', 'retake', 'marker2', 'pause2', 'paused_marker', 'resume2', 'retake2']
    assert [mark['k'] for mark in marks] == ['marker', 'pause', 'resume', 'retake', 'marker', 'pause', 'marker',
                                             'resume', 'retake'], marks
    capture = json.loads((bundle / 'capture.json').read_text())
    report['capture'] = capture
    first = capture['first_frame_us']
    mark = dict(zip(names, marks))
    seconds = {name: (mark[name]['t'] - first) / 1e6 for name in names}
    report['seconds'] = seconds
    retake_from = (mark['retake']['from'] - first) / 1e6
    assert abs(retake_from - seconds['resume']) < 1e-6, 'the retake cuts back to the resume'
    # Made while paused: at the pause's start, made about half a second later.
    assert mark['paused_marker']['t'] == mark['pause2']['t'], (mark['paused_marker'], mark['pause2'])
    assert 0.3 < (mark['paused_marker']['at'] - mark['paused_marker']['t']) / 1e6 < 1.5, mark['paused_marker']
    # Three recorded seconds: the second since the resume and two before the pause.
    retake2_from = (mark['retake2']['from'] - first) / 1e6
    after_resume = seconds['retake2'] - seconds['resume2']
    report['retake2'] = {'from': retake2_from, 'after_resume': after_resume}
    assert 0.5 < after_resume < 2.5, after_resume
    assert abs((seconds['pause2'] - retake2_from) + after_resume - 3.0) < 1e-5, report['retake2']

    # The video holds its last frame while paused: no frames from just after the pause until the
    # resume, and frames again after it. The fallback keeps capturing; the project cuts it.
    native = capture.get('backend') != 'gsr'
    packets = run('ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries', 'packet=pts_time', '-of',
                  'csv=p=0', str(bundle / 'screen.mp4'), timeout=60)
    times = sorted(float(line.split(',')[0]) for line in packets.split() if line.strip())
    paused_frames = [t for t in times if seconds['pause'] + 0.2 < t < seconds['resume'] - 0.02
                     or seconds['pause2'] + 0.2 < t < seconds['resume2'] - 0.02]
    report['frames'] = {'total': len(times), 'while_paused': paused_frames,
                        'before_pause': sum(t < seconds['pause'] for t in times),
                        'after_resume': sum(t >= seconds['resume'] for t in times)}
    assert not native or not paused_frames, paused_frames
    assert report['frames']['before_pause'] > 30 and report['frames']['after_resume'] > 30, report['frames']

    events = [json.loads(line) for line in (bundle / 'input.jsonl').read_text().splitlines() if line.strip()]
    during = [event for event in events if mark['pause']['t'] <= event['t'] < mark['resume']['t']
              or mark['pause2']['t'] <= event['t'] < mark['resume2']['t']]
    report['input'] = {'events': len(events), 'while_paused': during[:20]}
    assert not during, during[:5]

    # The editor's first project: the pause and the retake cut, the markers placed.
    editor_env = dict(os.environ, QT_QPA_PLATFORM='offscreen', OMAREEL_SCREENSHOT=str(root / 'editor.png'),
                      OMAREEL_SCREENSHOT_SIZE='1400x900')
    run(app, 'edit', str(bundle), timeout=90, env=editor_env)
    project = json.loads((bundle / 'project.json').read_text())
    report['project'] = {key: project.get(key) for key in ('clips', 'markers', 'zooms')}
    clips = [(clip['in'], clip['out']) for clip in project['clips']]
    assert len(clips) == 3, clips
    assert clips[0][0] == 0 and abs(clips[0][1] - seconds['pause']) < 1e-3, clips
    assert abs(clips[1][0] - seconds['retake']) < 1e-3 and abs(clips[1][1] - retake2_from) < 1e-3, clips
    assert abs(clips[2][0] - seconds['retake2']) < 1e-3, clips
    markers = project['markers']
    assert [(marker['label'], marker.get('kind', 'marker')) for marker in markers] == \
        [('good take', 'marker'), ('', 'retake'), ('', 'marker'), ('while paused', 'marker'), ('', 'retake')], markers
    assert abs(markers[0]['time'] - seconds['marker']) < 1e-3 and abs(markers[1]['time'] - seconds['retake']) < 1e-3
    assert abs(markers[3]['time'] - seconds['pause2']) < 1e-3, markers
    report['result'] = 'pass'
    print('recording marks proof passed')
finally:
    for child in children:
        child.terminate()
    (root / 'report.json').write_text(json.dumps(report, indent=2))
