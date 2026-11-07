#!/usr/bin/env python3
"""Launch and responsiveness of one app, measured the same way for Qt and Rune. Runs inside
tests/tools/nested-hyprland.sh after in-nested.sh's output setup (`in-nested.sh launch.py ...`).

launch.py <out.json> <runs> <play x,y> <seek y,x1,x2> -- <command...>

Each run starts the command with WAYLAND_DEBUG=client and screenshots the SPIKE output with grim
(a quarter-scale PPM, about every 15 ms) until the preview shows the video, then:
- window_s: launch until the app commits its first frame with a buffer (wl_surface.attach of a
  wl_buffer, then commit, in its WAYLAND_DEBUG log);
- video_s: launch until the probe region (the middle of the preview, inside the canvas in both
  apps) is mostly the test pattern's saturated colours;
- idle: 3 s after the video shows, CPU % and RSS of the app (and its children) over 5 s with the
  app paused, its window's commits over that time, and GPU render-engine busy % (sample.py);
- click_ms: a click on Play while paused, through spike-pointer, from the app's receipt of the
  button press (wl_pointer.button in its WAYLAND_DEBUG log) to its next wl_surface.commit, the
  first frame that answers it. Three clicks per run (each followed by a pause click that is not
  counted: during playback the next commit comes anyway).
- play_ms: from the app's receipt of the button release on Play (buttons act on release) to the
  first screenshot whose probe region differs from the paused frame: how long Play takes to show
  motion (decoders starting included).
- seek_ms: clicks on the timeline ruler, alternately at x1 and x2 (paused), from the app's
  receipt of the press (both seek on press) to the first screenshot whose probe region differs:
  how long a seek takes to show its frame.
Screenshots come every 15 to 25 ms, which bounds the resolution of both; the button times come
from the app's WAYLAND_DEBUG log, so spike-pointer's own pauses are not counted.
The first run is reported apart as `first`: it is the least warm (page cache, shader caches).
"""
import json
import os
import re
import statistics
import subprocess
import sys
import tempfile
import time

here = os.path.dirname(os.path.abspath(__file__))
out, runs = sys.argv[1], int(sys.argv[2])
play_x, play_y = sys.argv[3].split(',')
seek_y, seek_x1, seek_x2 = sys.argv[4].split(',')
command = sys.argv[sys.argv.index('--') + 1:]
pointer = os.environ.get('SPIKE_POINTER', os.path.join(os.path.dirname(command[0]), 'spike-pointer'))
shots = tempfile.mkdtemp(prefix='launch-', dir=os.environ.get('XDG_RUNTIME_DIR', '/tmp'))


def grab(path):
    subprocess.run(['grim', '-o', 'SPIKE', '-s', '0.25', '-t', 'ppm', path], check=False,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def ppm(path):
    """(width, height, bytes) of a binary PPM, or None."""
    try:
        data = open(path, 'rb').read()
    except OSError:
        return None
    parts = data.split(maxsplit=4)
    if len(parts) < 5 or parts[0] != b'P6':
        return None
    w, h = int(parts[1]), int(parts[2])
    return w, h, parts[4][: w * h * 3]


def saturated_share(image):
    """Share of the probe region (x 500-1100, y 300-600 of 1920x1080) that is strongly
    saturated, as the test pattern's bars are."""
    w, h, px = image
    x0, x1, y0, y1 = w * 500 // 1920, w * 1100 // 1920, h * 300 // 1080, h * 600 // 1080
    hits = total = 0
    for y in range(y0, y1, 2):
        row = y * w * 3
        for x in range(x0, x1, 2):
            r, g, b = px[row + 3 * x], px[row + 3 * x + 1], px[row + 3 * x + 2]
            total += 1
            if max(r, g, b) > 180 and max(r, g, b) - min(r, g, b) > 150:
                hits += 1
    return hits / max(1, total)


def probe_difference(a, b):
    """Mean absolute difference (0-255) over the probe region of two same-size images."""
    w, h, pa = a
    pb = b[2]
    x0, x1, y0, y1 = w * 500 // 1920, w * 1100 // 1920, h * 300 // 1080, h * 600 // 1080
    total = count = 0
    for y in range(y0, y1, 2):
        row = y * w * 3
        for i in range(row + 3 * x0, row + 3 * x1, 6):
            total += abs(pa[i] - pb[i]) + abs(pa[i + 1] - pb[i + 1]) + abs(pa[i + 2] - pb[i + 2])
            count += 3
    return total / max(1, count)


def until_change(x, y, limit=4.0):
    """Clicks at (x, y); returns (UTC seconds when the click began, UTC seconds of the first
    screenshot whose probe region changed, or None)."""
    before = os.path.join(shots, 'before.ppm')
    grab(before)
    reference = ppm(before)
    clicked, clicked_utc = time.monotonic(), now_utc()
    pointer_process = subprocess.Popen([pointer, x, y], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    after = os.path.join(shots, 'after.ppm')
    changed = None
    while time.monotonic() - clicked < limit:
        taken = now_utc()
        grab(after)
        image = ppm(after)
        if image and reference and len(image[2]) == len(reference[2]) and probe_difference(reference, image) > 0.05:
            changed = taken
            break
    pointer_process.wait()
    return clicked_utc, changed


def buttons(log):
    """(UTC seconds, pressed) of each wl_pointer.button event the app received."""
    result = []
    for text in open(log, errors='replace'):
        m = line.match(text)
        if m and 'wl_pointer' in m.group(2) and '.button(' in m.group(2) and '->' not in m.group(2):
            result.append((utc_seconds(m.group(1)), m.group(2).rstrip().endswith(', 1)')))
    return result


def delay(events, began, changed, pressed):
    """ms from the first press (or the release after it) the app got after `began` to `changed`."""
    if changed is None:
        return None
    press = next((i for i, (t, p) in enumerate(events) if t >= began and p), None)
    if press is None:
        return None
    if pressed:
        start = events[press][0]
    else:
        start = next((t for t, p in events[press + 1:] if not p), None)
        if start is None:
            return None
    return round((changed - start) * 1000, 1)


def utc_seconds(stamp):
    h, m, s = stamp.split(':')
    return int(h) * 3600 + int(m) * 60 + float(s)


def now_utc():
    t = time.time()
    return t % 86400


line = re.compile(r'^\[(\d+:\d+:\d+\.\d+)\]\s*(?:\{[^}]*\}\s*)?(.*)')


def first_frame(log):
    """UTC seconds of day of the first commit after an attach of a buffer."""
    attached = False
    for text in open(log, errors='replace'):
        m = line.match(text)
        if not m:
            continue
        if re.search(r'-> wl_surface[#@]\d+\.attach\(wl_buffer', m.group(2)):
            attached = True
        elif attached and re.search(r'-> wl_surface[#@]\d+\.commit', m.group(2)):
            return utc_seconds(m.group(1))
    return None


def click_latencies(log, after):
    """For each wl_pointer.button press after `after` (UTC seconds of day), the delay to the
    next commit of the app's busiest surface, in ms."""
    events = []
    for text in open(log, errors='replace'):
        m = line.match(text)
        if m:
            events.append((utc_seconds(m.group(1)), m.group(2)))
    surfaces = {}
    for t, e in events:
        s = re.search(r'-> (wl_surface[#@]\d+)\.commit', e)
        if s:
            surfaces[s.group(1)] = surfaces.get(s.group(1), 0) + 1
    if not surfaces:
        return []
    main = max(surfaces, key=surfaces.get)
    result = []
    for i, (t, e) in enumerate(events):
        if t >= after and 'wl_pointer' in e and '.button(' in e and e.rstrip().endswith(', 1)'):
            for t2, e2 in events[i + 1:]:
                if f'-> {main}.commit' in e2:
                    result.append(round((t2 - t) * 1000, 1))
                    break
    return result


def tree_cpu(pid):
    """utime+stime seconds of pid and its descendants."""
    tick = os.sysconf('SC_CLK_TCK')
    total, stack = 0.0, [pid]
    while stack:
        p = stack.pop()
        try:
            fields = open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()
            total += (int(fields[11]) + int(fields[12])) / tick
            for task in os.listdir(f'/proc/{p}/task'):
                stack += [int(c) for c in open(f'/proc/{p}/task/{task}/children').read().split()]
        except OSError:
            pass
    return total


results = []
for n in range(runs):
    log = os.path.join(shots, f'run{n}.log')
    env = dict(os.environ, WAYLAND_DEBUG='client')
    started = time.monotonic()
    started_utc = now_utc()
    process = subprocess.Popen(command, env=env, stdout=open(log + '.out', 'w'), stderr=open(log, 'w'))
    window_s = video_s = None
    k = 0
    while time.monotonic() - started < 15:
        path = os.path.join(shots, f'{n}-{k}.ppm')
        taken = time.monotonic() - started
        grab(path)
        image = ppm(path)
        k += 1
        if image is None:
            continue
        if saturated_share(image) > 0.5:
            video_s = taken
            break
    for f in os.listdir(shots):
        if f.startswith(f'{n}-'):
            os.remove(os.path.join(shots, f))
    time.sleep(3)
    # Idle: the app paused with its preview showing.
    commits_before = sum(1 for t in open(log, errors='replace') if '.commit()' in t and '->' in t)
    sample = os.path.join(shots, f'sample{n}.json')
    cpu0, t0 = tree_cpu(process.pid), time.monotonic()
    subprocess.run([sys.executable, os.path.join(here, 'sample.py'), str(process.pid), '5', sample],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    cpu1, t1 = tree_cpu(process.pid), time.monotonic()
    commits_after = sum(1 for t in open(log, errors='replace') if '.commit()' in t and '->' in t)
    idle = json.load(open(sample)) if os.path.exists(sample) else {}
    # The nested output draws the cursor into screenshots: park it away from the probe region
    # (the targets below are outside it too).
    subprocess.run([pointer, '1910', '20', 'move'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.3)
    # Seeks while paused, then Play (timed) and Pause (not timed), three times.
    seeks = []
    for i in range(4):
        seeks.append(until_change(seek_x1 if i % 2 == 0 else seek_x2, seek_y))
        time.sleep(1.0)
    # Back to the early position, so three rounds of play never reach the end of the clip.
    subprocess.run([pointer, seek_x1, seek_y], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.0)
    click_from = now_utc()
    plays = []
    for _ in range(3):
        plays.append(until_change(play_x, play_y))
        time.sleep(0.8)
        subprocess.run([pointer, play_x, play_y], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1.0)
    process.terminate()
    try:
        process.wait(5)
    except subprocess.TimeoutExpired:
        process.kill()
    clicks = click_latencies(log, click_from)[0::2]
    events = buttons(log)
    seeks = [delay(events, began, changed, True) for began, changed in seeks]
    plays = [delay(events, began, changed, False) for began, changed in plays]
    first = first_frame(log)
    window_s = first - started_utc if first is not None else None
    results.append({
        'window_s': round(window_s, 3) if window_s is not None else None,
        'video_s': round(video_s, 3) if video_s is not None else None,
        'idle_cpu_percent': round((cpu1 - cpu0) / (t1 - t0) * 100, 2),
        'idle_rss_mb': idle.get('rss_peak_mb'),
        'idle_tree_rss_mb': idle.get('tree_rss_peak_mb'),
        'idle_gpu_render_percent': (idle.get('gpu_busy_percent') or {}).get('render'),
        'idle_commits_per_s': round((commits_after - commits_before) / (t1 - t0), 2),
        'click_ms': clicks,
        'play_ms': plays,
        'seek_ms': seeks,
    })
    os.remove(log)
    time.sleep(1)


def median(key):
    values = [r[key] for r in results[1:] if r[key] is not None] or [r[key] for r in results if r[key] is not None]
    return round(statistics.median(values), 3) if values else None


clicks = [c for r in results for c in r['click_ms']]


def pooled(key):
    values = [v for r in results for v in r[key] if v is not None]
    misses = sum(1 for r in results for v in r[key] if v is None)
    return {'median': round(statistics.median(values), 1) if values else None, 'max': max(values) if values else None, 'timeouts': misses}


summary = {
    'command': command,
    'runs': results,
    'first': results[0] if results else None,
    'warm_median': {k: median(k) for k in ('window_s', 'video_s', 'idle_cpu_percent', 'idle_rss_mb', 'idle_tree_rss_mb',
                                           'idle_gpu_render_percent', 'idle_commits_per_s')},
    'click_ms_median': round(statistics.median(clicks), 1) if clicks else None,
    'click_ms_max': max(clicks) if clicks else None,
    'play_ms': pooled('play_ms'),
    'seek_ms': pooled('seek_ms'),
}
json.dump(summary, open(out, 'w'), indent=1)
print(json.dumps(summary['warm_median']), summary['click_ms_median'], summary['play_ms'], summary['seek_ms'])
