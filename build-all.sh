#!/usr/bin/env bash
set -euo pipefail

# Build all requested targets from macOS:
# 1) native macOS release
# 2) Windows GNU x86_64
# 3) Raspberry Pi armv7 musl (static-friendly)

MAC_TARGET=""
ARCH="$(uname -m)"
case "$ARCH" in
  arm64) MAC_TARGET="aarch64-apple-darwin" ;;
  x86_64) MAC_TARGET="x86_64-apple-darwin" ;;
  *)
    echo "Unsupported macOS architecture: $ARCH"
    exit 1
    ;;
esac

WINDOWS_TARGET="x86_64-pc-windows-gnu"
PI_TARGET="armv7-unknown-linux-musleabihf"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1"
    exit 1
  fi
}

require_cmd cargo
require_cmd rustup
require_cmd zig

if ! command -v cargo-zigbuild >/dev/null 2>&1; then
  echo "cargo-zigbuild not found."
  echo "Install with: cargo install cargo-zigbuild"
  exit 1
fi

echo "Adding targets (safe if already installed)..."
rustup target add "$MAC_TARGET" "$WINDOWS_TARGET" "$PI_TARGET"

echo "Building macOS release ($MAC_TARGET)..."
cargo build --release --target "$MAC_TARGET"

echo "Building Windows release ($WINDOWS_TARGET)..."
cargo zigbuild --release --target "$WINDOWS_TARGET"

echo "Building Raspberry Pi release ($PI_TARGET)..."
cargo zigbuild --release --target "$PI_TARGET"

echo ""
echo "Build complete. Artifacts:"
echo "  target/$MAC_TARGET/release/ping-plotter"
echo "  target/$WINDOWS_TARGET/release/ping-plotter.exe"
echo "  target/$PI_TARGET/release/ping-plotter"
