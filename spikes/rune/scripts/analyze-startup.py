#!/usr/bin/env python3
"""Reads one run of startup-profile.py: when the first frames came, which threads and child
processes ran before them, and what the main thread spent its time on.

    analyze-startup.py <run dir> [--until S] [--bucket MS] [--focus REGEX] [--json out.json]

Times are seconds from the launch. CPU time is samples / frequency (perf's -F). Gaps in the
main thread's samples are time it spent off the CPU: waiting on a child process, the
compositor, the GPU or the disk.
"""
import argparse
import collections
import json
import os
import re
import subprocess

ap = argparse.ArgumentParser()
ap.add_argument('run')
ap.add_argument('--until', type=float, default=None, help='end of the window (default: 0.3 s after the first frame)')
ap.add_argument('--bucket', type=float, default=25.0)
ap.add_argument('--focus', default=None, help='inclusive costs only of samples whose stack matches')
ap.add_argument('--top', type=int, default=45)
ap.add_argument('--json', default=None)
ap.add_argument('--freq', type=float, default=2999)
args = ap.parse_args()
perf = os.path.expanduser('~/.cache/omareel-spikes/tools/perf')
start = json.load(open(os.path.join(args.run, 'start.json')))

# --- The Wayland log: commits with a buffer.
stamp = re.compile(r'^\[(\d+):(\d+):(\d+\.\d+)\]\s*(?:\{[^}]*\}\s*)?(.*)')
commits, attached, qt_lines = [], False, []
for text in open(os.path.join(args.run, 'wayland.log'), errors='replace'):
    m = stamp.match(text)
    if not m:
        if text.startswith('[') and ']' in text[:16]:
            qt_lines.append(text.rstrip())
        else:
            qt_lines.append(text.rstrip())
        continue
    t = int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3]) - start['utc']
    body = m[4]
    if re.search(r'-> wl_surface[#@]\d+\.attach\(wl_buffer', body):
        attached = True
    elif re.search(r'-> wl_surface[#@]\d+\.commit', body) and attached:
        commits.append(round(t, 4))
        attached = False
first_frame = commits[0] if commits else None
until = args.until if args.until is not None else (first_frame or 1.0) + 0.3

# --- perf samples.
proc = subprocess.Popen([perf, 'script', '-i', os.path.join(args.run, 'perf.data'), '-F', 'comm,pid,tid,time,ip,sym,dso',
                         '--show-task-events', '--no-inline'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
                        errors='replace')
header = re.compile(r'^\s*(\S.*?)\s+(\d+)/(\d+)\s+(\d+\.\d+):\s*(.*)$')
frame_re = re.compile(r'^\s+[0-9a-f]+\s+(.*?)\s+\((.*)\)$')
samples, tasks, thread_names = [], [], {}
current = None


def finish():
    if current is not None:
        samples.append(current)


for line in proc.stdout:
    if not line.strip():
        continue
    h = header.match(line)
    if h and not line.startswith('\t'):
        finish()
        current = None
        comm, pid, tid, t, rest = h[1].strip(), int(h[2]), int(h[3]), float(h[4]) - start['mono'], h[5]
        if rest.startswith('PERF_RECORD'):
            tasks.append((round(t, 4), comm, pid, tid, rest.strip()))
            continue
        thread_names[tid] = comm
        current = {'t': t, 'comm': comm, 'pid': pid, 'tid': tid, 'frames': []}
        continue
    f = frame_re.match(line)
    if f and current is not None:
        sym, dso = f[1], os.path.basename(f[2])
        sym = re.sub(r'\+0x[0-9a-f]+$', '', sym)
        current['frames'].append((sym, dso))
finish()

main_pid = samples[0]['pid'] if samples else None
main_tid = main_pid
period = 1.0 / args.freq
window = [s for s in samples if s['t'] <= until]
print(f'run {args.run}: first frame {first_frame}s; commits {commits[:8]}; window 0..{until:.3f}s; {len(window)} samples')

# --- Threads and processes.
by_thread = collections.Counter()
for s in window:
    by_thread[(s['pid'], s['tid'], s['comm'])] += 1
print('\nCPU ms by thread before the window ends:')
for (pid, tid, comm), c in by_thread.most_common(25):
    tag = 'main' if tid == main_tid else ('child process' if pid != main_pid else '')
    print(f'  {c * period * 1000:7.1f} ms  {comm:<20} {pid}/{tid} {tag}')
children = {}
cpu_by_pid = collections.Counter(s['pid'] for s in samples)
for t, comm, pid, tid, rest in tasks:
    m = re.match(r'PERF_RECORD_FORK\((\d+):(\d+)\):\((\d+):(\d+)\)', rest)
    if m and m[1] == m[2] and int(m[1]) != main_pid:
        children[int(m[1])] = {'fork': t, 'exit': None, 'name': '?', 'parent': int(m[3])}
    m = re.match(r'PERF_RECORD_COMM exec: (.*?):(\d+)/(\d+)', rest)
    if m and int(m[2]) in children:
        children[int(m[2])]['name'] = m[1]
    m = re.match(r'PERF_RECORD_EXIT\((\d+):(\d+)\)', rest)
    if m and m[1] == m[2] and int(m[1]) in children:
        children[int(m[1])]['exit'] = t
print('\nchild processes (fork, exit, CPU ms over the whole run):')
for pid, c in sorted(children.items(), key=lambda kv: kv[1]['fork']):
    if c['fork'] > until + 1:
        continue
    end = f"{c['exit']:7.3f}" if c['exit'] is not None else '   (on)'
    print(f"  {c['fork']:7.3f} {end}  {c['name']:<16} {cpu_by_pid[pid] * period * 1000:7.1f} ms  pid {pid}")


def short(sym):
    sym = re.sub(r'::h[0-9a-f]{16}$', '', sym)
    sym = re.sub(r'<[^<>]*>', '<>', sym)
    sym = re.sub(r'<[^<>]*>', '<>', sym)
    return sym[:120]


# --- Main thread timeline.
mains = [s for s in window if s['tid'] == main_tid]
step = args.bucket / 1000.0
buckets = collections.defaultdict(list)
for s in mains:
    buckets[int(s['t'] / step)].append(s)
app_frame = re.compile(r'omareel|rune|spike|QQml|QQuick|QSG|QV4|QQmlComponent|QJS|wgpu|naga|cosmic|swash|ffmpeg|av_|Editor|Document|load|probe|thumb|wallpaper|image|png|jpeg|font', re.I)


def label(frames, depth=3):
    """The outermost interesting frames under main(), joined."""
    names = [short(sym) for sym, dso in reversed(frames)]
    picked = []
    for name in names:
        if app_frame.search(name) and not re.search(r'^(main|__libc_start|_start|std::rt|core::ops|std::sys|std::panic)', name):
            picked.append(name)
    if not picked:
        leaf = frames[0] if frames else ('?', '?')
        return f'[{leaf[1]}] {short(leaf[0])}'
    # The outer two and the innermost interesting frame.
    keep = picked[:depth - 1] + ([picked[-1]] if len(picked) >= depth else [])
    return ' > '.join(dict.fromkeys(keep))


print(f'\nmain thread, {args.bucket:.0f} ms buckets (busy % of the bucket, then the commonest stacks):')
last = int(until / step)
for b in range(0, last + 1):
    group = buckets.get(b, [])
    busy = len(group) * period / step * 100
    marks = ''.join(' FRAME' for c in commits if b * step <= c < (b + 1) * step)
    if not group:
        print(f'  {b * step:6.3f}  {busy:4.0f}%  (off CPU){marks}')
        continue
    labels = collections.Counter(label(s['frames']) for s in group)
    top = '; '.join(f'{n}× {l}' for l, n in labels.most_common(2))
    print(f'  {b * step:6.3f}  {busy:4.0f}%  {top[:260]}{marks}')

# --- Inclusive costs on the main thread up to the first frame, and on every thread.
for title, chosen in (('main thread up to the first frame', [s for s in mains if s['t'] <= (first_frame or until)]),
                      ('all threads of the app in the window', [s for s in window if s['pid'] == main_pid])):
    if args.focus:
        chosen = [s for s in chosen if any(re.search(args.focus, sym) for sym, _ in s['frames'])]
    inclusive = collections.Counter()
    dsos = collections.Counter()
    for s in chosen:
        for name in {short(sym) for sym, dso in s['frames']}:
            inclusive[name] += 1
        if s['frames']:
            dsos[s['frames'][0][1]] += 1
    print(f'\ninclusive CPU ms, {title} ({len(chosen) * period * 1000:.0f} ms sampled):')
    for name, c in inclusive.most_common(args.top):
        print(f'  {c * period * 1000:7.1f}  {name}')
    print('  self time by library: ' + ', '.join(f'{d} {c * period * 1000:.0f}' for d, c in dsos.most_common(10)))

# --- The main thread's time up to the first frame, by kind of work (the first kind whose
# pattern matches anywhere in the stack wins).
CATEGORIES = [
    ('GPU and EGL setup', r'QWaylandEglClientBufferIntegration|eglInitialize|eglCreate|libgallium|libEGL|vkCreate|vkEnumerate|libvulkan|request_adapter|request_device|create_surface'),
    ('icon theme lookup', r'QIconLoader'),
    ('SVG icons drawn', r'QSvg|resvg|usvg|tiny_skia'),
    ('image decode', r'libwebp|libpng|libjpeg|QImageReader|image::|png::|jpeg_decoder|zune|libjxl'),
    ('fonts', r'\bFc[A-Z]|fontconfig|libfreetype|cosmic_text|swash|fontdb|ThemeFonts|QFontDatabase|harfbuzz|libharfbuzz'),
    ('QML compile and type loading', r'QV4::Compiler|QQmlTypeLoader|ExecutableCompilationUnit|QQmlScriptBlob|QQmlTypeData|QQmlImportDatabase|QQmlImports'),
    ('QML objects, bindings and layout', r'QQmlObjectCreator|QQmlBinding|QQmlComponent|QQuick|QQmlEngine|QJSEngine'),
    ('recording and project load', r'load_bundle|Document|qjson|input_log|InputLog|cursor_path|CursorSample|zoom_timeline|KeystrokeTrack|Project|parse_value|media_probe|probe'),
    ('rune build, layout and draw', r'rune::|rune_|wgpu|naga'),
    ('dynamic linking', r'_dl_|ld-linux|dlopen'),
    ('Wayland', r'wl_display|libwayland|xkb'),
]
upto = first_frame or until
chosen = [s for s in mains if s['t'] <= upto]
kinds = collections.Counter()
for s in chosen:
    stack = ' '.join(f'{sym} {dso}' for sym, dso in s['frames'])
    kinds[next((name for name, pattern in CATEGORIES if re.search(pattern, stack)), 'other')] += 1
on_cpu = len(chosen) * period
print(f'\nmain thread up to the first frame ({upto * 1000:.0f} ms), by kind:')
breakdown = {}
for name, c in kinds.most_common():
    breakdown[name] = round(c * period * 1000, 1)
    print(f'  {c * period * 1000:7.1f} ms  {name}')
breakdown['off the CPU (waiting)'] = round((upto - on_cpu) * 1000, 1)
print(f'  {(upto - on_cpu) * 1000:7.1f} ms  off the CPU (waiting)')

if args.json:
    json.dump({'first_frame': first_frame, 'commits': commits, 'breakdown': breakdown,
               'children': [dict(c, pid=p) for p, c in children.items()]}, open(args.json, 'w'), indent=1)
