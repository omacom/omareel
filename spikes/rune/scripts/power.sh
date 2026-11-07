#!/usr/bin/env bash
# NVIDIA only: GPU power, clock and whole-GPU SM % (nvidia-smi dmon) over the playback window
# measure.sh samples, for the Rune and Qt previews of b1080/ and b2160/ and two baselines (the
# idle desktop; the nested compositor with nothing in it). Appends one line per run to
# <out dir>/summary.txt. Uses NESTED_OUTPUT and OUTER_MONITOR as set (n1x: window, DP-7).
#   power.sh <spike> <omareel> <dir with b1080/ and b2160/> <out dir> [runs, default 2]
set -uo pipefail
spike="$1"; qt="$2"; bundles="$3"; out="$4"; runs="${5:-2}"
scripts="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$scripts/../../.." && pwd)"
mkdir -p "$out"
export NESTED_OUTPUT="${NESTED_OUTPUT:-window}" OUTER_MONITOR="${OUTER_MONITOR:-DP-7}"
nested() { (cd "$repo" && timeout 180 tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/power --timeout 120 -- "$scripts/in-nested.sh" "$@"); }
# window <label> <after s> <seconds> -- command...: dmon over [after, after+seconds] from start.
window() {
    local label="$1" after="$2" secs="$3"; shift 4
    python3 "$scripts/quiet.py" >/dev/null
    nvidia-smi dmon -s puc -d 1 -o T > "$out/$label.dmon" 2>/dev/null &
    local dmon=$!
    local start; start=$(date +%s)
    "$@" > "$out/$label.log" 2>&1
    kill "$dmon"; wait "$dmon" 2>/dev/null
    python3 - "$out/$label.dmon" "$start" "$after" "$secs" "$label" <<'PY' | tee -a "$out/summary.txt"
import sys, time, statistics
path, start, after, secs, label = sys.argv[1], int(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4]), sys.argv[5]
head, rows = None, []
for line in open(path):
    f = line.split()
    if line.startswith('#') and head is None:
        head = [x.lstrip('#') for x in f]
        continue
    if line.startswith('#') or not f:
        continue
    h, m, s = map(int, f[0].split(':'))
    lt = time.localtime(start)
    t = time.mktime(lt[:3] + (h, m, s) + lt[6:]) - start
    if after <= t <= after + secs:
        rows.append(dict(zip(head, f)))
num = lambda r, k: float(r[k]) if r.get(k, '-').replace('.', '', 1).isdigit() else None
out = {k: round(statistics.mean(v), 1) for k in ('pwr', 'sm', 'mem', 'dec', 'pclk')
       if (v := [x for x in (num(r, k) for r in rows) if x is not None])}
print(label, len(rows), 'samples', out)
PY
}
for n in $(seq 1 "$runs"); do
    window "idle-desktop-$n" 3 8 -- sleep 12
    window "nested-empty-$n" 3 8 -- nested sleep 12
    for b in b1080 b2160; do
        window "rune-$b-$n" 4 8 -- nested "$spike" preview "$bundles/$b" --measure 10 --stats "$out/rune-$b-$n.stats.json"
        window "qt-$b-$n" 3.5 8 -- env STOP_AFTER=13.5 OMAREEL_PREVIEW_STATS=1 bash -c "cd $repo && timeout 180 tests/tools/nested-hyprland.sh --directory /tmp/omareel-nested/power --timeout 120 -- $scripts/in-nested.sh $qt edit $bundles/$b"
    done
done
