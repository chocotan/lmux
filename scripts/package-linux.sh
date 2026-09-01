#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
VERSION="${VERSION:-0.1.0}"
ARCH="$(uname -m)"
NAME="lmux-${VERSION}-linux-${ARCH}"
DIST="$ROOT/dist/$NAME"
rm -rf "$DIST"
mkdir -p "$DIST"
cargo build -p lmux-app --release
install -Dm755 "$ROOT/target/release/lmux" "$DIST/lmux"
cp "$ROOT/README.md" "$DIST/README.md"
cp "$ROOT/packaging/lmux.desktop" "$DIST/lmux.desktop"
cp "$ROOT/packaging/install.sh" "$DIST/install.sh"
tar -C "$ROOT/dist" -czf "$ROOT/dist/$NAME.tar.gz" "$NAME"
sha256sum "$ROOT/dist/$NAME.tar.gz" > "$ROOT/dist/$NAME.tar.gz.sha256"
echo "$ROOT/dist/$NAME.tar.gz"
