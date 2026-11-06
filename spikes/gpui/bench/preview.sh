#!/bin/bash
# One preview measurement in the nested Hyprland, on a 60 Hz headless output:
#   preview.sh NAME command...
# Artifacts go to /tmp/ogp/NAME (small files; the harness wants a short path for its sockets).
# Prints summary.py's JSON.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
name=$1; shift
rm -rf "/tmp/ogp/$name"
OMAREEL_NESTED_ROOT=/tmp/ogp "$repo/tests/tools/nested-hyprland.sh" --directory "/tmp/ogp/$name" --timeout 90 -- \
    python3 "$here/nested-body.py" --name "$name" --timeout 22 --env WAYLAND_DEBUG=client -- "$@" > /dev/null
python3 "$here/summary.py" "/tmp/ogp/$name" "$name" 10
