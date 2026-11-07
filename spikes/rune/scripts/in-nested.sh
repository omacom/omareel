#!/usr/bin/env bash
# Runs inside tests/tools/nested-hyprland.sh (Hyprland 0.57: Lua config through `hyprctl eval`). Gives the nested compositor a 1920x1080@60 headless
# output in place of its own window output (which follows the real display's refresh), then runs
# the command.
#   SHOT=<file.png> SHOT_AFTER=<s>       screenshot the output meanwhile
#   SAMPLE=<file.json> SAMPLE_AFTER=<s> SAMPLE_SECONDS=<s>
#                                        sample CPU, RSS and GPU of the command (sample.py)
#   STOP_AFTER=<s>                       end the command after this long (else wait for it)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
hyprctl output create headless SPIKE >/dev/null
hyprctl eval 'hl.monitor({ output = "SPIKE", mode = "1920x1080@60", position = "0x0", scale = 1 })' >/dev/null
sleep 0.3
for monitor in $(hyprctl monitors -j | python3 -c 'import json,sys; print(" ".join(m["name"] for m in json.load(sys.stdin) if m["name"] != "SPIKE"))'); do
    hyprctl eval "hl.monitor({ output = \"$monitor\", disabled = true })" >/dev/null
done
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
        (at "$when"; grim -o SPIKE "${SHOT%.png}-$when.png" || true; hyprctl layers > "${SHOT%.png}.layers.txt") &
    done
fi
if [[ -n "${STOP_AFTER:-}" ]]; then
    at "$STOP_AFTER"
    kill "$pid" 2>/dev/null || true
fi
wait "$pid" || true
[[ -n "${sampler:-}" ]] && wait "$sampler" || true
sleep 0.2
