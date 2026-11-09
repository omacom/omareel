#!/usr/bin/env bash
# A realistic recording for startup measurements: 3 minutes of 2560x1440 at 60 fps (one-second
# keyframes, no B-frames, as the recorder writes) with AAC audio, a 720p camera at 30 fps, and
# an input log of 120 Hz pointer motion, clicks every 4-15 s and typing every 8-20 s (about
# 21,800 events). No project.json: it opens as a new recording does after Stop.
#   make-real-bundle.sh <dir> [seconds]
set -euo pipefail
dir=$1 seconds=${2:-180}
mkdir -p "$dir"
python3 - "$dir" "$seconds" <<'PY'
import json, random, sys
random.seed(1)
d, duration = sys.argv[1], int(sys.argv[2])
W, H = 2560, 1440
out = open(f'{d}/input.jsonl', 'w')
t, x, y = 1_000_000, 1280.0, 720.0
end = 1_000_000 + duration * 1_000_000
next_click, next_key = 3_000_000, 5_000_000
while t < end:
    x = min(W - 1, max(0, x + random.uniform(-8, 8)))
    y = min(H - 1, max(0, y + random.uniform(-6, 6)))
    out.write(json.dumps({"t": t, "k": "m", "x": round(x, 3), "y": round(y, 3)}) + "\n")
    if t >= next_click:
        out.write(json.dumps({"t": t, "k": "d", "b": "left", "x": round(x, 3), "y": round(y, 3)}) + "\n")
        out.write(json.dumps({"t": t + 90000, "k": "u", "b": "left", "x": round(x, 3), "y": round(y, 3)}) + "\n")
        next_click = t + random.randint(4, 15) * 1_000_000
    if t >= next_key:
        for i, c in enumerate("hello"):
            out.write(json.dumps({"t": t + i * 80000, "k": "kd", "code": 35 + i, "name": "KEY_" + c.upper(), "mods": []}) + "\n")
            out.write(json.dumps({"t": t + i * 80000 + 40000, "k": "ku", "code": 35 + i}) + "\n")
        next_key = t + random.randint(8, 20) * 1_000_000
    t += 8333
json.dump({"version": 1, "fps": 60, "width": W, "height": H, "region": {"x": 0, "y": 0, "w": W, "h": H}, "scale": 1.0,
           "monitor": "synthetic", "first_frame_us": 1000000, "started_us": 1000000, "stopped_us": end,
           "audio": {"desktop": True, "mic": True}}, open(f'{d}/capture.json', 'w'), indent=1)
PY
ffmpeg -y -v error -f lavfi -i "testsrc2=size=2560x1440:rate=60:duration=$seconds" \
    -f lavfi -i "sine=frequency=440:duration=$seconds:sample_rate=48000" -ac 2 \
    -c:v libx264 -preset veryfast -tune zerolatency -crf 23 -g 60 -bf 0 -pix_fmt yuv420p \
    -c:a aac -b:a 192k "$dir/screen.mp4"
ffmpeg -y -v error -f lavfi -i "testsrc=size=1280x720:rate=30:duration=$seconds" \
    -c:v libx264 -preset veryfast -g 30 -bf 0 -pix_fmt yuv420p "$dir/camera.mp4"
echo "$dir: $(wc -l < "$dir/input.jsonl") input events"
