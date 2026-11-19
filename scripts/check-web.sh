#!/bin/bash
# Checks that the crates meant for the web still compile for it (wasm32-unknown-unknown, WebGPU),
# so nothing native-only creeps in before the web version is built. It only compiles: nothing runs.
# Usage: scripts/check-web.sh
# sim-env (the socket server) and the Swift trainer are native only and not checked on their own.
cd "$(dirname "$0")/.." || exit 1
TARGET=wasm32-unknown-unknown

# Homebrew's rustc has no standard library for the web: use rustup's toolchain when it has one.
if ! rustc --print target-libdir --target "$TARGET" >/dev/null 2>&1 || [ ! -d "$(rustc --print target-libdir --target "$TARGET" 2>/dev/null)" ]; then
  if command -v rustup >/dev/null; then
    rustup target list --installed | grep -qx "$TARGET" || rustup target add "$TARGET" || exit 1
    RUSTC=$(rustup which rustc) || exit 1
    export RUSTC
  fi
fi

cargo check --target "$TARGET" -p sim-core -p engine -p world-render -p geodata -p flight "$@"
