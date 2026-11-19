#!/bin/bash
# PPO fine-tuning of the drive network from its last DAgger round (trainer/DRIVE.md), then every
# checkpoint scored on 40 drives (20 dry, 20 wet) of other seeds.
# Usage: trainer/drive-ppo.sh <runs dir> <init model> [threads] [steps]
set -e
RUNS=${1:?runs dir}; INIT=${2:?init model}; T=${3:-12}; STEPS=${4:-10000000}
D=target/release/examples/drive; FT=trainer/.build/release/flight-train
$FT --task drive --init "$INIT" --fresh-critic --critic-warmup 15 \
    --set config.wet_share=0.5 --set config.time_limit=220 \
    --envs 128 --rollout 256 --minibatches 8 --epochs 4 --lr 1e-5 --lr-end 1e-6 --gamma 0.995 \
    --log-std -3 --target-kl 0.02 --hidden 512,512 --steps $STEPS --threads $T --device cpu \
    --checkpoint-every 20 --out "$RUNS/ppo1" > "$RUNS/ppo1.log" 2>&1
for m in "$RUNS"/ppo1/checkpoint-*.fltmodel; do
  echo "$m" >> "$RUNS/ppo-scores.log"
  $D score "$m" --episodes 20 --first-seed 6000 --threads $T | grep -E "dry|wet|all" >> "$RUNS/ppo-scores.log"
done
echo done >> "$RUNS/ppo-scores.log"
