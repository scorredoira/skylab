#!/bin/bash
# Takes again the pictures the menus show, from the games themselves: each aircraft's beauty shot
# into assets/aircraft/<model>/picture.jpg (the aircraft view, the start screen, the settings), the
# featured places' and the race courses' into assets/menu/places/<key>.jpg (the place picker), and
# the first screen's three into assets/menu/vehicles/<planes|drones|cars>.jpg (the cars' is the
# driving game's). Run it when an aircraft's model changes, or to add the picture of a new aircraft
# or place.
#
# Each game runs alone (one at a time, under the lock $FLIGHT_GPU_LOCK if it is set) and is killed
# with -9 if its memory passes 4 GB. It reads no settings of the player's (a scratch
# XDG_CONFIG_HOME) and the real places are downloaded into its own cache (target/menu-shots/cache,
# or $FLIGHT_CACHE), never the player's: some 400 MB the first time, for the twelve places.
#
# Usage: scripts/menu-shots.sh              every picture
#        scripts/menu-shots.sh c172 LEMH    only those (an aircraft's folder, a place's code, a course's key,
#                                           generated, planes, drones, cars)
cd "$(dirname "$0")/.." || exit 1
WORK=target/menu-shots
mkdir -p assets/menu/places assets/menu/vehicles "$WORK"
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

# shot <name> <out.jpg> <width> <x,y,w,h> <game flags...>: a 1600 x 1000 picture of the game with
# nothing written over it, that box of it cut out and made <width> pixels wide.
shot() {
  local name=$1 out=$2 width=$3 box=$4
  shift 4
  if [ -n "$ONLY" ] && [[ " $ONLY " != *" $name "* ]]; then return; fi
  echo "== $name: $*"
  capped ./target/release/skylab --shot "$WORK/$name" --clean "$@" >/dev/null || exit 1
  ./target/release/examples/shrink "$WORK/$name/scene.png" "$out" "$width" 84 "$box" || exit 1
}

# aircraft <folder> <game flags...>: its picture, on the runway of the generated world in the
# evening, from the side the low sun lights. The camera looks at the aircraft, in the middle of the
# shot; the box cut out leaves it up and to the right, clear of what the menu writes over the foot
# of the picture.
aircraft() {
  local name=$1
  shift
  shot "$name" "assets/aircraft/$name/picture.jpg" 1280 0,330,1130,665 --aircraft "$name" --ext --hour 18.2 --sky fair "$@"
}

# place <code> <game flags...>: a featured place's picture, from the air over it, less the strip
# at the top of the shot where the maps' credit is.
place() {
  local code=$1
  shift
  shot "$code" "assets/menu/places/$code.jpg" 640 0,40,1600,960 --place "$code" --aircraft c172 --ext --sky fair "$@"
}

# vehicle <planes|drones|cars> <x,y,w,h> <game flags...>: a picture of the first screen, square,
# with its subject in the middle (the screen cuts it at the sides or at the top and bottom, as
# the window is). The shot is 2880 x 1800, that box of it cut out and made 1200 pixels wide. The
# cars' is the driving game's.
vehicle() {
  local name=$1 box=$2
  shift 2
  if [ -n "$ONLY" ] && [[ " $ONLY " != *" $name "* ]]; then return; fi
  echo "== $name: $*"
  if [ "$name" = cars ]; then
    capped ./target/release/skylab-drive --shot "$WORK/$name" --size 2880x1800 "$@" >/dev/null || exit 1
    ./target/release/examples/shrink "$WORK/$name/drive.png" "assets/menu/vehicles/$name.jpg" 1200 84 "$box" || exit 1
  else
    capped ./target/release/skylab --shot "$WORK/$name" --clean --retina "$@" >/dev/null || exit 1
    ./target/release/examples/shrink "$WORK/$name/scene.png" "assets/menu/vehicles/$name.jpg" 1200 84 "$box" || exit 1
  fi
}

locked cargo build --release -p flight -p drive || exit 1
locked cargo build --release -p engine --example shrink || exit 1
ONLY="$*"
# The aircraft: from ahead and to one side, a little above, close.
aircraft c172 --look -150 8 --zoom 4.5
aircraft extra300 --look -150 8 --zoom 7
# The quads: on the runway, from as near as the camera goes.
aircraft fpv5 --look -150 16 --zoom 12
aircraft race5 --look -150 16 --zoom 12
# The tilt-rotor: in the air over the aerodrome, its nacelles on their way forward.
aircraft kestrel --air --nacelles 0.6 --wait 4 --look -140 14 --zoom 3
# The seed's world, and the race courses in it through the racing quad's camera, a few seconds into a lap.
shot generated assets/menu/places/generated.jpg 640 0,40,1600,960 --aircraft c172 --air --ext --hour 17.5 --sky fair --look 25 12 --zoom -6
shot deepwood assets/menu/places/deepwood.jpg 640 0,40,1600,960 --course deepwood --racer --wait 7 --hour 10 --sky fair
shot pinewood assets/menu/places/pinewood.jpg 640 0,40,1600,960 --course pinewood --racer --wait 3.1 --hour 10 --sky fair
# The featured places (geodata::PRESETS), each from where it shows best.
place LEMH --air --hour 18 --look 20 14 --zoom -6
place LOWI --air --hour 17 --look 20 10 --zoom -6
place LEBL --air --hour 18.5 --look 20 14 --zoom -6
place ENSG --air --hour 16 --look 20 10 --zoom -6
place ENLK --air --hour 16 --look 20 10 --zoom -6
place TNCS --air --hour 17 --look 20 12 --zoom -6
place VNLK --air --hour 9 --look 20 8 --zoom -6
place LFLJ --air --hour 16 --look 20 10 --zoom -6
place LPMA --air --hour 18 --look 20 12 --zoom -6
place VHHH --air --hour 18 --look 20 14 --zoom -6
place KSFO --air --hour 18 --look 20 14 --zoom -6
place NZQN --air --hour 16 --look 20 10 --zoom -6
# The first screen: the Cessna over the fields at dusk, the racing quad at Pinewood's start gate,
# and the GT on the main straight of Lakeside Park in the evening, the racing driver driving.
vehicle planes 486,72,1728,1728 --aircraft c172 --air --low 350 --ext --look -150 6 --zoom 0.5 --hour 18.2 --sky fair
vehicle drones 612,72,1728,1728 --course pinewood --racer --wait 3.1 --ext --look -20 6 --zoom 8 --hour 17 --sky fair
vehicle cars 657,342,1458,1458 --driver --at 330 --wait 1 --orbit 160 3 8.5 --hour 18.2
du -ch assets/aircraft/*/picture.jpg assets/menu/places/*.jpg assets/menu/vehicles/*.jpg | tail -1
