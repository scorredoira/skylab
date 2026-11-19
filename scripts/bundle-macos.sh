#!/bin/bash
# Builds the games (release) and wraps them in target/release/Skylab.app: Skylab's binary (which
# has the driving game inside it: the first screen's "Cars" shows it in the same window), the
# driving game's own binary beside it, the icon (assets/brand/skylab.icns) and an Info.plist. Open it from Finder
# or with `open target/release/Skylab.app`.
# The app finds the repository's `assets` by walking up from its binaries, so it stays where it is made;
# settings and downloads are the same as the binaries' (~/.config/flight, ~/.config/skylab-drive,
# ~/Library/Caches/flight).
# Usage: scripts/bundle-macos.sh
set -euo pipefail
cd "$(dirname "$0")/.." || exit 1
cargo build --release -p flight -p drive
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP=target/release/Skylab.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/skylab "$APP/Contents/MacOS/skylab"
cp target/release/skylab-drive "$APP/Contents/MacOS/skylab-drive"
cp assets/brand/skylab.icns "$APP/Contents/Resources/skylab.icns"
cat >"$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Skylab</string>
  <key>CFBundleDisplayName</key><string>Skylab</string>
  <key>CFBundleIdentifier</key><string>local.skylab</string>
  <key>CFBundleExecutable</key><string>skylab</string>
  <key>CFBundleIconFile</key><string>skylab</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>${VERSION:-0.1.0}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION:-0.1.0}</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.simulation-games</string>
</dict>
</plist>
EOF
# An ad-hoc signature, so macOS runs it on this machine.
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
echo "$APP"
