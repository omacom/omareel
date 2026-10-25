#!/bin/bash
# Inside the nested harness: one preview measurement. preview-body.sh NAME -- command...
here=$(dirname "$0")
name=$1; shift; shift
exec python3 "$here/nested-body.py" --name "$name" --start 1.5 --seconds 10 --shot 3 --wait 8 --env WAYLAND_DEBUG=client -- "$@"
