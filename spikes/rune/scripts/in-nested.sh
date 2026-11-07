#!/usr/bin/env bash
# Runs inside tests/tools/nested-hyprland.sh (Hyprland 0.57: Lua config through `hyprctl eval`). Gives the nested compositor a 1920x1080@60 headless
# output in place of its own window output (which follows the real display's refresh), then runs
# the command.
#   SHOT=<file.png> SHOT_AFTER=<s>       screenshot the output meanwhile
#   SAMPLE=<file.json> SAMPLE_AFTER=<s> SAMPLE_SECONDS=<s>
#                                        sample CPU, RSS and GPU of the command (sample.py)
#   STOP_AFTER=<s>                       end the command after this long (else wait for it)
#   NESTED_OUTPUT=window                 keep the window output (WAYLAND-1, at 1920x1080) instead:
#                                        on NVIDIA a headless output never gets a buffer (GBM
#                                        "bo null") and Hyprland aborts. Its frames follow the
#                                        refresh of the outer monitor the window is on.
#   OUTER_MONITOR=<name>                 with NESTED_OUTPUT=window: float this harness's window in
#                                        the outer compositor and move it onto that monitor (on
#                                        n1x, DP-7 is the 60 Hz one). Only this window is touched.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
output=SPIKE
if [[ "${NESTED_OUTPUT:-headless}" == window ]]; then
    output=WAYLAND-1
    hyprctl eval 'hl.monitor({ output = "WAYLAND-1", mode = "1920x1080@60", position = "0x0", scale = 1 })' >/dev/null
    if [[ -n "${OUTER_MONITOR:-}" ]]; then
        XDG_RUNTIME_DIR="$OMAREEL_OUTER_RUNTIME" WAYLAND_DISPLAY="$OMAREEL_OUTER_DISPLAY" \
            HYPRLAND_INSTANCE_SIGNATURE="$OMAREEL_OUTER_SIGNATURE" python3 - "$OUTER_MONITOR" <<'PY'
import json, os, subprocess, sys
# The outer window whose process carries this harness's marker.
marker = ('OMAREEL_NESTED_TEST=' + os.environ['OMAREEL_NESTED_TEST']).encode()
ctl = lambda *a: subprocess.run(['hyprctl', *a], capture_output=True, text=True, check=True).stdout
def ours(pid):
    try:
        return marker in open(f'/proc/{pid}/environ', 'rb').read().split(b'\0')
    except OSError:
        return False
client = next(c for c in json.loads(ctl('-j', 'clients')) if ours(c['pid']))
# The window output takes the window's logical size, so the window stays 1920x1080 (larger than
# a small monitor) and goes where it overlaps no other monitor: frame callbacks then come from
# this one only.
monitors = json.loads(ctl('-j', 'monitors'))
rect = lambda m: (m['x'], m['y'], m['width'] / m['scale'], m['height'] / m['scale'])
monitor = rect(next(m for m in monitors if m['name'] == sys.argv[1]))
others = [rect(m) for m in monitors if m['name'] != sys.argv[1]]
mx, my, mw, mh = monitor
W, H = 1920, 1080
def clear(x, y):
    return not any(x < ox + ow and ox < x + W and y < oy + oh and oy < y + H for ox, oy, ow, oh in others)
x, y = next(((x, y) for x in (mx, mx + mw - W) for y in (my, my + mh - H) if clear(x, y)), (mx, my))
w = f"address:{client['address']}"
for d in (f"hl.dsp.window.float({{action='enable',window='{w}'}})",
          f"hl.dsp.window.resize({{x={W},y={H},window='{w}'}})",
          f"hl.dsp.window.move({{x={int(x)},y={int(y)},window='{w}'}})"):
    ctl('eval', f'hl.dispatch({d})')
PY
    fi
    sleep 0.3
else
    hyprctl output create headless SPIKE >/dev/null
    hyprctl eval 'hl.monitor({ output = "SPIKE", mode = "1920x1080@60", position = "0x0", scale = 1 })' >/dev/null
    sleep 0.3
    for monitor in $(hyprctl monitors -j | python3 -c 'import json,sys; print(" ".join(m["name"] for m in json.load(sys.stdin) if m["name"] != "SPIKE"))'); do
        hyprctl eval "hl.monitor({ output = \"$monitor\", disabled = true })" >/dev/null
    done
fi
export NESTED_OUTPUT_NAME="$output"
hyprctl eval 'hl.config({ general = { gaps_out = 0, gaps_in = 0 }, decoration = { rounding = 0 } })' >/dev/null
sleep 0.5
started=$(date +%s.%N)
"$@" &
pid=$!
at() { sleep "$(python3 -c "import time; print(max(0, $started + $1 - time.time()))")"; }
if [[ -n "${SAMPLE:-}" ]]; then
    (at "${SAMPLE_AFTER:-3}"; python3 "$here/sample.py" "$pid" "${SAMPLE_SECONDS:-10}" "$SAMPLE" >/dev/null) &
    sampler=$!
fi
if [[ -n "${SHOT:-}" ]]; then
    for when in ${SHOT_AFTER:-3}; do
        (at "$when"; grim -o "$output" "${SHOT%.png}-$when.png" || true; hyprctl layers > "${SHOT%.png}.layers.txt") &
    done
fi
if [[ -n "${STOP_AFTER:-}" ]]; then
    at "$STOP_AFTER"
    kill "$pid" 2>/dev/null || true
fi
wait "$pid" || true
[[ -n "${sampler:-}" ]] && wait "$sampler" || true
sleep 0.2
