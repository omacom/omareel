#!/usr/bin/env bash
# The whole Rune omareel in the nested compositor, as a person would use it: the launcher, Ctrl+R,
# a click on the desktop in the region picker (the whole output), the countdown, the recording bar
# and the camera bubble, some pointer movement and typing to record, Stop from the bar, and the
# editor the recorder opens on the new recording. Screenshots of every step. Run from the
# repository root:
#
#   NESTED_OUTPUT=window OUTER_MONITOR=DP-7 tests/tools/nested-hyprland.sh \
#       --directory /tmp/omareel-nested/full-loop --timeout 170 -- \
#       spikes/rune/scripts/in-nested.sh spikes/rune/scripts/full-loop.sh \
#       spikes/rune/target/release/omareel-rune-spike spikes/rune/target/release/spike-pointer \
#       /tmp/omareel-nested/full-loop/out [/dev/video0]
#
# Two things about the nested compositor, not the app, are worked around as in
# src/recording_bar/nested-test.sh: it has one output, so the overlays the recorder moves off
# the recorded output (there is no capture-exclusion plugin on aarch64) are pointed back at it;
# and it aborts drawing hardware cursors on NVIDIA, so software cursors are put back.
set -uo pipefail
spike="$(realpath "$1")"; pointer="$(realpath "$2")"; out="$3"; camera="${4:-/dev/video0}"
mkdir -p "$out/recordings"
export OMAREEL_RECENTS_DIR="$out/recordings"
export PULSE_SERVER="unix:${OMAREEL_OUTER_RUNTIME:-/run/user/$(id -u)}/pulse/native"
output="${NESTED_OUTPUT_NAME:-WAYLAND-1}"
log() { echo "$(date +%T.%3N) $*" >> "$out/steps.log"; }
shot() { grim -o "$output" "$out/$1.png"; log "shot $1"; }
state="$XDG_RUNTIME_DIR/omareel/recording.json"
mkdir -p "$XDG_CONFIG_HOME/omareel"
cat > "$XDG_CONFIG_HOME/omareel/settings.json" <<EOF
{ "systemAudio": false, "microphone": false, "countdownBeforeRecording": true,
  "selfViewEnabled": true, "selfViewSize": "M", "selfViewPosition": { "x": 1, "y": 1 },
  "outputDirectory": "$out/recordings",
  "webcam": { "enabled": true, "device": "$camera", "captureHeight": 1080, "shape": "round", "radius": 16 } }
EOF

python3 - "$state" "$output" > "$out/patch.log" 2>&1 <<'PY' &
import json, os, subprocess, sys, time
path, output = sys.argv[1:]
seen, cursor_check = None, 0
while True:
    if time.time() > cursor_check:
        cursor_check = time.time() + 0.1
        got = subprocess.run(['hyprctl', '-j', 'getoption', 'cursor:no_hardware_cursors'], capture_output=True, text=True).stdout
        if '"int": 0' in got:
            subprocess.run(['hyprctl', 'eval', 'hl.config({ cursor = { no_hardware_cursors = 1 } })'], capture_output=True)
    try:
        state = json.load(open(path))
    except Exception:
        seen = None; time.sleep(0.02); continue
    if 'bundle' not in state:
        time.sleep(0.02); continue
    seen = seen or time.time()
    if time.time() - seen < 0.4:
        time.sleep(0.02); continue
    want = {'overlay_monitor': output, 'selfview_monitor': output, 'selfview_safe': True, 'bar_visible': True}
    if any(state.get(k) != v for k, v in want.items()):
        state.update(want)
        json.dump(state, open(path + '.patch', 'w'))
        os.replace(path + '.patch', path)
    time.sleep(0.05)
PY
patcher=$!

# 1. The launcher.
"$spike" > "$out/launcher.log" 2>&1 &
launcher=$!
sleep 5; shot 01-launcher
# 2. Ctrl+R starts recording: the region picker.
wtype -M ctrl r -m ctrl
sleep 3; shot 02-picker
# 3. A click on the bare desktop takes the whole output.
"$pointer" 1700 950
sleep 1.4; shot 03-countdown
sleep 3.4; shot 04-recording
kill -0 "$launcher" 2>/dev/null && log "launcher still running" || log "launcher quit when the capture started"
# 4. Something to record: the pointer moves and clicks around, a key or two.
for p in "400 300" "900 500" "1300 420" "700 800" "1500 700"; do
    "$pointer" $p move; sleep 0.5
done
"$pointer" 960 540; sleep 0.6
wtype -M ctrl s -m ctrl; sleep 0.8
shot 05-recording-later
cp "$state" "$out/state-recording.json" 2>/dev/null
# 5. Stop from the bar (Stop is at x 1068 on the 448-wide bar, centred at the top).
"$pointer" 1068 32
sleep 0.5; shot 06-stopping
for _ in $(seq 1 100); do [[ -f "$state" ]] || break; sleep 0.2; done
log "recorder finished"
# 6. The editor the recorder opens.
for _ in $(seq 1 60); do
    hyprctl -j clients | grep -q '"class": "omareel"' && break
    sleep 0.25
done
sleep 4; shot 07-editor
hyprctl -j clients | python3 -c 'import json,sys; print("clients:", [(c["class"], c["title"], c["size"]) for c in json.load(sys.stdin)])' >> "$out/steps.log"
# 7. Play two seconds.
wtype ' '; sleep 2; shot 08-editor-playing
wtype ' '; sleep 0.5
kill "$patcher" 2>/dev/null
for bundle in "$out"/recordings/*/; do
    { ls -la "$bundle"; ffprobe -v error -show_entries format=duration:stream=codec_name,width,height -of compact "$bundle/screen.mp4" "$bundle/camera.mp4"; } >> "$out/steps.log" 2>&1
done
pgrep -af omareel-rune-spike >> "$out/steps.log"
true
