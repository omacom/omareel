#!/usr/bin/env bash
# Makes the spikes' test bundle: make-bundle.sh <out-dir> [1080|2160]
set -euo pipefail
out="$1"; height="${2:-1080}"; width=$((height * 16 / 9))
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$out"
ffmpeg -v error -y -f lavfi -i "testsrc2=size=${width}x${height}:rate=60" -t 10 -pix_fmt yuv420p "$out/screen.mp4"
ffmpeg -v error -y -f lavfi -i "testsrc=size=640x480:rate=30" -t 10 -pix_fmt yuv420p "$out/camera.mp4"
cp "$root/tests/fixtures/synthetic/input.jsonl" "$root/tests/fixtures/synthetic/capture.json" "$out/"
echo "bundle in $out (screen ${width}x${height})"
