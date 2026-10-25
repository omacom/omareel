#!/usr/bin/env bash
# Usage: import-omarchy-themes.sh [omarchy-root, default /usr/share/omarchy]
# Copies every installed Omarchy theme's colors.toml into tests/golden/corpus/themes/<name>/
# and renders the shell.toml Omarchy would put in ~/.local/state/omarchy/current/theme/ for
# it, by running Omarchy's own omarchy-theme-set-templates in a scratch HOME (the theme's
# shell.<section>.toml overrides included). Run it once when the corpus is created or when
# Omarchy's themes should be refreshed on purpose; the goldens use the copied files.
set -euo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
omarchy=${1:-/usr/share/omarchy}
[[ -x $omarchy/bin/omarchy-theme-set-templates ]] \
    || { echo "Omarchy is not installed at $omarchy" >&2; exit 1; }
out="$repo/tests/golden/corpus/themes"
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for theme in "$omarchy"/themes/*/; do
    name=$(basename "$theme")
    [[ -f $theme/colors.toml ]] || continue
    home="$tmp/$name"
    next="$home/.local/state/omarchy/current/next-theme"
    mkdir -p "$next"
    find "$theme" -maxdepth 1 -type f ! -name '*.png' -exec cp {} "$next/" \;
    env -i HOME="$home" OMARCHY_PATH="$omarchy" PATH="$omarchy/bin:/usr/bin:/bin" LC_ALL=C \
        bash "$omarchy/bin/omarchy-theme-set-templates"
    mkdir -p "$out/$name"
    cp "$theme/colors.toml" "$out/$name/colors.toml"
    cp "$next/shell.toml" "$out/$name/shell.toml"
    printf '%s\n' "$name" > "$out/$name/theme.name"
done
cp "$omarchy/version" "$out/OMARCHY_VERSION"
