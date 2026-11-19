#!/bin/bash
# Builds both games (release) and runs the driving game. Its pause menu's "Change vehicle" (Esc)
# starts Skylab in its place, on its first screen.
# Usage: scripts/drive.sh [game flags...]   e.g. scripts/drive.sh --hour 18 --view cockpit
#        scripts/drive.sh --shot /tmp/shots --driver --at 600 --wait 2
# With SKYLAB_CAP_GB set (for unattended runs), the game is killed with -9 if its memory passes it.
cd "$(dirname "$0")/.." || exit 1
cargo build --release -p drive -p flight || exit 1
exec scripts/cap.sh ./target/release/skylab-drive "$@"
