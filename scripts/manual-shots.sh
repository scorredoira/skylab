#!/bin/bash
# Takes again the pictures of the manual (F1) from the game itself, into assets/manual: run it when
# the cockpit or the screens they show change. The boxes and labels over them are not in the
# pictures: they are `crates/games/flight/src/manual/pages.rs`'s, in fractions of each picture, so
# look at the manual afterwards (`--manual <page> --shot <dir>`) and move any that no longer fit.
#
# Each game runs alone (one at a time, under the lock $FLIGHT_GPU_LOCK if it is set) and is killed
# with -9 if its memory passes 4 GB. It reads no settings or controls of the player's (a scratch
# XDG_CONFIG_HOME) and the place picker downloads into its own cache (target/manual-shots/cache,
# or $FLIGHT_CACHE), never the player's.
#
# Usage: scripts/manual-shots.sh            every picture
#        scripts/manual-shots.sh pfd seat    only those
cd "$(dirname "$0")/.." || exit 1
OUT=assets/manual
WORK=target/manual-shots
mkdir -p "$OUT" "$WORK"
export XDG_CONFIG_HOME="$WORK/config"
export FLIGHT_CACHE="${FLIGHT_CACHE:-$WORK/cache}"
mkdir -p "$XDG_CONFIG_HOME" "$FLIGHT_CACHE"
LIMIT_KB=$((4 * 1024 * 1024))

# Runs a command under the GPU lock, if there is one.
locked() {
  if [ -z "$FLIGHT_GPU_LOCK" ]; then "$@"; elif command -v lockf >/dev/null; then lockf "$FLIGHT_GPU_LOCK" "$@"; else flock "$FLIGHT_GPU_LOCK" "$@"; fi
}

# Runs a command under the GPU lock and the 4 GB cap.
capped() {
  locked "$@" &
  local pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    # The game is the lock's child when there is a lock: its memory is the tree's.
    local rss
    rss=$(ps -o rss= -p "$pid" $(pgrep -P "$pid") 2>/dev/null | awk '{s += $1} END {print s + 0}')
    if [ "$rss" -gt "$LIMIT_KB" ]; then
      pkill -9 -P "$pid"
      kill -9 "$pid"
      echo "killed: $((rss / 1024)) MB, over 4 GB" >&2
      return 1
    fi
    sleep 0.5
  done
  wait "$pid"
}

# shot <name> <width> <crop x,y,w,h or -> <game flags...>: a 1600 x 1000 picture of the game, cut
# and made <width> pixels wide as assets/manual/<name>.jpg.
shot() {
  local name=$1 width=$2 crop=$3
  shift 3
  if [ -n "$ONLY" ] && [[ " $ONLY " != *" $name "* ]]; then return; fi
  echo "== $name: $*"
  capped ./target/release/skylab --shot "$WORK/$name" "$@" >/dev/null || exit 1
  if [ "$crop" = - ]; then crop=""; fi
  ./target/release/examples/shrink "$WORK/$name/scene.png" "$OUT/$name.jpg" "$width" 80 $crop || exit 1
}

locked cargo build --release -p flight || exit 1
locked cargo build --release -p engine --example shrink || exit 1
ONLY="$*"
# The C172 on the runway and on final, leaning in: the outside and the PFD.
shot seat 1200 - --aircraft c172 --hour 10 --spot 1
shot final 1200 - --aircraft c172 --final --hour 10 --spot 1
# Every spot of the C172's cockpit, small, and the outside view.
spots=(pilot leaning panel pfd standby mfd switches levers)
for i in "${!spots[@]}"; do
  shot "spot-${spots[$i]}" 400 - --aircraft c172 --air --hour 10 --spot "$i"
done
shot spot-exterior 400 - --aircraft c172 --air --hour 10 --ext
# The levers, and the PFD flying on the autopilot: heading bug 40° right, HDG and ALT, 1000 ft to climb to.
shot levers 1000 - --aircraft c172 --hour 10 --spot 7
shot pfd 1200 - --aircraft c172 --air --hour 10 --spot 3 --afcs bug=+40,hdg,alt,sel=+1000,ap --wait 5
# The FPV quad hovering over the woods north of the aerodrome, through its camera, its display on.
shot fpv 1200 - --aircraft fpv --pos 4391 11000 --low 26 --hour 10.5
# The place picker over Menorca, its aerodrome chosen.
shot picker 1200 - --picker --map 39.95,4.05,8.6 --select LEMH
du -ch "$OUT"/*.jpg | tail -1
