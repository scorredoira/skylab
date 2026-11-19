#!/bin/bash
# Runs a program; with SKYLAB_CAP_GB set, kills it with -9 if its memory passes that many GB.
# Used by run.sh and drive.sh.
if [ -z "$SKYLAB_CAP_GB" ]; then
  exec "$@"
fi
LIMIT_KB=$((SKYLAB_CAP_GB * 1024 * 1024))
"$@" &
PID=$!
while kill -0 "$PID" 2>/dev/null; do
  RSS=$(ps -o rss= -p "$PID" | tr -d ' ')
  if [ -n "$RSS" ] && [ "$RSS" -gt "$LIMIT_KB" ]; then
    kill -9 "$PID"
    echo "$1 killed: $((RSS / 1024)) MB, over $SKYLAB_CAP_GB GB" >&2
    break
  fi
  sleep 0.5
done
wait "$PID" 2>/dev/null
