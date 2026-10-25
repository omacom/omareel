#!/usr/bin/env bash
# Measures the iced spike against the Qt build: the preview (in a nested Hyprland on a
# 1920x1080@60 headless output) and the export, for the 1080p and 4K bundles, RUNS times each.
#
# measure.sh <spike binary> <omareel binary> <dir with b1080/ and b2160/> <results dir> [preview|export|all]
#
# Run from the repository root. Per run it writes <app>-<what>-<bundle>-<n>.json (and the logs).
# Preview: frame times from the window surface's commits (WAYLAND_DEBUG, scripts/frames.py),
# CPU/RSS/GPU over the 10 s of playback (scripts/sample.py), dropped frames as each app counts
# them (the spike: recording frames never shown; omareel: OMAREEL_PREVIEW_STATS's drops).
set -euo pipefail
spike="$1"; qt="$2"; bundles="$3"; out="$4"; what="${5:-all}"
runs="${RUNS:-3}"
apps="${APPS:-qt iced}"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"
nested() { timeout 180 tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/iced-measure --timeout 120 -- "$here/in-nested.sh" "$@"; }

preview() {
    local bundle="$1" n="$2" base
    base="$out/iced-preview-$bundle-$n"
    if [[ " $apps " == *" iced "* ]]; then
    WAYLAND_DEBUG=client SAMPLE="$base.sample.json" SAMPLE_AFTER=3.2 SAMPLE_SECONDS=9.5 \
        nested "$spike" preview "$bundles/$bundle" --measure 10 --canvas-height 620 --stats "$base.stats.json" > "$base.log" 2>&1 || true
    python3 "$here/frames.py" "$base.log" 9.5 3.2 > "$base.frames.json"
    python3 - "$base" <<'EOF'
import json, sys
base = sys.argv[1]
r = {'frames': json.load(open(base + '.frames.json')), 'sample': json.load(open(base + '.sample.json')), 'app': json.load(open(base + '.stats.json'))}
json.dump(r, open(base + '.json', 'w'), indent=1)
EOF
    fi
    base="$out/qt-preview-$bundle-$n"
    [[ " $apps " == *" qt "* ]] || return 0
    WAYLAND_DEBUG=client OMAREEL_PREVIEW_STATS=1 SAMPLE="$base.sample.json" SAMPLE_AFTER=2.7 SAMPLE_SECONDS=9.5 STOP_AFTER=13.5 \
        nested "$qt" edit "$bundles/$bundle" > "$base.log" 2>&1 || true
    python3 "$here/frames.py" "$base.log" 9.5 0.5 > "$base.frames.json"
    python3 - "$base" <<'EOF'
import json, re, sys
base = sys.argv[1]
lines = [l for l in open(base + '.log', errors='replace') if l.startswith('preview fps=')]
stats = [dict(re.findall(r'(\w+)=([\d.]+)', l)) for l in lines]
# One line a second while playing; the first ten seconds of playback.
playing = [s for s in stats if float(s['fps']) > 1][:10]
app = {'seconds': len(playing), 'fps_per_second': [float(s['fps']) for s in playing],
       'dropped': sum(int(s['drops']) for s in playing)}
r = {'frames': json.load(open(base + '.frames.json')), 'sample': json.load(open(base + '.sample.json')), 'app': app}
json.dump(r, open(base + '.json', 'w'), indent=1)
EOF
}

export_run() {
    local bundle="$1" n="$2" start end
    if [[ " $apps " == *" iced "* ]]; then
    start=$(date +%s.%N)
    "$spike" export "$bundles/$bundle" -o "$out/iced-$bundle.mp4" --quality social --timing > "$out/iced-export-$bundle-$n.log" 2>&1
    end=$(date +%s.%N)
    python3 -c "import json,sys; print(json.dumps({'seconds': round($end - $start, 3), 'fps': round(600 / ($end - $start), 1), 'load': open('/proc/loadavg').read().split()[:3]}))" > "$out/iced-export-$bundle-$n.json"
    fi
    [[ " $apps " == *" qt "* ]] || return 0
    start=$(date +%s.%N)
    "$qt" export "$bundles/$bundle" -o "$out/qt-$bundle.mp4" --quality social --timing > "$out/qt-export-$bundle-$n.log" 2>&1
    end=$(date +%s.%N)
    python3 -c "import json,sys; print(json.dumps({'seconds': round($end - $start, 3), 'fps': round(600 / ($end - $start), 1), 'load': open('/proc/loadavg').read().split()[:3]}))" > "$out/qt-export-$bundle-$n.json"
}

for n in $(seq 1 "$runs"); do
    for bundle in b1080 b2160; do
        # Other agents share this machine: start each run once it is idle.
        if [[ "$what" == preview || "$what" == all ]]; then python3 "$here/quiet.py" | tee -a "$out/quiet.log"; preview "$bundle" "$n"; fi
        if [[ "$what" == export || "$what" == all ]]; then python3 "$here/quiet.py" | tee -a "$out/quiet.log"; export_run "$bundle" "$n"; fi
        echo "run $n $bundle done (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
