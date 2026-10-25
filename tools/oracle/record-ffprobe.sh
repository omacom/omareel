#!/usr/bin/env bash
# Writes tests/golden/corpus/mediaprobe/ffprobe/<file>/ for the mediaprobe family: generates a
# handful of small test files with ffmpeg (testsrc video at several sizes and rates, with and
# without audio, rotation metadata, non-square sample aspect, variable frame rate, audio only,
# an image, a text file, an empty file, and a path that does not exist), runs the real ffprobe on
# each with exactly the arguments MediaProbe.cpp passes, and keeps what it printed and its exit
# status. The oracle never runs ffprobe: tests/golden/shims/ffprobe replays these recordings, so
# the goldens do not depend on the machine's FFmpeg. Run it once when the corpus is created or
# when the recordings should be refreshed on purpose; the generated media files are not kept.
#
# Each <file>/ holds probeVideo.{stdout,stderr,status} and probeFrameTimes.{stdout,stderr,status}.
# The path ffprobe was given is written @PATH@ in stdout and stderr (the shim puts the path it is
# given back) and FFmpeg's log-context addresses are written 0x0. FFPROBE_VERSION is the first
# line of `ffprobe -version`.
set -euo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
out="$repo/tests/golden/corpus/mediaprobe/ffprobe"
for tool in ffmpeg ffprobe; do
    command -v "$tool" >/dev/null || { echo "record-ffprobe.sh: $tool is required" >&2; exit 1; }
done
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
media="$tmp/media"
mkdir -p "$media"

ff() { ffmpeg -hide_banner -loglevel error -nostdin -y "$@"; }
x264=(-c:v libx264 -preset ultrafast -pix_fmt yuv420p -bf 0 -g 30)

ff -f lavfi -i testsrc=size=320x240:rate=30:duration=2 "${x264[@]}" "$media/testsrc-320x240-30.mp4"
ff -f lavfi -i testsrc=size=1280x720:rate=60:duration=1 -f lavfi -i sine=frequency=440:duration=1 \
    "${x264[@]}" -c:a aac -shortest "$media/testsrc-1280x720-60-audio.mp4"
ff -f lavfi -i testsrc=size=640x360:rate=24000/1001:duration=1 "${x264[@]}" "$media/testsrc-640x360-ntsc.mkv"
ff -f lavfi -i testsrc=size=641x361:rate=25:duration=1 -c:v libx264 -preset ultrafast -pix_fmt yuv444p \
    "$media/odd-size-641x361.mp4"
ff -f lavfi -i testsrc=size=160x120:rate=480:duration=0.25 "${x264[@]}" "$media/high-rate-480.mp4"
# B-frames: packets come in decode order, so their pts_time are out of order until sorted.
ff -f lavfi -i testsrc=size=320x240:rate=30:duration=1 -c:v libx264 -preset medium -pix_fmt yuv420p \
    -bf 3 -g 12 "$media/bframes.mp4"
ff -display_rotation 90 -i "$media/testsrc-320x240-30.mp4" -c copy "$media/rotated-90.mp4"
ff -display_rotation 180 -i "$media/testsrc-320x240-30.mp4" -c copy "$media/rotated-180.mp4"
ff -display_rotation 270 -i "$media/testsrc-320x240-30.mp4" -c copy "$media/rotated-270.mov"
ff -f lavfi -i testsrc=size=640x480:rate=30:duration=1 -vf setsar=4/3 "${x264[@]}" "$media/sar-4-3.mp4"
ff -f lavfi -i testsrc=size=320x240:rate=30:duration=1 -vf setsar=2/1 "${x264[@]}" "$tmp/sar-2-1.mp4"
ff -display_rotation 90 -i "$tmp/sar-2-1.mp4" -c copy "$media/sar-2-1-rotated-90.mp4"
# A native screen capture only holds a frame where the screen changed: 30 frames over 2.2 s.
ff -f lavfi -i testsrc=size=320x240:rate=30:duration=1 \
    -vf "setpts='(N+N*N/10)/30/TB'" -fps_mode passthrough "${x264[@]}" "$media/vfr.mp4"
ff -f lavfi -i testsrc=size=320x240:rate=30:duration=1 \
    -vf "setpts='(N+N*N/10)/30/TB'" -fps_mode passthrough "${x264[@]}" "$media/vfr.mkv"
ff -f lavfi -i sine=frequency=440:duration=1 -c:a aac "$media/audio-only.m4a"
ff -f lavfi -i testsrc=size=64x48:rate=1:duration=1 -frames:v 1 "$media/image.png"
ff -f lavfi -i testsrc=size=64x48:rate=10:duration=0.5 "$media/animated.gif"
printf 'not a video\n' > "$media/not-media.txt"
: > "$media/empty.mp4"

probe_video=(-v error -show_entries
    'stream=codec_type,width,height,r_frame_rate,avg_frame_rate,duration,sample_aspect_ratio:stream_side_data=rotation:stream_tags=rotate:format=duration'
    -of json)
probe_frames=(-v error -select_streams v:0 -show_entries packet=pts_time -of csv=p=0)

record() { # <name> <kind> <path> <ffprobe arguments...>
    local name=$1 kind=$2 path=$3
    shift 3
    local dir="$out/$name" status=0
    mkdir -p "$dir"
    ffprobe "$@" "$path" > "$tmp/stdout" 2> "$tmp/stderr" || status=$?
    # The path becomes @PATH@ (a literal match, not a pattern), and the context address in
    # FFmpeg's "[mov,mp4,... @ 0x5fe16ca35c00]" log prefixes becomes 0x0, so that rerunning the
    # script with the same FFmpeg reproduces the files.
    for stream in stdout stderr; do
        PATH_TEXT="$path" perl -0777 -pe 's/\Q$ENV{PATH_TEXT}\E/\@PATH\@/g; s/ \@ 0x[0-9a-f]+\]/ \@ 0x0]/g' \
            "$tmp/$stream" > "$dir/$kind.$stream"
    done
    printf '%s\n' "$status" > "$dir/$kind.status"
}

mkdir -p "$out"
find "$out" -mindepth 1 -maxdepth 1 ! -name 'crafted-*' -exec rm -rf {} +
names=()
for file in "$media"/*; do names+=("$(basename "$file")"); done
names+=(vanished.mp4)
for name in "${names[@]}"; do
    record "$name" probeVideo "$media/$name" "${probe_video[@]}"
    record "$name" probeFrameTimes "$media/$name" "${probe_frames[@]}"
done
ffprobe -version | head -n 1 > "$out/FFPROBE_VERSION"
echo "recorded ${#names[@]} files into ${out#"$repo/"}"
