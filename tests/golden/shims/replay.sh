# Sourced by the program shims (gpu-screen-recorder, slurp, omarchy-capture-region, pactl, ffmpeg,
# and hyprctl when a replay directory is set): `replay <program> "$@"` answers one call from
# $OMAREEL_ORACLE_REPLAY_DIR and never returns.
#
# The answer for a call is named by its key: the program name and every argument, each preceded by
# a space, with % written %25 and / written %2F. The n-th call with a key (counted in .calls/)
# answers from <key>.<n>.* when <key>.<n>.status exists, and from <key>.* otherwise; a call with
# neither answers from <program>.any.* when that exists (for arguments holding scratch paths).
# <key>.status holds the exit status and must exist (a call without any answer exits 65);
# <key>.stdout and <key>.stderr, when present, are printed as they
# are; <key>.creates, when present, is copied to the path given as the last argument first;
# <key>.stdin makes the shim read its standard input to the end (otherwise it is left alone, so a
# caller that never closes it does not hang the shim); <key>.hang makes it sleep that many seconds
# and exit without answering, for the callers' timeouts.
#
# When $OMAREEL_ORACLE_REPLAY_LOG is set, every call appends one line to it: the program and its
# arguments joined by tabs. Standard input read because of <key>.stdin goes to
# $OMAREEL_ORACLE_REPLAY_LOG.<n>.stdin, n being the number of lines the log had before the call.
replay() {
    replay_program=$1
    shift
    replay_key=$replay_program
    for replay_argument in "$@"; do replay_key="$replay_key $replay_argument"; done
    replay_key=$(printf '%s' "$replay_key" | sed 's/%/%25/g; s|/|%2F|g')
    replay_count=0
    if [ -n "${OMAREEL_ORACLE_REPLAY_LOG:-}" ]; then
        [ -f "$OMAREEL_ORACLE_REPLAY_LOG" ] && replay_count=$(wc -l < "$OMAREEL_ORACLE_REPLAY_LOG")
        # One write per call, so calls made at the same time do not interleave their lines.
        replay_line=$replay_program
        for replay_argument in "$@"; do replay_line="$replay_line	$replay_argument"; done
        printf '%s\n' "$replay_line" >> "$OMAREEL_ORACLE_REPLAY_LOG"
    fi
    replay_answer="${OMAREEL_ORACLE_REPLAY_DIR:?}/$replay_key"
    # Calls are counted per key in .calls/<cksum of the key> (keys can be longer than a file name).
    replay_counter="$OMAREEL_ORACLE_REPLAY_DIR/.calls/$(printf '%s' "$replay_key" | cksum | cut -d ' ' -f 1)"
    mkdir -p "$OMAREEL_ORACLE_REPLAY_DIR/.calls"
    replay_call=1
    [ -f "$replay_counter" ] && replay_call=$(( $(cat "$replay_counter") + 1 ))
    printf '%s\n' "$replay_call" > "$replay_counter"
    [ -f "$replay_answer.$replay_call.status" ] && replay_answer="$replay_answer.$replay_call"
    if [ ! -f "$replay_answer.status" ] && [ -f "$OMAREEL_ORACLE_REPLAY_DIR/$replay_program.any.status" ]; then
        replay_answer="$OMAREEL_ORACLE_REPLAY_DIR/$replay_program.any"
    fi
    if [ ! -f "$replay_answer.status" ]; then
        echo "$replay_program shim: no answer for: $replay_key" >&2
        exit 65
    fi
    if [ -f "$replay_answer.stdin" ]; then
        if [ -n "${OMAREEL_ORACLE_REPLAY_LOG:-}" ]; then
            cat > "$OMAREEL_ORACLE_REPLAY_LOG.$replay_count.stdin"
        else
            cat > /dev/null
        fi
    fi
    if [ -f "$replay_answer.hang" ]; then
        exec sleep "$(cat "$replay_answer.hang")"
    fi
    if [ -f "$replay_answer.creates" ]; then
        for replay_argument in "$@"; do :; done
        cat "$replay_answer.creates" > "$replay_argument"
    fi
    [ -f "$replay_answer.stdout" ] && cat "$replay_answer.stdout"
    [ -f "$replay_answer.stderr" ] && cat "$replay_answer.stderr" >&2
    exit "$(cat "$replay_answer.status")"
}
