#!/bin/sh
# ffmpeg that refuses the screen encoder's Matroska input and runs everything else.
for argument in "$@"; do
    if [ "$argument" = "matroska" ]; then
        echo "pipe:0: Invalid data found when processing input" >&2
        exit 1
    fi
done
exec /usr/bin/ffmpeg "$@"
