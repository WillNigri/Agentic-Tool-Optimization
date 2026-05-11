#!/usr/bin/env bash
# Build the `ato` CLI for the host target triple and stage it as a
# Tauri sidecar in apps/desktop/src-tauri/binaries/.
#
# Why this script exists: tauri.conf.json declares the CLI as an
# externalBin. Tauri's bundler looks for `binaries/ato-<host-triple>`
# at bundle time. CI builds per-target in release.yml; local devs run
# this script (or `npm run tauri:build` which chains it).
#
# `tauri dev` does NOT need the sidecar — externalBin is bundle-time
# only. This script is for `tauri build` and verifying local bundle
# output.

set -euo pipefail

# Resolve the host target triple from rustc. This is the same value
# Tauri expects in the sidecar filename suffix.
TARGET="$(rustc -vV | sed -n 's/host: //p')"
if [ -z "$TARGET" ]; then
  echo "::error::Could not determine host target triple from rustc."
  exit 1
fi

# Build the CLI. The release profile keeps the binary small enough not
# to bloat the desktop bundle materially (a few MB).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_DIR="$SCRIPT_DIR/../../cli"
DEST_DIR="$SCRIPT_DIR/../src-tauri/binaries"

(
  cd "$CLI_DIR"
  cargo build --release --target "$TARGET"
)

mkdir -p "$DEST_DIR"

# Windows binaries have .exe; everything else is bare.
if [[ "$TARGET" == *"windows"* ]]; then
  cp "$CLI_DIR/target/$TARGET/release/ato.exe" "$DEST_DIR/ato-$TARGET.exe"
else
  cp "$CLI_DIR/target/$TARGET/release/ato" "$DEST_DIR/ato-$TARGET"
  chmod +x "$DEST_DIR/ato-$TARGET"
fi

echo "Sidecar staged: $DEST_DIR/ato-$TARGET"
