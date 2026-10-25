#!/usr/bin/python3 -I
"""Stands in for gpu-screen-recorder, which cannot capture a nested compositor's output: records a
test pattern of the requested size and rate with ffmpeg into -o, writes the first frame's
CLOCK_MONOTONIC microseconds next to it as gsr's -write-first-frame-ts does, and stops cleanly
on SIGINT (exit 0), or fails on demand (PARITY_GSR_FAIL)."""
import os, signal, subprocess, sys, time

args = sys.argv[1:]
def value(name, default=None):
    return args[args.index(name) + 1] if name in args and args.index(name) + 1 < len(args) else default
if args[:1] == ['--list-capture-options']:
    print(os.environ.get('PARITY_GSR_OPTIONS', 'WAYLAND-1|1600x900\nregion').replace('\\n', '\n'))
    sys.exit(0)
if os.environ.get('PARITY_GSR_FAIL'):
    print('gsr error: failed to capture monitor ' + str(value('-w')), file=sys.stderr, flush=True)
    sys.exit(1)
output = value('-o')
fps = value('-f', '60')
size = '1600x900'
if value('-w') == 'region':
    size = value('-region').split('+')[0]
ffmpeg = subprocess.Popen(['/usr/bin/ffmpeg', '-y', '-v', 'error', '-re', '-f', 'lavfi', '-i',
                           f'testsrc2=size={size}:rate={fps}', '-c:v', 'libx264', '-pix_fmt', 'yuv420p',
                           output], stdin=subprocess.DEVNULL)
time.sleep(0.15)
if value('-write-first-frame-ts') == 'yes':
    with open(output + '.ts', 'w') as file:
        file.write(str(time.monotonic_ns() // 1000) + '\n')
print('fake gsr recording', size, 'at', fps, flush=True)
stop = []
signal.signal(signal.SIGINT, lambda *_: stop.append(1))
while not stop and ffmpeg.poll() is None:
    time.sleep(0.02)
if ffmpeg.poll() is None:
    ffmpeg.send_signal(signal.SIGINT)
sys.exit(0 if ffmpeg.wait() in (0, 255) else 1)
