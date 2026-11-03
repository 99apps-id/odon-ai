#!/usr/bin/env bash
# Stages the odond daemon as the desktop shell's Tauri sidecar.
#
# Usage: scripts/stage-sidecar.sh [TARGET_TRIPLE]
#
# Builds odond in release mode for the given (or host) target triple and
# copies it to desktop/src-tauri/binaries/odond-<triple>, which is where
# tauri.conf.json's externalBin expects it. The release workflow runs
# this on each platform runner before `tauri build`.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TRIPLE="${1:-$(rustc -vV | awk '/^host:/ {print $2}')}"
HOST_TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"

if [ "$TRIPLE" = "$HOST_TRIPLE" ]; then
  cargo build --release --manifest-path "$ROOT/Cargo.toml" -p odond
  BUILT="$ROOT/target/release/odond"
else
  cargo build --release --manifest-path "$ROOT/Cargo.toml" -p odond --target "$TRIPLE"
  BUILT="$ROOT/target/$TRIPLE/release/odond"
fi

DEST_DIR="$ROOT/desktop/src-tauri/binaries"
mkdir -p "$DEST_DIR"
cp "$BUILT" "$DEST_DIR/odond-$TRIPLE"
chmod +x "$DEST_DIR/odond-$TRIPLE"
echo "staged $DEST_DIR/odond-$TRIPLE"
