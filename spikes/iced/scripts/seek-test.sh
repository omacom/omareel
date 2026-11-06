#!/usr/bin/env bash
# Drives the preview with the keyboard inside a nested Hyprland (run through in-nested.sh):
# three seconds forward, play for a second and a half, pause, two seconds back, screenshots in
# between. seek-test.sh <spike binary> <bundle> <out prefix>
set -euo pipefail
spike="$1"; bundle="$2"; out="$3"
"$spike" preview "$bundle" > "$out.log" 2>&1 &
pid=$!
sleep 3
shot() { sleep "$1"; grim -o SPIKE "$out-$2.png"; }
shot 0 0-start
for _ in 1 2 3; do wtype -k Right; sleep 0.15; done
shot 0.8 1-at-3s
wtype -k space
shot 1.5 2-playing
wtype -k space
shot 0.5 3-paused
wtype -k Left; sleep 0.15; wtype -k Left
shot 0.8 4-back-2s
wtype -k Home
shot 0.8 5-home
kill "$pid"
wait "$pid" 2>/dev/null || true
