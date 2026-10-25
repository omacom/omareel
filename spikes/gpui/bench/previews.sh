#!/bin/bash
# All preview measurements, ROUNDS times, interleaved: previews.sh ROUNDS SCRATCH GPUI_BIN QT_BIN
# One summary.py JSON line per run (prefixed with the load average) on stdout.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
rounds=$1; scratch=$2; gpui=$3; qt=$4
qt_wrapper=$(mktemp)
# argv[0] without "omareel": a recording parity test elsewhere on this machine SIGKILLs any
# "omareel edit" it finds.
printf '#!/bin/bash\nOMAREEL_PREVIEW_STATS=1 exec -a qt-reference %q edit "$@"\n' "$qt" > "$qt_wrapper"
chmod +x "$qt_wrapper"
for round in $(seq "$rounds"); do
    for run in qt-1080 gpui-1080 qt-2160 gpui-2160 gpui-2160-d1080; do
        size=${run#*-}; size=${size%%-*}
        load=$(cut -d' ' -f1 /proc/loadavg)
        case $run in
            qt-*) json=$("$here/preview.sh" "$run" "$qt_wrapper" "$scratch/bundle-$size") ;;
            gpui-2160-d1080) json=$("$here/preview.sh" "$run" "$gpui" preview "$scratch/bundle-2160" --bench 10 --decode-height 1080) ;;
            gpui-*) json=$("$here/preview.sh" "$run" "$gpui" preview "$scratch/bundle-$size" --bench 10) ;;
        esac
        echo "{\"round\": $round, \"load\": $load, \"result\": $json}"
    done
done
rm -f "$qt_wrapper"
