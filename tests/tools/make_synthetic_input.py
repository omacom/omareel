#!/usr/bin/env python3
"""Replace a bundle's input.jsonl with a deterministic cursor/click path."""

import argparse
import json
import math
from pathlib import Path
import subprocess


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--duration", type=float, default=8.0)
    parser.add_argument("--camera", action="store_true")
    parser.add_argument("--camera-offset", type=float, default=0.2)
    args = parser.parse_args()
    capture = json.loads((args.bundle / "capture.json").read_text())
    first = int(capture["first_frame_us"])
    width = int(capture.get("width", 640))
    height = int(capture.get("height", 360))
    clicks = [1.5, 2.85, 4.8, 6.85]
    events = []
    for i in range(round(args.duration * 120) + 1):
        t = i / 120
        x = width * (0.5 + 0.38 * math.sin(2 * math.pi * t / 4.7))
        y = height * (0.5 + 0.34 * math.sin(2 * math.pi * t / 3.1 + 0.7))
        events.append({"t": first + round(t * 1_000_000), "k": "m", "x": round(x, 3), "y": round(y, 3)})
    for t in clicks:
        x = width * (0.5 + 0.38 * math.sin(2 * math.pi * t / 4.7))
        y = height * (0.5 + 0.34 * math.sin(2 * math.pi * t / 3.1 + 0.7))
        for kind, offset in (("d", 0.0), ("u", 0.12)):
            events.append({"t": first + round((t + offset) * 1_000_000), "k": kind,
                           "b": "left", "x": round(x, 3), "y": round(y, 3)})
    events.sort(key=lambda event: event["t"])
    with (args.bundle / "input.jsonl").open("w") as output:
        for event in events:
            output.write(json.dumps(event, separators=(",", ":")) + "\n")
    if args.camera:
        camera_first = first + round(args.camera_offset * 1_000_000)
        subprocess.run([
            "ffmpeg", "-y", "-v", "error", "-f", "lavfi", "-i",
            "testsrc2=size=320x180:rate=30", "-t", "2", "-c:v", "libx264",
            str(args.bundle / "camera.mp4"),
        ], check=True)
        (args.bundle / "camera.mp4.ts").write_text(
            f"monotonic_microsec\trealtime_microsec\n{camera_first}\t{camera_first}\n"
        )
        capture["camera"] = {
            "device": "synthetic", "width": 320, "height": 180, "fps": 30,
            "first_frame_us": camera_first, "backend": "synthetic",
        }
        (args.bundle / "capture.json").write_text(json.dumps(capture, indent=2) + "\n")


if __name__ == "__main__":
    main()
