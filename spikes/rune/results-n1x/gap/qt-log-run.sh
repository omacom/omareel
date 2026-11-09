#!/usr/bin/env bash
# run.sh <out log> <seconds> <app> <bundle template>: one Qt editor launch with Qt Multimedia's logs.
out=$1 secs=$2 app=$3 tpl=$4
b="$XDG_RUNTIME_DIR/gap-bundle"; rm -rf "$b"; cp -r "$tpl" "$b"
QT_FORCE_STDERR_LOGGING=1 WAYLAND_DEBUG=client QT_LOGGING_RULES="qt.multimedia.ffmpeg*=true;qt.multimedia.playback*=true;qt.scenegraph.time.renderloop=true" \
QT_MESSAGE_PATTERN='[%{time process}] %{category}: %{message}' "$app" edit "$b" > "$out" 2>&1 &
pid=$!; sleep "$secs"; kill $pid; wait $pid
