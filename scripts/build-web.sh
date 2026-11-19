#!/bin/bash
# Builds the web version (WebGPU) into a folder of static files: the page, the module and its
# JavaScript, the worker, the textures and their list. Serve it cross-origin isolated
# (scripts/serve-web.sh does) and open it in Chrome or Safari.
# Usage: scripts/build-web.sh [<folder>]          (default: dist)
#
# The module uses wasm's threads (the workers of `engine::threads` share its memory), which needs
# the standard library built again with atomics: a nightly toolchain with its sources, pinned below
# and installed with rustup if missing, as is the wasm-bindgen of Cargo.lock's version.
set -euo pipefail
cd "$(dirname "$0")/.." || exit 1
OUT=${1:-dist}
TARGET=wasm32-unknown-unknown
TOOLCHAIN=${FLIGHT_WEB_TOOLCHAIN:-nightly-2026-11-14}
TARGET_DIR=${CARGO_TARGET_DIR:-target}

if ! rustup which --toolchain "$TOOLCHAIN" rustc >/dev/null 2>&1; then
  rustup toolchain install "$TOOLCHAIN" --profile minimal --component rust-src --target "$TARGET"
fi
NIGHTLY_CARGO=$(rustup which --toolchain "$TOOLCHAIN" cargo)
NIGHTLY_RUSTC=$(rustup which --toolchain "$TOOLCHAIN" rustc)

BINDGEN=$(sed -n '/^name = "wasm-bindgen"$/{n;s/version = "\(.*\)"/\1/p;}' Cargo.lock)
if ! wasm-bindgen --version 2>/dev/null | grep -qx "wasm-bindgen $BINDGEN"; then
  cargo install wasm-bindgen-cli --version "$BINDGEN" --locked
fi

# Threads: atomics, a shared memory the page makes (up to 4 GB) and the thread-local storage each
# worker sets up. 8 MB of stack for the page's thread, as a native main thread has.
LINK="--shared-memory --import-memory --max-memory=4294967296 --export=__wasm_init_tls --export=__tls_size --export=__tls_align --export=__tls_base -zstack-size=8388608"
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals $(printf -- '-C link-arg=%s ' $LINK)" \
  RUSTC="$NIGHTLY_RUSTC" "$NIGHTLY_CARGO" build -Z build-std=std,panic_abort --target "$TARGET" --release -p flight --bin skylab

rm -rf "$OUT"
mkdir -p "$OUT/assets/textures"
wasm-bindgen --target web --no-typescript --out-dir "$OUT" --out-name flight "$TARGET_DIR/$TARGET/release/skylab.wasm"
# Smaller and faster with binaryen's optimiser, when it is installed (brew install binaryen).
if command -v wasm-opt >/dev/null; then
  # The wasm features it may use are those the compiler wrote into the module.
  wasm-opt -O2 -o "$OUT/flight_bg.wasm" "$OUT/flight_bg.wasm"
fi
cp crates/games/flight/web/index.html crates/games/flight/web/worker.js crates/games/flight/web/sw.js "$OUT/"

# The textures: the colours and the foliage's cut-outs as they are; the normal, roughness and occlusion maps at 512 and as
# JPEGs (prepare_textures' `web`), made once and kept in the target folder.
TEXTURES="$TARGET_DIR/web-textures"
mkdir -p "$TEXTURES"
stale=()
for file in assets/textures/*_normal.png assets/textures/*_rough.jpg assets/textures/*_ao.jpg; do
  [ "$TEXTURES/$(basename "$file")" -nt "$file" ] || stale+=("$file")
done
if [ ${#stale[@]} -gt 0 ]; then
  cargo run --release -p engine --example prepare_textures -- web 512 90 "$TEXTURES" "${stale[@]}"
fi
cp assets/textures/*_color.jpg assets/textures/foliage_*.png "$TEXTURES"/* assets/textures/CREDITS.md "$OUT/assets/textures/"
# The aircraft's looks (`kinds`: each type's exterior and cockpit, glTF and RON).
for dir in assets/aircraft/*/; do
  mkdir -p "$OUT/$dir"
  cp "$dir"*.glb "$dir"*.ron "$OUT/$dir" 2>/dev/null
done
(cd "$OUT" && for file in assets/textures/*_*.* assets/aircraft/*/*.glb assets/aircraft/*/*.ron; do echo "$(wc -c <"$file" | tr -d ' ') $file"; done) >"$OUT/files.txt"

echo "$OUT: $(du -sh "$OUT" | cut -f1) ($(du -h "$OUT/flight_bg.wasm" | cut -f1) the module, $(du -sh "$OUT/assets" | cut -f1) the textures)"
