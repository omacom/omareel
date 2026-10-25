#!/usr/bin/env bash
# Writes tests/golden/corpus/project/history-*.json: the project.json that Project::defaults()
# saved at every commit up to rust-migration-base that changed src/core/Project.cpp. Each one
# is built from that commit's Project.h and Project.cpp alone, so the files are the exact bytes
# those versions of omareel wrote for a new recording. Run it once when the corpus is created;
# the goldens are generated from the files it leaves, not from this script.
set -euo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
git -C "$repo" rev-parse -q --verify 'rust-migration-base^{commit}' >/dev/null \
    || { echo "tag rust-migration-base is missing" >&2; exit 1; }
out="$repo/tests/golden/corpus/project"
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/driver.cpp" <<'CPP'
#include "core/Project.h"
#include <QCoreApplication>
#include <cstdlib>
using namespace PROJECT_NAMESPACE;
int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    QString error;
    const Project project = Project::defaults(QString::fromUtf8(argv[2]), std::atof(argv[3]));
    return project.save(QString::fromUtf8(argv[1]), &error) ? 0 : 1;
}
CPP

flags=$(pkg-config --cflags --libs Qt6Core Qt6Gui)
index=0
for rev in $(git -C "$repo" log --reverse --format=%h rust-migration-base -- src/core/Project.cpp); do
    index=$((index + 1))
    dir="$tmp/$rev"
    mkdir -p "$dir/src/core"
    git -C "$repo" show "$rev:src/core/Project.h" > "$dir/src/core/Project.h"
    git -C "$repo" show "$rev:src/core/Project.cpp" > "$dir/src/core/Project.cpp"
    namespace=Omareel
    grep -q 'namespace OmaRecord' "$dir/src/core/Project.h" && namespace=OmaRecord
    # shellcheck disable=SC2086
    g++ -std=c++17 -fPIC -O1 -w -DPROJECT_NAMESPACE=$namespace -I"$dir/src" \
        "$tmp/driver.cpp" "$dir/src/core/Project.cpp" -o "$dir/driver" $flags
    date=$(git -C "$repo" show -s --format=%cs "$rev")
    name=$(printf 'history-%02d-%s-%s.json' "$index" "$date" "$rev")
    "$dir/driver" "$out/$name" "Recording $(printf '%02d' "$index")" 8.25
    echo "$name"
done
