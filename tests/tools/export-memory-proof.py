#!/usr/bin/env python3
"""Run inside omadev: bounded-memory proof with real, deliberately slow 4K encoding.

Usage: omadev run N python3 tests/tools/export-memory-proof.py /path/to/omareel
Creates disposable synthetic media and retains logs, samples, and MP4s in /tmp.
Stops its own process group if memory exceeds the safety limits or export stalls.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time


def memory(pid):
    try:
        status = Path(f"/proc/{pid}/status").read_text()
        fields = {line.split(":", 1)[0]: line.split(":", 1)[1].strip() for line in status.splitlines()}
        return {key: int(fields.get(key, "0 kB").split()[0]) for key in ("VmRSS", "VmSwap")}
    except FileNotFoundError:
        return {"VmRSS": 0, "VmSwap": 0}


def tree_memory(pid):
    own = memory(pid)
    try:
        children = Path(f"/proc/{pid}/task/{pid}/children").read_text().split()
    except FileNotFoundError:
        children = []
    # Qt starts child processes on worker threads, so inspect every task.
    for task in Path(f"/proc/{pid}/task").glob("*/children"):
        try:
            children += task.read_text().split()
        except FileNotFoundError:
            pass
    return sum(own.values()) + sum(tree_memory(int(child)) for child in set(children))


def main():
    if not os.environ.get("OMADEV_SLOT"):
        sys.exit("Run this proof through omadev run; do not test on the host desktop.")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--durations", nargs="+", type=int, default=[5, 20])
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    ffmpeg = shutil.which("ffmpeg")
    ffprobe = shutil.which("ffprobe")
    if not ffmpeg or not ffprobe:
        sys.exit("ffmpeg and ffprobe are required")
    root = Path(tempfile.mkdtemp(prefix="omareel-memory-proof-"))
    print(f"Artifacts: {root}", flush=True)
    bundle = root / "synthetic.omareel"
    bundle.mkdir()
    (bundle / "capture.json").write_text(json.dumps({"fps": 30, "first_frame_us": 0}))
    (bundle / "input.jsonl").write_text("")
    subprocess.run([ffmpeg, "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=3840x2160:rate=30",
                    "-t", str(max(args.durations)), "-c:v", "libx264", "-preset", "ultrafast",
                    "-threads", "2", str(bundle / "screen.mp4")], check=True, timeout=120)
    shimdir = root / "bin"
    shimdir.mkdir()
    shim = shimdir / "ffmpeg"
    shim.write_text("#!/bin/sh\nexec " + shlex.quote(sys.executable) + " "
                    + shlex.quote(str(Path(__file__).resolve().with_name("export-encoder.py"))) + ' "$@"\n')
    shim.chmod(0o700)
    env = dict(os.environ, PATH=str(shimdir) + ":" + os.environ["PATH"],
               OMAREEL_DISABLE_NVENC="1", OMAREEL_TEST_ENCODER_MODE="slow",
               OMAREEL_TEST_REAL_FFMPEG=ffmpeg, OMAREEL_TEST_READRATE="0.5")
    results = []
    for duration in args.durations:
        (bundle / "project.json").write_text(json.dumps({
            "name": "Memory proof", "clips": [{"id": "c1", "in": 0, "out": duration, "speed": 1}],
            "background": {"type": "color", "color": "#202020"},
            "frame": {"padding": 0, "radius": 0, "shadow": {"enabled": False}, "border": {"enabled": False}},
            "cursor": {"visible": False}, "camera": {"enabled": False},
        }))
        output = root / f"out-{duration}s.mp4"
        samples = []
        start = time.monotonic()
        with (root / f"export-{duration}s.log").open("w") as log:
            child = subprocess.Popen([str(binary), "export", str(bundle), "-o", str(output),
                                      "--width", "3840", "--fps", "30", "--quality", "web-low", "--timing"],
                                     env=env, stdout=log, stderr=log, start_new_session=True)
            try:
                while child.poll() is None:
                    own = memory(child.pid)
                    elapsed = time.monotonic() - start
                    total = tree_memory(child.pid)
                    samples.append(dict(seconds=elapsed, rss_kib=own["VmRSS"], swap_kib=own["VmSwap"], tree_kib=total))
                    if sum(own.values()) > 1536 * 1024 or total > 6144 * 1024:
                        raise RuntimeError("Memory safety limit exceeded (1.5 GiB exporter / 6 GiB tree)")
                    if elapsed > duration * 6 + 60:
                        raise RuntimeError("Export safety timeout exceeded")
                    time.sleep(0.1)
                if child.returncode:
                    raise RuntimeError(f"Export failed: see {log.name}")
            finally:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
                (root / f"samples-{duration}s.json").write_text(json.dumps(samples, indent=2))
        probe = json.loads(subprocess.check_output([
            ffprobe, "-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries",
            "stream=codec_name,width,height,nb_read_frames:format=duration", "-of", "json", str(output)], timeout=120))
        stream = probe["streams"][0]
        assert (stream["width"], stream["height"]) == (3840, 2160), stream
        assert int(stream["nb_read_frames"]) == duration * 30, stream
        result = dict(duration=duration, wall_seconds=round(time.monotonic() - start, 2),
                      max_rss_mib=round(max(s["rss_kib"] for s in samples) / 1024, 1),
                      max_swap_mib=round(max(s["swap_kib"] for s in samples) / 1024, 1),
                      max_tree_mib=round(max(s["tree_kib"] for s in samples) / 1024, 1),
                      late_median_rss_mib=round(statistics.median(s["rss_kib"] for s in samples[len(samples)//2:]) / 1024, 1),
                      frames=stream["nb_read_frames"], output=str(output))
        results.append(result)
        print(json.dumps(result), flush=True)
        (root / "results.json").write_text(json.dumps(results, indent=2))
    if len(results) > 1:
        growth = results[-1]["late_median_rss_mib"] - results[0]["late_median_rss_mib"]
        assert growth < 128, f"Longer export grew by {growth} MiB"
    print("PASS: complete 4K outputs, bounded memory, no length-dependent backlog", flush=True)


if __name__ == "__main__":
    main()
