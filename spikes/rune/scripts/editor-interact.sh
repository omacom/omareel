#!/usr/bin/env bash
# Runs inside tests/tools/nested-hyprland.sh after in-nested.sh's output setup:
#   in-nested.sh editor-interact.sh <spike> <spike-pointer> <bundle> <out dir>
# Drives the editor slice by hand, as a person would: camera shapes, two slider drags, a zoom
# drag, a clip, a seek, S to split, Ctrl+Z, Space to play and pause, with a screenshot after each
# step. Positions are for a 1920x1080 output at scale 1 (check them against 0-open.png).
set -uo pipefail
spike="$1"; pointer="$2"; bundle="$3"; out="$4"
output="${NESTED_OUTPUT_NAME:-SPIKE}"
mkdir -p "$out"
"$spike" editor "$bundle" > "$out/editor.log" 2>&1 &
app=$!
sleep 3
n=0
shot() { n=$((n + 1)); sleep "${2:-0.8}"; grim -o "$output" "$out/$n-$1.png"; }
grim -o "$output" "$out/0-open.png"
"$pointer" 1758 304;            shot square
"$pointer" 1845 304;            shot rectangle
"$pointer" 1672 304;            shot round
"$pointer" 1733 133 drag 1650 133; shot padding-dragged
"$pointer" 1693 186 drag 1800 186; shot radius-dragged
"$pointer" 540 976;             shot zoom-selected 0.5
"$pointer" 540 976 drag 660 976; shot zoom-dragged
"$pointer" 1046 936;            shot clip2-selected
"$pointer" 460 900;             shot seek-2s
wtype s;                        shot split
wtype -M ctrl z -m ctrl;        shot undo
wtype ' ';                      shot playing 1.5
wtype ' ';                      shot paused
kill "$app" 2>/dev/null; wait "$app" 2>/dev/null
