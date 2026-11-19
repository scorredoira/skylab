#!/bin/bash
# Makes the video of a recording (`skylab --record <dir> --script <file>`): its shots' frames, in the
# order given, joined by hard cuts or by crossfades, to H.264 at 60 fps (or --fps), yuv420p, for Twitter/X.
# If every shot has its sound (`<dir>/<shot>.wav`, a script with `sound: true`), the sounds are joined
# the same way and go under the pictures as AAC; otherwise the video is silent.
# Usage: scripts/video/assemble.sh <dir> <out.mp4> [--fps <n>] [--fade <s>] [--crop <w>:<h>] <shot> [<shot>...]
# (--fps: the script's frames a second, 60 unless given; --crop: the middle of the picture, so many
# pixels wide and high: 1080:1080 of a 1920 x 1080 recording is a square one).
set -euo pipefail
DIR=$1
OUT=$2
shift 2
FPS=60
if [ "${1:-}" = "--fps" ]; then
  FPS=$2
  shift 2
fi
FADE=0
if [ "${1:-}" = "--fade" ]; then
  FADE=$2
  shift 2
fi
CROP=""
if [ "${1:-}" = "--crop" ]; then
  CROP="crop=$2,"
  shift 2
fi
[ $# -gt 0 ] || { echo "no shots given" >&2; exit 1; }

INPUTS=()
SOUND=1
for shot in "$@"; do
  INPUTS+=(-framerate "$FPS" -i "$DIR/$shot/%05d.png")
  [ -f "$DIR/$shot.wav" ] || SOUND=0
done
if [ "$SOUND" = 1 ]; then
  for shot in "$@"; do
    INPUTS+=(-i "$DIR/$shot.wav")
  done
fi

AUDIO=""
if [ "$FADE" = "0" ] || [ $# -eq 1 ]; then
  FILTER=""
  for i in $(seq 0 $(($# - 1))); do FILTER+="[$i:v]"; done
  FILTER+="concat=n=$#:v=1:a=0,${CROP}format=yuv420p[v]"
  if [ "$SOUND" = 1 ]; then
    for i in $(seq $# $((2 * $# - 1))); do AUDIO+="[$i:a]"; done
    AUDIO=";${AUDIO}concat=n=$#:v=0:a=1[a]"
  fi
else
  # Each crossfade starts where the video so far ends, less the fade.
  FILTER=""
  PREV="[0:v]"
  APREV="[$#:a]"
  LENGTH=$(ls "$DIR/$1" | grep -c '\.png$')
  LENGTH=$(echo "$LENGTH / $FPS" | bc -l)
  i=1
  for shot in "${@:2}"; do
    OFFSET=$(echo "$LENGTH - $FADE" | bc -l)
    FILTER+="${PREV}[$i:v]xfade=transition=fade:duration=$FADE:offset=$OFFSET[x$i];"
    PREV="[x$i]"
    if [ "$SOUND" = 1 ]; then
      AUDIO+=";${APREV}[$(($# + i)):a]acrossfade=d=$FADE:c1=tri:c2=tri[y$i]"
      APREV="[y$i]"
    fi
    FRAMES=$(ls "$DIR/$shot" | grep -c '\.png$')
    LENGTH=$(echo "$OFFSET + $FRAMES / $FPS" | bc -l)
    i=$((i + 1))
  done
  FILTER+="${PREV}${CROP}format=yuv420p[v]"
  [ "$SOUND" = 1 ] && AUDIO+=";${APREV}anull[a]"
fi

if [ "$SOUND" = 1 ]; then
  SOUND_OUT=(-map "[a]" -c:a aac -b:a 192k -ar 48000)
else
  SOUND_OUT=(-an)
fi
ffmpeg -y -loglevel error "${INPUTS[@]}" -filter_complex "$FILTER$AUDIO" -map "[v]" "${SOUND_OUT[@]}" \
  -c:v libx264 -preset slow -profile:v high -level 4.2 -crf 14 -maxrate 25M -bufsize 50M \
  -pix_fmt yuv420p -r "$FPS" -g "$FPS" -movflags +faststart "$OUT"
echo "$OUT"
