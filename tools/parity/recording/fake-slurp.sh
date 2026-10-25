#!/bin/sh
# slurp without a person: -r answers the first rectangle it was given, otherwise the region in
# $PARITY_DIR/region-answer.
for argument in "$@"; do [ "$argument" = "-r" ] && restricted=1; done
if [ -n "$restricted" ]; then
    head -n 1 | awk '{print $1, $2}'
else
    cat > /dev/null
    cat "$PARITY_DIR/region-answer"
fi
