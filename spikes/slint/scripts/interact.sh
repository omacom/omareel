#!/usr/bin/env bash
# Runs inside tests/tools/nested-hyprland.sh (after in-nested.sh's output setup, so call it as
# `in-nested.sh interact.sh <spike> <spike-pointer> <bundle> <out dir>`): clicks the preview's
# Play button and its scrub bar, and the recording bar's Stop button, through wlr-virtual-pointer,
# with screenshots.
set -uo pipefail
spike="$1"; pointer="$2"; bundle="$3"; out="$4"
mkdir -p "$out"
"$spike" preview "$bundle" > "$out/preview.log" 2>&1 &
preview=$!
sleep 3
grim -o SPIKE "$out/1-paused.png"
"$pointer" 62 1050            # Play
sleep 2.5
grim -o SPIKE "$out/2-playing.png"
"$pointer" 62 1050            # Pause
sleep 0.5
"$pointer" 1100 1050          # the scrub bar, about 6 s in
sleep 1.5
grim -o SPIKE "$out/3-scrubbed.png"
kill "$preview" 2>/dev/null; wait "$preview" 2>/dev/null
"$spike" bar > "$out/bar.log" 2>&1 &
bar=$!
sleep 2
grim -o SPIKE "$out/4-bar.png"
"$pointer" 1030 38            # Stop
sleep 1
if kill -0 "$bar" 2>/dev/null; then echo "bar still running after Stop" >> "$out/bar.log"; kill "$bar"; else echo "bar exited after Stop" >> "$out/bar.log"; fi
