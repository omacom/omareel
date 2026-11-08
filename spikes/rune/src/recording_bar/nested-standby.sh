#!/usr/bin/env bash
# The standby self-view host in the nested compositor: the bubble the launcher shows before a
# recording (`__record-bar --standby --owner <pid>`), commanded through `__selfview-ipc`, then
# adopted by a recording (one process shows the bar and keeps its bubble), back in standby when
# the recording ends, and gone on `quit`. Run from the repository root like nested-test.sh:
#
#   NESTED_OUTPUT=window OUTER_MONITOR=DP-7 tests/tools/nested-hyprland.sh \
#       --directory /tmp/omareel-nested/recording-bar-standby --timeout 120 -- \
#       spikes/rune/scripts/in-nested.sh spikes/rune/src/recording_bar/nested-standby.sh \
#       spikes/rune/target/release/omareel-rune-spike /tmp/omareel-nested/recording-bar-standby/out
#
# As in nested-test.sh, the recording's overlays are pointed back at the one output (once the
# capture started: the adopted host must first take its bubble off it, for the recorder) and
# cursors kept in software.
set -uo pipefail
spike="$(realpath "$1")"; out="$2"; camera="${3:-/dev/video0}"
mkdir -p "$out"
output="${NESTED_OUTPUT_NAME:-WAYLAND-1}"
shot() { grim -o "$output" "$out/$1.png"; hyprctl layers > "$out/$1.layers.txt"; cp "$XDG_RUNTIME_DIR/omareel/selfview-host.json" "$out/$1.host.json" 2>/dev/null; }
state="$XDG_RUNTIME_DIR/omareel/recording.json"
mkdir -p "$XDG_CONFIG_HOME/omareel"
cat > "$XDG_CONFIG_HOME/omareel/settings.json" <<EOF
{ "selfViewEnabled": true, "selfViewSize": "M", "selfViewPosition": { "x": 1, "y": 1 },
  "webcam": { "enabled": true, "device": "$camera", "captureHeight": 1080, "shape": "square", "radius": 24 } }
EOF

python3 - "$state" "$output" > "$out/patch.log" 2>&1 <<'PY' &
import json, os, subprocess, sys, time
path, output = sys.argv[1:]
while True:
    got = subprocess.run(['hyprctl', '-j', 'getoption', 'cursor:no_hardware_cursors'], capture_output=True, text=True).stdout
    if '"int": 0' in got:
        subprocess.run(['hyprctl', 'eval', 'hl.config({ cursor = { no_hardware_cursors = 1 } })'], capture_output=True)
    try:
        state = json.load(open(path))
        want = {'overlay_monitor': output, 'selfview_monitor': output, 'bar_visible': True, 'selfview_safe': True}
        if state.get('capture_started') and any(state.get(k) != v for k, v in want.items()):
            state.update(want)
            json.dump(state, open(path + '.patch', 'w'))
            os.replace(path + '.patch', path)
            print(f'{time.time():.3f} patched', flush=True)
    except Exception:
        pass
    time.sleep(0.05)
PY
patcher=$!

"$spike" __record-bar --standby --owner $$ >> "$out/host.log" 2>&1 &
host=$!
sleep 3; shot 1-standby
# A second host finds the first one's lock and leaves at once.
timeout 5 "$spike" __record-bar --standby --owner $$; echo "second host exit $?" >> "$out/host.log"
"$spike" __selfview-ipc L; sleep 0.8; shot 2-large
"$spike" __selfview-ipc corner; sleep 0.8; shot 3-corner
"$spike" __selfview-ipc shape; sleep 0.8; shot 4-shape

"$spike" record --fullscreen --with-webcam --webcam-device "$camera" --no-audio --no-open --dir "$out/recordings" > "$out/record.log" 2>&1
sleep 2; shot 5-adopted
pgrep -af "__record-bar" > "$out/5-processes.txt"
cp "$state" "$out/5-state.json"
"$spike" record --stop >> "$out/record.log" 2>&1
for _ in $(seq 1 100); do [[ -f "$state" ]] || break; sleep 0.2; done
sleep 1.5; shot 6-back-in-standby
"$spike" __selfview-ipc quit; sleep 1
if kill -0 "$host" 2>/dev/null; then echo "host still running after quit" >> "$out/host.log"; kill "$host"; else echo "host exited after quit" >> "$out/host.log"; fi
shot 7-after-quit
kill "$patcher" 2>/dev/null; wait "$patcher" 2>/dev/null
ls -la "$out/recordings"/*/ > "$out/bundle.txt" 2>&1
true
