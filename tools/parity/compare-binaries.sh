#!/usr/bin/env bash
# Runs two omareel binaries on the same inputs and reports any difference in what they print,
# their exit codes, and the editor and launcher they draw. Used to check that moving code to
# Rust changed nothing: compare-binaries.sh <old omareel> <new omareel>.
#
# Everything runs in a scratch HOME with fixed XDG directories, an Omarchy state directory made
# from tests/golden/corpus/themes, and the golden shims for fc-match and hyprctl first on PATH,
# so both binaries see exactly the same world.
set -euo pipefail

old="$(realpath "$1")"
new="$(realpath "$2")"
root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d /tmp/omareel-parity.XXXXXX)"
trap 'rm -rf "$scratch"' EXIT
python="${PYTHON:-python3}"
failures=0

mkdir -p "$scratch"/{home,config,cache,state,data,runtime,bundles,out}
chmod 700 "$scratch/runtime"
state="$scratch/state/omarchy"
mkdir -p "$state/theme"
cp "$root/tests/golden/corpus/themes/tokyo-night/colors.toml" "$state/theme/"
[[ -f "$root/tests/golden/corpus/themes/tokyo-night/shell.toml" ]] && cp "$root/tests/golden/corpus/themes/tokyo-night/shell.toml" "$state/theme/"
printf 'tokyo-night\n' > "$state/theme.name"

run() { # run <old|new> <name> <args...>: stdout, stderr and status go to out/<name>.<side>.*
    local side="$1" name="$2"; shift 2
    local binary; binary="$([[ "$side" == old ]] && echo "$old" || echo "$new")"
    set +e
    env -i HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_CACHE_HOME="$scratch/cache" \
        XDG_STATE_HOME="$scratch/state" XDG_DATA_HOME="$scratch/data" XDG_RUNTIME_DIR="$scratch/runtime" \
        PATH="$root/tests/golden/shims:/usr/bin:/bin" LANG=C.UTF-8 TZ=UTC \
        OMAREEL_OMARCHY_STATE_DIR="$state" QT_QPA_PLATFORM=offscreen OMAREEL_THEME_RADIUS=8 \
        "${EXTRA_ENV[@]}" "$binary" "$@" > "$scratch/out/$name.$side.stdout" 2> "$scratch/out/$name.$side.stderr"
    echo $? > "$scratch/out/$name.$side.status"
    set -e
}

same() { # same <name> <what>: compare one output channel of both runs
    if ! cmp -s "$scratch/out/$1.old.$2" "$scratch/out/$1.new.$2"; then
        echo "DIFF $1 ($2):"; { diff "$scratch/out/$1.old.$2" "$scratch/out/$1.new.$2" || true; } | head -20
        failures=$((failures + 1))
    fi
}

both() { # both <name> <args...>: run both binaries and compare stdout, stderr and status
    local name="$1"; shift
    run old "$name" "$@"; run new "$name" "$@"
    same "$name" stdout; same "$name" status
    # Qt prints its own warnings to stderr; the app's own lines must match.
    grep -v '^qt\.\|^QFile::open' "$scratch/out/$name.old.stderr" > "$scratch/out/$name.old.err" || true
    grep -v '^qt\.\|^QFile::open' "$scratch/out/$name.new.stderr" > "$scratch/out/$name.new.err" || true
    same "$name" err
}

# Bundles: a short test pattern recording with the synthetic input log, with and without a
# camera, and broken variants.
make_bundle() {
    local dir="$scratch/bundles/$1" camera="$2"
    mkdir -p "$dir"
    cp "$root/tests/fixtures/synthetic/capture.json" "$root/tests/fixtures/synthetic/input.jsonl" "$dir/"
    ffmpeg -v error -f lavfi -i testsrc=size=640x360:rate=60 -t 8 -pix_fmt yuv420p "$dir/screen.mp4"
    [[ "$camera" == camera ]] && ffmpeg -v error -f lavfi -i testsrc2=size=320x240:rate=30 -t 8 -pix_fmt yuv420p "$dir/camera.mp4"
    return 0
}
make_bundle plain none
make_bundle with-camera camera
make_bundle bad-input none
printf '{"t":1}\n{"t":\n' > "$scratch/bundles/bad-input/input.jsonl"
make_bundle no-capture none
rm "$scratch/bundles/no-capture/capture.json"
mkdir -p "$scratch/bundles/no-screen"
cp "$root/tests/fixtures/synthetic/"* "$scratch/bundles/no-screen/"

EXTRA_ENV=()
for bundle in plain with-camera bad-input no-capture no-screen missing; do
    both "probe-$bundle" probe "$scratch/bundles/$bundle"
done
both probe-no-argument probe
both help help
both help-flag --help
both version --version
both unknown bogus

# Screenshots: the editor on a bundle, a few panels, and the launcher.
if "$python" -c 'import PIL' 2>/dev/null; then
    # The editor saves project.json and fills a thumbnail cache; each run starts without the
    # former, and a first run warms the latter so both binaries draw the same timeline.
    fresh() { rm -f "$scratch/bundles/with-camera/project.json"; }
    shot() { # shot <name> <args...> with OMAREEL_SCREENSHOT* variables in EXTRA_ENV
        fresh; run old "$1" "${@:2}"; mv "$scratch/out/shot.png" "$scratch/out/$1.old.png" 2>/dev/null || true
        fresh; run new "$1" "${@:2}"; mv "$scratch/out/shot.png" "$scratch/out/$1.new.png" 2>/dev/null || true
        if ! "$python" - "$scratch/out/$1.old.png" "$scratch/out/$1.new.png" <<'PY'
import sys
from PIL import Image, ImageChops
try:
    a, b = (Image.open(p).convert('RGBA') for p in sys.argv[1:3])
except FileNotFoundError as e:
    print('missing screenshot', e.filename); sys.exit(1)
if a.size != b.size:
    print('size differs', a.size, b.size); sys.exit(1)
box = ImageChops.difference(a, b).getbbox()
if box:
    diff = sum(1 for p in ImageChops.difference(a, b).getdata() if p != (0, 0, 0, 0))
    print(f'{diff} pixels differ in {box}'); sys.exit(1)
PY
        then echo "DIFF screenshot $1"; failures=$((failures + 1)); fi
    }
    common=(OMAREEL_SCREENSHOT="$scratch/out/shot.png" OMAREEL_SCREENSHOT_DELAY_MS=2500 OMAREEL_SCREENSHOT_SIZE=1440x900)
    EXTRA_ENV=("${common[@]}"); fresh; run old warm-up edit "$scratch/bundles/with-camera"
    EXTRA_ENV=("${common[@]}"); shot editor edit "$scratch/bundles/with-camera"
    for panel in background shape cursor zoom clip camera keystrokes audio; do
        EXTRA_ENV=("${common[@]}" OMAREEL_SCREENSHOT_PANEL="$panel"); shot "editor-$panel" edit "$scratch/bundles/with-camera"
    done
    EXTRA_ENV=("${common[@]}" OMAREEL_SCREENSHOT_SEEK=3.5); shot editor-seek edit "$scratch/bundles/with-camera"
    EXTRA_ENV=("${common[@]}"); shot launcher
else
    echo "skipping screenshots: Pillow is not importable by $python (set PYTHON to a venv's python)"
fi

if ((failures)); then echo "$failures difference(s)"; exit 1; fi
echo "no differences"
