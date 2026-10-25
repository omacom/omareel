#!/bin/sh
# hyprctl whose monitor list names the nested output FAKE-1, so the in-process capture cannot
# find it and the recorder falls back to gpu-screen-recorder.
if [ "$1" = "-j" ] && [ "$2" = "monitors" ]; then
    /usr/bin/hyprctl "$@" | sed 's/"WAYLAND-1"/"FAKE-1"/'
else
    exec /usr/bin/hyprctl "$@"
fi
