#!/bin/sh
# `omareel agent` end to end with tests/tools/fake-agent.py standing in for Claude Code and
# Codex: the real omareel describe/apply/frame run in a synthetic bundle, and the events,
# project.json and chapters.json come out as an agent's edits would leave them.
# Usage: agent_fake_e2e.sh OMAREEL SOURCE_DIR
set -eu
omareel=$1
source=$2
work=$(mktemp -d "${TMPDIR:-/tmp}/omareel-agent-e2e.XXXXXX")
trap 'rm -rf "$work"' EXIT
export XDG_CACHE_HOME="$work/cache"
export OMAREEL_AGENT_PROGRAM="$source/tests/tools/fake-agent.py"
fail() { echo "FAIL: $*" >&2; exit 1; }

bundle="$work/Demo.omareel"
mkdir -p "$bundle"
cp "$source/tests/fixtures/synthetic/capture.json" "$source/tests/fixtures/synthetic/input.jsonl" "$bundle/"
ffmpeg -v error -f lavfi -i testsrc2=size=640x360:rate=60 -t 8 -c:v libx264 -pix_fmt yuv420p "$bundle/screen.mp4"

# Polish as Claude Code: one apply (cuts and keystrokes), a contact sheet, the end.
OMAREEL_AGENT=claude "$omareel" agent "$bundle" polish > "$work/polish.jsonl" || { cat "$work/polish.jsonl"; fail "polish exited $?"; }
cat "$work/polish.jsonl"
grep -q '"type":"started"' "$work/polish.jsonl" || fail "no start"
grep -q '"ok":true,"output_duration":6.5,"text":"Applied delete, set' "$work/polish.jsonl" || fail "no apply"
grep -q '"type":"image"' "$work/polish.jsonl" || fail "no contact sheet"
grep -q '"cost_usd":0.0123' "$work/polish.jsonl" || fail "no cost"
"$omareel" describe "$bundle" > "$work/describe.json"
grep -q '"duration": 6.5' "$work/describe.json" || fail "the cut is not in project.json"
python3 - "$bundle/project.json" <<'PY' || fail "keystrokes are not on"
import json, sys
assert json.load(open(sys.argv[1]))["keystrokes"]["enabled"] is True
PY

# Chapters as Codex: set-chapters through apply, at source times.
OMAREEL_AGENT=codex "$omareel" agent "$bundle" chapters > "$work/chapters.jsonl" || { cat "$work/chapters.jsonl"; fail "chapters exited"; }
cat "$work/chapters.jsonl"
grep -q '"text":"Applied set-chapters' "$work/chapters.jsonl" || fail "no set-chapters"
"$omareel" describe "$bundle" > "$work/describe.json"
python3 - "$work/describe.json" <<'PY' || fail "describe has no chapters"
import json, sys
chapters = json.load(open(sys.argv[1]))["chapters"]
assert chapters["title"] == "A synthetic test", chapters
# Source 0 was cut (0-1.5): the first chapter starts the video; source 4.4 plays at 2.9.
assert [c["output"] for c in chapters["chapters"]] == [0, 2.9], chapters
PY

# Inside an agent's scope the real omareel runs only describe, apply, frame into the scratch
# directory, and transcribe, on this bundle, and help; it refuses the rest before doing anything.
scratch="$work/scratch"
other="$work/Other.omareel"
mkdir -p "$scratch" "$other"
ln -s "$other" "$bundle/sneaky"
scope="$bundle
$scratch"
scoped() { (cd "$bundle" && OMAREEL_AGENT_SCOPE="$scope" "$omareel" "$@"); }
refused() {
    if scoped "$@" > "$work/refused.txt" 2>&1; then fail "allowed in scope: $*"; fi
    grep -q "omareel: refused:" "$work/refused.txt" || fail "$* failed without refusing: $(cat "$work/refused.txt")"
}
scoped help > /dev/null || fail "help refused"
scoped describe . > /dev/null || fail "describe refused"
scoped apply . '[]' --dry-run > /dev/null || fail "apply refused"
scoped frame . --at 0.5 -o "$scratch/still.png" 2> /dev/null || fail "frame into the scratch directory refused"
test -s "$scratch/still.png" || fail "no still"
refused
refused record
refused record --region
refused export . -o "$scratch/out.mp4"
refused export . -o "$work/out.mp4"
refused edit .
refused skill install
refused agent . polish
refused __record-daemon
refused describe ../Other.omareel
refused apply "$other" '[]'
refused apply sneaky '[]'
refused frame . --at 0.5 -o "$work/still.png"
refused frame . --at 0.5 -o "$scratch/../still.png"
refused transcribe ../Other.omareel
test ! -e "$work/out.mp4" -a ! -e "$work/still.png" || fail "a refused command wrote a file"
# A scope that names nothing that exists refuses everything.
if (cd "$bundle" && OMAREEL_AGENT_SCOPE="/nonexistent" "$omareel" describe . > /dev/null 2>&1); then fail "a broken scope allowed describe"; fi

# An agent omareel can't run says so, and runs nothing.
if OMAREEL_AGENT=opencode "$omareel" agent "$bundle" polish 2> "$work/unsupported.txt"; then fail "opencode ran"; fi
grep -q "OpenCode, which omareel can't run" "$work/unsupported.txt" || fail "$(cat "$work/unsupported.txt")"
echo "agent e2e ok"
