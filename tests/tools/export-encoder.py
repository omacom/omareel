#!/usr/bin/env python3
"""FFmpeg stand-in for export backpressure tests (selected via a private PATH)."""
import os
import sys
import time

ffmpeg = os.environ["OMAREEL_TEST_REAL_FFMPEG"]
# The decoder also invokes ffmpeg. Only intercept the raw frames sent on stdin.
if not any(a == "-i" and b == "-" for a, b in zip(sys.argv[1:], sys.argv[2:])):
    os.execv(ffmpeg, [ffmpeg, *sys.argv[1:]])

mode = os.environ["OMAREEL_TEST_ENCODER_MODE"]
if mode == "stalled":
    time.sleep(60)
elif mode == "finalizing":
    while sys.stdin.buffer.read(65536):
        pass
    time.sleep(60)
elif mode == "failed":
    sys.exit(7)
elif mode == "slow":
    os.execv(ffmpeg, [ffmpeg, "-readrate", os.environ.get("OMAREEL_TEST_READRATE", "0.5"), *sys.argv[1:]])
else:
    sys.exit("Unknown test encoder mode: " + mode)
