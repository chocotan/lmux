#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
if [[ -n "${1:-}" ]]; then
  BIN="$1"
elif [[ -x "$HERE/muxlane" ]]; then
  BIN="$HERE/muxlane"                 # extracted release archive
else
  BIN="$REPO_ROOT/target/release/muxlane" # repository layout
fi
if [[ -f "$HERE/muxlane.desktop" ]]; then
  DESKTOP="$HERE/muxlane.desktop"
else
  DESKTOP="$REPO_ROOT/packaging/muxlane.desktop"
fi
[[ -x "$BIN" ]] || { echo "binary not found: $BIN" >&2; exit 1; }
[[ -f "$DESKTOP" ]] || { echo "desktop file not found: $DESKTOP" >&2; exit 1; }
install -Dm755 "$BIN" "$PREFIX/bin/muxlane"
install -Dm644 "$DESKTOP" "$PREFIX/share/applications/muxlane.desktop"
if [[ -f "$HERE/muxlane.svg" ]]; then
  install -Dm644 "$HERE/muxlane.svg" "$PREFIX/share/icons/hicolor/scalable/apps/muxlane.svg"
elif [[ -f "$REPO_ROOT/packaging/muxlane.svg" ]]; then
  install -Dm644 "$REPO_ROOT/packaging/muxlane.svg" "$PREFIX/share/icons/hicolor/scalable/apps/muxlane.svg"
fi
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
echo "installed: $PREFIX/bin/muxlane"
echo "ensure $PREFIX/bin is in PATH"
