#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
VERSION="${VERSION:-0.1.0}"
ARCH="$(uname -m)"
NAME="muxlane-${VERSION}-linux-${ARCH}"
DIST="$ROOT/dist/$NAME"
rm -rf "$DIST"
mkdir -p "$DIST"
cargo build -p muxlane-app --release
install -Dm755 "$ROOT/target/release/muxlane" "$DIST/muxlane"
cp "$ROOT/README.md" "$DIST/README.md"
cp "$ROOT/packaging/muxlane.desktop" "$DIST/muxlane.desktop"
cp "$ROOT/packaging/muxlane.svg" "$DIST/muxlane.svg"
cp "$ROOT/packaging/install.sh" "$DIST/install.sh"
tar -C "$ROOT/dist" -czf "$ROOT/dist/$NAME.tar.gz" "$NAME"
(cd "$ROOT/dist" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
echo "$ROOT/dist/$NAME.tar.gz"
