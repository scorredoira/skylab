#!/bin/bash
# Serves the web version built by scripts/build-web.sh at http://localhost:8080/ (or the port
# given), cross-origin isolated as its workers need, with a small server of the standard library
# (scripts/serve-web.rs, run as a cargo script by the web build's nightly toolchain).
# Usage: scripts/serve-web.sh [<folder>] [<port>]          (default: dist 8080)
cd "$(dirname "$0")/.." || exit 1
TOOLCHAIN=${FLIGHT_WEB_TOOLCHAIN:-nightly-2026-11-14}
if ! rustup which --toolchain "$TOOLCHAIN" cargo >/dev/null 2>&1; then
  rustup toolchain install "$TOOLCHAIN" --profile minimal --component rust-src --target wasm32-unknown-unknown || exit 1
fi
[ -f "${1:-dist}/index.html" ] || { echo "no ${1:-dist}/index.html: run scripts/build-web.sh first"; exit 1; }
RUSTC=$(rustup which --toolchain "$TOOLCHAIN" rustc) exec "$(rustup which --toolchain "$TOOLCHAIN" cargo)" -Zscript -q scripts/serve-web.rs "${1:-dist}" "${2:-8080}"
