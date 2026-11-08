#!/usr/bin/env bash
# Runs inside tests/tools/nested-hyprland.sh (after in-nested.sh's output setup, so call it as
# `in-nested.sh launcher-interact.sh <spike> <spike-pointer> <out dir>`): drives the Rune
# launcher through wlr-virtual-pointer and wtype in the nested compositor, then records the
# nested output from it end to end: Record, a click on the desktop in the region picker, the
# recorder daemon, `record --stop`, the saved bundle and the editor the daemon opens.
#
# The nested harness gives the run its own XDG_RUNTIME_DIR and XDG_CONFIG_HOME, so the launcher
# starts from default preferences and the recorder's state stays private. PULSE_SERVER points
# PulseAudio clients (the device list, the meter) at the desktop's server; audio is switched off
# before recording. Recordings go to <out dir>/recordings.
set -uo pipefail
spike="$1"; pointer="$2"; out="$3"
mkdir -p "$out/recordings"
export OMAREEL_RECENTS_DIR="$out/recordings"
export PULSE_SERVER="unix:${OMAREEL_OUTER_RUNTIME:-/run/user/$(id -u)}/pulse/native"
output="${NESTED_OUTPUT_NAME:-SPIKE}"
shot() { grim -o "$output" "$out/$1.png"; echo "shot $1" >> "$out/steps.log"; }
step() { echo "$*" >> "$out/steps.log"; }

"$spike" > "$out/launcher.log" 2>&1 &
launcher=$!
sleep 5
read -r wx wy ww wh < <(hyprctl -j clients | python3 -c '
import json, sys
c = next(c for c in json.load(sys.stdin) if c["class"] == "omareel")
print(c["at"][0], c["at"][1], c["size"][0], c["size"][1])')
step "launcher window at $wx,$wy size ${ww}x$wh, floating: $(hyprctl -j clients | python3 -c 'import json,sys; print([c["floating"] for c in json.load(sys.stdin) if c["class"]=="omareel"])')"
# The capture exclusion note (one line) moves the sources card down.
note=0
grep -q "recording overlays stay off" "$out/launcher.log" && note=22
at() { "$pointer" $((wx + $1)) $((wy + $2)) ${3:-click}; }
shot 01-launcher

# The reel turns once (Ctrl+Shift+R).
wtype -M ctrl -M shift r -m shift -m ctrl
sleep 0.2; shot 02-reel-spinning

# Capture settings: open, pick the large self-view, close with Escape.
at 344 34; sleep 0.5; shot 03-settings
at 326 222; sleep 0.4; shot 04-settings-size-L
wtype -k Escape; sleep 0.5; shot 05-settings-closed

# The microphone's device menu, then Escape.
at 120 $((356 + note)); sleep 0.6; shot 06-microphone-menu
wtype -k Escape; sleep 0.4

# Webcam on: the window grows by the camera tools; the self-view host's status shows.
at 200 $((400 + note)); sleep 2.5; shot 07-webcam-on
step "window after webcam on: $(hyprctl -j clients | python3 -c 'import json,sys; print([c["size"] for c in json.load(sys.stdin) if c["class"]=="omareel"])')"
at 303 $((442 + note)); sleep 0.3; at 303 $((442 + note)); sleep 0.3
at 200 $((400 + note)); sleep 1; shot 08-webcam-off

# Audio off, so the recording takes no sound from this machine.
at 200 $((294 + note)); sleep 0.3
at 200 $((347 + note)); sleep 0.8; shot 09-audio-off

# A hover over the record button, then Record.
at 190 120 move; sleep 0.6; shot 10-record-hover
at 190 120; sleep 0.25; shot 11-starting
sleep 2.5; shot 12-picker
# A click on the bare desktop, away from the launcher, takes the whole output.
"$pointer" 1700 950; sleep 4
shot 13-recording
if kill -0 "$launcher" 2>/dev/null; then step "launcher still running after the capture started"; else step "launcher quit when the capture started"; fi
cp "$XDG_RUNTIME_DIR/omareel/recording.json" "$out/recording-state.json" 2>/dev/null
sleep 3
"$spike" record --stop > "$out/stop.log" 2>&1; step "record --stop exited $?"
sleep 6; shot 14-after-stop
ls -la "$out/recordings" >> "$out/steps.log"
for bundle in "$out"/recordings/*; do
    [[ -d "$bundle" ]] && { ls -la "$bundle"; ffprobe -v error -show_entries format=duration:stream=width,height,codec_name -of compact "$bundle/screen.mp4"; } >> "$out/steps.log" 2>&1
done
hyprctl -j clients | python3 -c 'import json,sys; print("clients:", [(c["class"], c["title"]) for c in json.load(sys.stdin)])' >> "$out/steps.log"
sleep 2; shot 15-editor
