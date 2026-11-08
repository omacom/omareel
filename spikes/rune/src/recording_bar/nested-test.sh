#!/usr/bin/env bash
# The recording bar end to end in the nested compositor: a real fullscreen recording with the
# webcam and the countdown, screenshots of the countdown, the bar and the bubble, a drag of the
# bubble, the bar's camera buttons and discard confirmation, and Stop from the bar. Run from
# the repository root:
#
#   NESTED_OUTPUT=window OUTER_MONITOR=DP-7 tests/tools/nested-hyprland.sh \
#       --directory /tmp/omareel-nested/recording-bar --timeout 150 -- \
#       spikes/rune/scripts/in-nested.sh spikes/rune/src/recording_bar/nested-test.sh \
#       spikes/rune/target/release/omareel-rune-spike spikes/rune/target/release/spike-pointer \
#       /tmp/omareel-nested/recording-bar/out [/dev/video0]
#
# The nested compositor has one output, and on aarch64 Hyprland plugins can't hook the
# renderer, so the recorder places overlays on another monitor ("fallback") and here finds none:
# the bar would stay hidden. Once the recorder has checked that no overlay is on the recorded
# output, this test points the overlays back at it in the recording's state (as the capture
# exclusion plugin would leave them), so they show (and are recorded).
#
# The recorder also forces hardware cursors while it records (so the pointer stays out of the
# frames); the nested compositor on NVIDIA aborts drawing one (CPointerManager::
# renderHWCursorBuffer) as soon as the pointer moves. The test puts software cursors back.
set -uo pipefail
spike="$(realpath "$1")"; pointer="$(realpath "$2")"; out="$3"; camera="${4:-/dev/video0}"
mkdir -p "$out"
output="${NESTED_OUTPUT_NAME:-WAYLAND-1}"
shot() { grim -o "$output" "$out/$1.png"; hyprctl layers > "$out/$1.layers.txt"; }
state="$XDG_RUNTIME_DIR/omareel/recording.json"
mkdir -p "$XDG_CONFIG_HOME/omareel"
cat > "$XDG_CONFIG_HOME/omareel/settings.json" <<EOF
{ "countdownBeforeRecording": true, "selfViewEnabled": true, "selfViewSize": "M",
  "selfViewPosition": { "x": 1, "y": 1 },
  "webcam": { "enabled": true, "device": "$camera", "captureHeight": 1080, "shape": "round", "radius": 16 } }
EOF

# Keeps the overlays on the recorded output for the first seconds of the recording, and the
# cursors in software.
python3 - "$state" "$output" > "$out/patch.log" 2>&1 <<'PY' &
import json, os, subprocess, sys, time
path, output = sys.argv[1:]
seen = None
end = time.time() + 12
cursor_check = 0
while True:
    if time.time() > cursor_check:
        cursor_check = time.time() + 0.1
        got = subprocess.run(['hyprctl', '-j', 'getoption', 'cursor:no_hardware_cursors'], capture_output=True, text=True).stdout
        if '"int": 0' in got:
            subprocess.run(['hyprctl', 'eval', 'hl.config({ cursor = { no_hardware_cursors = 1 } })'], capture_output=True)
            print(f'{time.time():.3f} software cursors', flush=True)
    if time.time() > end:
        time.sleep(0.05); continue
    try:
        state = json.load(open(path))
    except Exception:
        time.sleep(0.02); continue
    if 'bundle' not in state:
        time.sleep(0.02); continue
    seen = seen or time.time()
    if time.time() - seen < 0.4:
        time.sleep(0.02); continue
    want = {'overlay_monitor': output, 'selfview_monitor': output, 'selfview_safe': True}
    if not os.environ.get('NO_BAR'):
        want['bar_visible'] = True
    if any(state.get(k) != v for k, v in want.items()):
        state.update(want)
        tmp = path + '.patch'
        json.dump(state, open(tmp, 'w'))
        os.replace(tmp, path)
        print(f'{time.time():.3f} patched', flush=True)
    time.sleep(0.05)
PY
patcher=$!

# NO_BAR=1 records with --no-bar (the bar stays hidden; countdown and bubble still show).
"$spike" record --fullscreen --with-webcam --webcam-device "$camera" --no-audio --no-open --dir "$out/recordings" ${NO_BAR:+--no-bar} \
    > "$out/record.log" 2>&1 &
record=$!
# The countdown: three seconds once the bar published its geometry.
sleep 1.6; shot 1-countdown
sleep 1.0; shot 2-countdown
wait "$record"
sleep 1.5; shot 3-bar-and-bubble
cp "$state" "$out/state-recording.json"
"$spike" __record-bar-ipc rotate >> "$out/ipc.log" 2>&1
sleep 0.8; shot 4-rotated-by-ipc

if [[ -n "${STOP_FROM_CLI:-}" ]]; then
    sleep 1; "$spike" record --stop >> "$out/record.log" 2>&1
else
    # Pointer steps come from a file of "label x y [click|move|drag x2 y2]" lines, if given.
    if [[ -f "${STEPS:-}" ]]; then
        while read -r label x y rest; do
            [[ -z "$label" || "$label" == \#* ]] && continue
            if [[ "$label" == wait ]]; then sleep "$x"; continue; fi
            # shellcheck disable=SC2086
            "$pointer" "$x" "$y" $rest
            sleep "${STEP_PAUSE:-0.5}"; shot "step-$label"
            cp "$state" "$out/state-$label.json" 2>/dev/null
        done < "$STEPS"
    fi
fi
# The recorder saves and exits; the bar goes with it.
for _ in $(seq 1 100); do
    [[ -f "$state" ]] || break
    sleep 0.2
done
sleep 1
shot 9-after
kill "$patcher" 2>/dev/null; wait "$patcher" 2>/dev/null
ls -la "$out/recordings"/*/ > "$out/bundle.txt" 2>&1
pgrep -af omareel-rune-spike > "$out/leftover.txt" 2>&1
true
