#!/usr/bin/env bash
# Measures the Slint spike against the Qt build: the preview in a nested Hyprland on a
# 1920x1080@60 headless output, and the export, for the 1080p and 4K bundles, RUNS times each.
#
#   measure.sh <spike binary> <omareel binary> <dir with b1080/ and b2160/> <results dir> [preview|export|uploads|all]
#
# Run from the repository root. Each run writes <app>-<what>-<bundle>-<n>.json with its logs.
# Preview: frame times from the window surface's commits (WAYLAND_DEBUG, frames.py), CPU, RSS
# and GPU over the playback (sample.py), and each app's own count of dropped frames. `uploads`
# runs the spike's preview once per way of getting frames to the GPU.
set -euo pipefail
spike="$1"; qt="$2"; bundles="$3"; out="$4"; what="${5:-all}"
runs="${RUNS:-3}"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"
nested() { timeout 180 tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/slint-measure --timeout 120 -- "$here/in-nested.sh" "$@"; }
quiet() { python3 "$here/quiet.py" | tee -a "$out/quiet.log"; }

spike_preview() {
    local bundle="$1" n="$2" upload="$3" base
    base="$out/slint-$upload-preview-$bundle-$n"
    WAYLAND_DEBUG=client SAMPLE="$base.sample.json" SAMPLE_AFTER=3.2 SAMPLE_SECONDS=8 \
        nested "$spike" preview "$bundles/$bundle" --upload "$upload" --measure 10 --stats "$base.stats.json" > "$base.log" 2>&1 || true
    python3 "$here/frames.py" "$base.log" 9.5 0.3 > "$base.frames.json"
    python3 - "$base" <<'PY'
import json, sys
b = sys.argv[1]
app = json.load(open(b + '.stats.json'))
app['dropped'] = app['recording_frames_in_span'] - app['screen_frames_shown']
json.dump({'frames': json.load(open(b + '.frames.json')), 'sample': json.load(open(b + '.sample.json')), 'app': app}, open(b + '.json', 'w'), indent=1)
PY
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
}

export_run() {
    local app="$1" bundle="$2" n="$3" start end
    start=$(date +%s.%N)
    if [[ "$app" == slint ]]; then
        "$spike" export "$bundles/$bundle" -o "$out/slint-$bundle.mp4" --quality social --timing > "$out/slint-export-$bundle-$n.log" 2>&1
    else
        "$qt" export "$bundles/$bundle" -o "$out/qt-$bundle.mp4" --quality social --timing > "$out/qt-export-$bundle-$n.log" 2>&1
    fi
    end=$(date +%s.%N)
    python3 -c "import json; print(json.dumps({'seconds': round($end - $start, 3), 'fps': round(600 / ($end - $start), 1), 'load': open('/proc/loadavg').read().split()[:3]}))" > "$out/$app-export-$bundle-$n.json"
}

for n in $(seq 1 "$runs"); do
    for bundle in b1080 b2160; do
        if [[ "$what" == preview || "$what" == all ]]; then
            quiet; spike_preview "$bundle" "$n" wgpu
            quiet; qt_preview "$bundle" "$n"
        fi
        if [[ "$what" == uploads ]]; then
            for upload in wgpu wgpu-ui cpu; do quiet; spike_preview "$bundle" "$n" "$upload"; done
        fi
        if [[ "$what" == export || "$what" == all ]]; then
            quiet; export_run slint "$bundle" "$n"
            quiet; export_run qt "$bundle" "$n"
        fi
        echo "run $n $bundle done (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
