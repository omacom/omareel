#!/usr/bin/env bash
# Measures the Rune spike against the Qt build: the preview in a nested Hyprland on a
# 1920x1080@60 headless output, and the export, for the 1080p and 4K bundles, RUNS times each.
#
#   measure.sh <spike binary> <omareel binary> <dir with b1080/ and b2160/> <results dir> [preview|rune-preview|qt-preview|export|variants|all]
#
# Run from the repository root, with WAYLAND_DISPLAY (and HYPRLAND_INSTANCE_SIGNATURE) set for
# the outer compositor. Each run writes <app>-<what>-<bundle>-<n>.json with its logs.
# Preview: frame times from the window surface's commits (WAYLAND_DEBUG, frames.py), CPU, RSS and
# GPU over the playback (sample.py), and each app's own count of dropped frames. `variants` runs
# the spike's preview once per way of decoding and uploading frames (VARIANTS overrides the list;
# SPIKE_EXPERIMENT passes through to the spike). RUNE_MODE=editor measures the whole Rune editor
# (default: the preview alone); RUNE_FLAGS adds flags to every Rune preview run, as
# `--canvas-height 0` for the editor's own layout (the canvas fills its pane).
# The Qt export needs a display for its GPU path (without one it renders in software).
set -euo pipefail
spike="$1"; qt="$2"; bundles="$3"; out="$4"; what="${5:-all}"
runs="${RUNS:-3}"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"
nested() { timeout 180 tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/rune-measure --timeout 120 -- "$here/in-nested.sh" "$@"; }
quiet() { python3 "$here/quiet.py" | tee -a "$out/quiet.log"; }

# rune_preview <bundle> <n> <label> [spike preview flags...]
rune_preview() {
    local bundle="$1" n="$2" label="$3" base
    shift 3
    base="$out/rune-$label-preview-$bundle-$n"
    WAYLAND_DEBUG=client SAMPLE="$base.sample.json" SAMPLE_AFTER=3.2 SAMPLE_SECONDS=${SAMPLE_SECONDS:-8} \
        nested "$spike" "${RUNE_MODE:-preview}" "$bundles/$bundle" "$@" ${RUNE_FLAGS:-} --measure 10 --stats "$base.stats.json" > "$base.log" 2>&1 || true
    python3 "$here/frames.py" "$base.log" 9.5 0.3 > "$base.frames.json"
    python3 - "$base" <<'PY'
import json, sys
b = sys.argv[1]
app = json.load(open(b + '.stats.json'))
app['dropped'] = app['recording_frames_in_span'] - app['screen_frames_shown']
json.dump({'frames': json.load(open(b + '.frames.json')), 'sample': json.load(open(b + '.sample.json')), 'app': app}, open(b + '.json', 'w'), indent=1)
PY
    grep -v '^\[' "$base.log" > "$base.app.log" || true
    rm -f "$base.log"
}

qt_preview() {
    local bundle="$1" n="$2" base
    base="$out/qt-preview-$bundle-$n"
    WAYLAND_DEBUG=client OMAREEL_PREVIEW_STATS=1 SAMPLE="$base.sample.json" SAMPLE_AFTER=2.7 SAMPLE_SECONDS=8 STOP_AFTER=13.5 \
        nested "$qt" edit "$bundles/$bundle" > "$base.log" 2>&1 || true
    python3 "$here/frames.py" "$base.log" 9.5 0.3 > "$base.frames.json"
    python3 - "$base" <<'PY'
import json, re, sys
b = sys.argv[1]
stats = [dict(re.findall(r'(\w+)=([\d.]+)', l)) for l in open(b + '.log', errors='replace') if l.startswith('preview fps=')]
playing = [s for s in stats if float(s['fps']) > 1][:10]
app = {'seconds': len(playing), 'fps_per_second': [float(s['fps']) for s in playing],
       'average_frame_cost_ms': [float(s['average_frame_cost_ms']) for s in playing],
       'dropped': sum(int(s['drops']) for s in playing)}
json.dump({'frames': json.load(open(b + '.frames.json')), 'sample': json.load(open(b + '.sample.json')), 'app': app}, open(b + '.json', 'w'), indent=1)
PY
    grep -v '^\[' "$base.log" > "$base.app.log" || true
    rm -f "$base.log"
}

export_run() {
    local app="$1" bundle="$2" n="$3" start end
    start=$(date +%s.%N)
    if [[ "$app" == rune ]]; then
        "$spike" export "$bundles/$bundle" -o "$out/rune-$bundle.mp4" --quality social --timing > "$out/rune-export-$bundle-$n.log" 2>&1
    else
        "$qt" export "$bundles/$bundle" -o "$out/qt-$bundle.mp4" --quality social --timing > "$out/qt-export-$bundle-$n.log" 2>&1
    fi
    end=$(date +%s.%N)
    python3 -c "import json; print(json.dumps({'seconds': round($end - $start, 3), 'fps': round(600 / ($end - $start), 1), 'load': open('/proc/loadavg').read().split()[:3]}))" > "$out/$app-export-$bundle-$n.json"
}

variants="${VARIANTS:-vaapi-surface software-surface software-pixels}"
for n in $(seq 1 "$runs"); do
    for bundle in ${BUNDLES:-b1080 b2160}; do
        if [[ "$what" == preview || "$what" == rune-preview || "$what" == all ]]; then
            quiet; rune_preview "$bundle" "$n" vaapi-surface --decode vaapi --upload surface
        fi
        if [[ "$what" == preview || "$what" == qt-preview || "$what" == all ]]; then
            quiet; qt_preview "$bundle" "$n"
        fi
        if [[ "$what" == variants ]]; then
            for v in $variants; do
                # <decode>-<upload>[-<decode height>], e.g. software-surface-1080
                IFS=- read -r decode upload height <<< "$v"
                quiet; rune_preview "$bundle" "$n" "$v" --decode "$decode" --upload "$upload" ${height:+--decode-height "$height"}
            done
        fi
        if [[ "$what" == export || "$what" == all ]]; then
            quiet; export_run rune "$bundle" "$n"
            quiet; export_run qt "$bundle" "$n"
        fi
        echo "run $n $bundle done (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
