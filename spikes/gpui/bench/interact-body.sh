#!/bin/bash
# Inside tests/tools/nested-hyprland.sh: clicks through the bar and the preview's controls and
# saves screenshots. interact-body.sh BIN_DIR BUNDLE
set -u
bin=$1; bundle=$2; A=$OMAREEL_TEST_ARTIFACTS
hyprctl output create headless SPIKE >/dev/null; sleep 0.5
hyprctl keyword monitor SPIKE,1920x1080@60,5000x0,1 >/dev/null; sleep 0.5
hyprctl dispatch focusmonitor SPIKE >/dev/null
log() { echo "$*" >> "$A/interact.log"; }

# The layer-shell bar: anchored top centre; Stop quits.
"$bin/omareel-gpui" bar > "$A/bar.log" 2>&1 &
bar=$!
sleep 2.5
read -r x y w h < <(hyprctl -j layers | python3 -c "
import json,sys
for mon,v in json.load(sys.stdin).items():
    for lvl,l in v['levels'].items():
        for s in l:
            if s['namespace']=='omareel-bar': print(s['x'], s['y'], s['w'], s['h'], flush=True)")
log "bar layer: x=$x y=$y w=$w h=$h (output SPIKE starts at x=5000)"
grim -o SPIKE "$A/bar.png"
hyprctl dispatch movecursor $((x + w / 2 + 40)) $((y + h / 2)) >/dev/null
sleep 0.4
grim -o SPIKE "$A/bar-hover.png"
"$bin/spike-pointer" click
sleep 0.5
if kill -0 $bar 2>/dev/null; then log "bar: still running after clicking Stop"; kill $bar; else log "bar: Stop quit it"; fi

# The preview: play, pause, scrub.
"$bin/omareel-gpui" preview "$bundle" > "$A/preview.log" 2>&1 &
preview=$!
sleep 2.5
read -r wx wy ww wh < <(hyprctl -j clients | python3 -c "
import json,sys
for c in json.load(sys.stdin):
    if c['class']=='omareel-gpui': print(c['at'][0], c['at'][1], c['size'][0], c['size'][1], flush=True)")
log "preview window: $wx $wy ${ww}x$wh"
grim -o SPIKE "$A/preview-0-paused.png"
play_x=$((wx + 52)); controls_y=$((wy + wh - 28))
hyprctl dispatch movecursor $play_x $controls_y >/dev/null; sleep 0.2
"$bin/spike-pointer" click
sleep 2
grim -o SPIKE "$A/preview-1-playing.png"
"$bin/spike-pointer" click
sleep 0.5
grim -o SPIKE "$A/preview-2-paused.png"
bar_x0=$((wx + 102)); bar_w=$((ww - 102 - 16 - 14 - 165))
hyprctl dispatch movecursor $((bar_x0 + bar_w / 5)) $controls_y >/dev/null; sleep 0.2
"$bin/spike-pointer" drag $((bar_w / 2)) 20
sleep 1
grim -o SPIKE "$A/preview-3-scrubbed.png"
kill $preview
log "done"
