#!/usr/bin/env bash
# Regenerates tests/golden/expected/ from the C++ code at the tag rust-migration-base.
#
#   tools/oracle/generate-golden.sh [--check] [--allow-arch] [--keep]
#
#   --check       compare against the committed tests/golden/expected/ instead of replacing it;
#                 exits 1 and prints a diff summary when they differ
#   --allow-arch  run on a machine that is not x86_64 (the goldens define float behaviour on
#                 x86_64; elsewhere qRound and libm may differ, so only use this to experiment)
#   --keep        keep the scratch directory (build, hermetic HOME) and print where it is
#
# The C++ sources come from the tag itself (git archive), never from the working tree, and the
# oracle runs with a fresh HOME and XDG directories, TZ=UTC, LC_ALL=C, the offscreen Qt platform
# and tests/golden/shims first on PATH. tests/golden/README.md describes every output file.
set -euo pipefail

check=0
allow_arch=0
keep=0
for argument in "$@"; do
    case "$argument" in
        --check) check=1 ;;
        --allow-arch) allow_arch=1 ;;
        --keep) keep=1 ;;
        -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "generate-golden.sh: unknown argument $argument" >&2; exit 2 ;;
    esac
done

tag=rust-migration-base
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
golden="$repo/tests/golden"

if ! git -C "$repo" rev-parse -q --verify "refs/tags/$tag^{commit}" >/dev/null; then
    echo "generate-golden.sh: the tag $tag does not exist in $repo (git fetch --tags?)" >&2
    exit 1
fi
arch=$(uname -m)
if [[ $arch != x86_64 && $allow_arch -eq 0 ]]; then
    echo "generate-golden.sh: float parity is defined on x86_64 and this machine is $arch;" \
         "pass --allow-arch to run anyway (do not commit the result)" >&2
    exit 1
fi
if [[ $(id -u) -eq 0 ]]; then
    echo "generate-golden.sh: run as an ordinary user; the goldens record permission errors" >&2
    exit 1
fi
for tool in cmake c++ tar; do
    command -v "$tool" >/dev/null || { echo "generate-golden.sh: $tool is required" >&2; exit 1; }
done

umask 022
tmp=$(mktemp -d "${TMPDIR:-/tmp}/omareel-golden.XXXXXXXX")
cleanup() {
    if [[ $keep -eq 1 ]]; then
        echo "kept $tmp"
    else
        chmod -R u+rwx "$tmp" 2>/dev/null || true
        rm -rf "$tmp"
    fi
}
trap cleanup EXIT

mkdir -p "$tmp/base"
git -C "$repo" archive "$tag" src | tar -x -C "$tmp/base"

generator=()
command -v ninja >/dev/null && generator=(-G Ninja)
if ! cmake -S "$repo/tools/oracle" -B "$tmp/build" "${generator[@]}" \
        -DOMAREEL_BASE="$tmp/base" -DCMAKE_BUILD_TYPE=Release > "$tmp/configure.log" 2>&1; then
    cat "$tmp/configure.log" >&2
    exit 1
fi
if ! cmake --build "$tmp/build" --parallel "$(nproc 2>/dev/null || echo 4)" > "$tmp/build.log" 2>&1; then
    cat "$tmp/build.log" >&2
    exit 1
fi

run="$tmp/run"
mkdir -p "$run"/{home,config,cache,state,data,runtime,tmp}
chmod 700 "$run/runtime"
output="$tmp/expected"
env -i \
    HOME="$run/home" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_STATE_HOME="$run/state" \
    XDG_DATA_HOME="$run/data" \
    XDG_RUNTIME_DIR="$run/runtime" \
    TMPDIR="$run/tmp" \
    TZ=UTC \
    LC_ALL=C \
    LANG=C \
    QT_QPA_PLATFORM=offscreen \
    PATH="$golden/shims:/usr/bin:/bin" \
    OMAREEL_ORACLE_HERMETIC=1 \
    "$tmp/build/omareel-oracle" "$golden/corpus" "$output"

if [[ $check -eq 1 ]]; then
    if diff -r -q "$golden/expected" "$output" > "$tmp/diff.txt"; then
        echo "tests/golden/expected/ matches $tag"
        exit 0
    fi
    echo "tests/golden/expected/ differs from what $tag produces:" >&2
    sed "s|$output|<generated>|g; s|$golden/expected|tests/golden/expected|g" "$tmp/diff.txt" >&2
    exit 1
fi

# Swap the new tree in with renames on the same filesystem, so expected/ is never half-written.
staging="$golden/.expected.new.$$"
rm -rf "$staging"
cp -R "$output" "$staging"
if [[ -d $golden/expected ]]; then
    if mv --exchange -T "$staging" "$golden/expected" 2>/dev/null; then
        rm -rf "$staging"
    else
        old="$golden/.expected.old.$$"
        mv -T "$golden/expected" "$old"
        mv -T "$staging" "$golden/expected"
        rm -rf "$old"
    fi
else
    mv -T "$staging" "$golden/expected"
fi
echo "wrote tests/golden/expected/ ($(du -sh "$golden/expected" | cut -f1)) from $tag"
