#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
if [[ -n "${1:-}" ]]; then
  BIN="$1"
elif [[ -x "$HERE/lmux" ]]; then
  BIN="$HERE/lmux"                 # extracted release archive
else
  BIN="$REPO_ROOT/target/release/lmux" # repository layout
fi
if [[ -f "$HERE/lmux.desktop" ]]; then
  DESKTOP="$HERE/lmux.desktop"
else
  DESKTOP="$REPO_ROOT/packaging/lmux.desktop"
fi
[[ -x "$BIN" ]] || { echo "binary not found: $BIN" >&2; exit 1; }
[[ -f "$DESKTOP" ]] || { echo "desktop file not found: $DESKTOP" >&2; exit 1; }
install -Dm755 "$BIN" "$PREFIX/bin/lmux"
install -Dm644 "$DESKTOP" "$PREFIX/share/applications/lmux.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
echo "installed: $PREFIX/bin/lmux"
echo "ensure $PREFIX/bin is in PATH"
