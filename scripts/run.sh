#!/bin/bash
# Builds both games (release) and runs Skylab, which opens on its first screen (planes, drones or
# cars; "Cars" shows the driving game in the same window).
# Usage: scripts/run.sh [game flags...]   e.g. scripts/run.sh --air
# With SKYLAB_CAP_GB set (for unattended runs), the game is killed with -9 if its memory passes it.
cd "$(dirname "$0")/.." || exit 1
cargo build --release -p flight -p drive || exit 1
exec scripts/cap.sh ./target/release/skylab "$@"
