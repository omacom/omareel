#!/bin/bash
# Offscreen export throughput, the spike against `omareel export` of the same bundle with the
# same settings (quality social; both use omareel_core's ExportJob and so the same ffmpeg
# arguments). export.sh ROUNDS SCRATCH_DIR GPUI_BIN QT_BIN
# Prints one line per run: name, load average, seconds, frames/s.
set -euo pipefail
rounds=$1; scratch=$2; gpui=$3; qt=$4
mkdir -p "$scratch/out"
for round in $(seq "$rounds"); do
    for size in 1080 2160; do
        for app in qt gpui; do
            out="$scratch/out/$app-$size.mp4"
            load=$(cut -d' ' -f1 /proc/loadavg)
            start=$(date +%s.%N)
            if [ $app = qt ]; then
                "$qt" export "$scratch/bundle-$size" -o "$out" --quality social > /dev/null 2>&1
            else
                "$gpui" export "$scratch/bundle-$size" -o "$out" --quality social > /dev/null 2>&1
            fi
            end=$(date +%s.%N)
            frames=$(ffprobe -v error -count_packets -select_streams v -show_entries stream=nb_read_packets -of csv=p=0 "$out")
            python3 -c "print(f'$app-$size round $round load $load: {$end-$start:.2f} s, {$frames/($end-$start):.1f} frames/s ($frames frames)')"
        done
    done
done
