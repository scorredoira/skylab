#!/bin/bash
# The drive network's imitation and DAgger rounds (trainer/DRIVE.md): the racing driver's data,
# a clone of it, then rounds of DAgger, each round's network scored on 40 drives (20 dry, 20 wet).
# Run from the repository's root after `cargo build --release -p sim-env --examples --bins` and
# `(cd trainer && swift build -c release)`. Usage: trainer/drive-pipeline.sh <runs dir> [threads] [rounds]
set -e
RUNS=${1:?runs dir}; T=${2:-12}; ROUNDS=${3:-8}
D=target/release/examples/drive; FT=trainer/.build/release/flight-train
mkdir -p "$RUNS"
log() { echo "$(date +%H:%M:%S) $*" | tee -a "$RUNS/pipeline.log"; }
if [ ! -f "$RUNS/d0.bin" ]; then
  log "data 0: the racing driver"
  $D data --out "$RUNS/d0.bin" --dt 0.05 --episodes 400 --threads $T | tee -a "$RUNS/pipeline.log"
fi
if [ ! -f "$RUNS/bc0/model.fltmodel" ]; then
  log "imitate 0"
  $FT imitate --task drive --data "$RUNS/d0.bin" --out "$RUNS/bc0" --hidden 256,256 --epochs 30 --dt 0.05 > "$RUNS/bc0.log" 2>&1
  $D score "$RUNS/bc0/model.fltmodel" --episodes 20 --first-seed 5000 --threads $T | tee -a "$RUNS/pipeline.log"
fi
DATA="$RUNS/d0.bin"
for k in $(seq 1 $ROUNDS); do
  DATA="$DATA,$RUNS/d$k.bin"
  [ -f "$RUNS/bc$k/model.fltmodel" ] && continue
  HIDDEN=256,256; [ $k -ge 3 ] && HIDDEN=512,512
  log "DAgger round $k"
  $D data --out "$RUNS/d$k.bin" --dt 0.05 --episodes 300 --first-seed $((100000 + 1000 * k)) --model "$RUNS/bc$((k - 1))/model.fltmodel" --beta 0 --threads $T | tee -a "$RUNS/pipeline.log"
  $FT imitate --task drive --data "$DATA" --out "$RUNS/bc$k" --hidden $HIDDEN --epochs 30 --dt 0.05 > "$RUNS/bc$k.log" 2>&1
  $D score "$RUNS/bc$k/model.fltmodel" --episodes 20 --first-seed 5000 --threads $T | tee -a "$RUNS/pipeline.log"
done
log "done"
