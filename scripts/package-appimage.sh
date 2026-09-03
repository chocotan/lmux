#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${1:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)}"
VERSION="${VERSION:-0.0.1}"
ARCH="$(uname -m)"

echo "==> Building Linux AppImage for Muxlane v${VERSION} (${ARCH})..."

# 1. Build release binary if not present
if [[ ! -f "$ROOT/target/release/muxlane" ]]; then
    cargo build -p muxlane-app --release
fi

# 2. Setup AppDir
APPDIR="$ROOT/dist/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"

install -Dm755 "$ROOT/target/release/muxlane" "$APPDIR/usr/bin/muxlane"
install -Dm644 "$ROOT/packaging/muxlane.desktop" "$APPDIR/usr/share/applications/muxlane.desktop"
install -Dm644 "$ROOT/packaging/muxlane.desktop" "$APPDIR/muxlane.desktop"
install -Dm644 "$ROOT/packaging/muxlane.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/muxlane.svg"
install -Dm644 "$ROOT/packaging/muxlane.svg" "$APPDIR/muxlane.svg"
install -Dm644 "$ROOT/packaging/muxlane.svg" "$APPDIR/.DirIcon"

# AppRun launcher script
cat << 'EOF' > "$APPDIR/AppRun"
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"
export PATH="${HERE}/usr/bin:${PATH}"
exec "${HERE}/usr/bin/muxlane" "$@"
EOF
chmod +x "$APPDIR/AppRun"

# 3. Download appimagetool if not found
mkdir -p "$ROOT/dist"
APPIMAGETOOL="$ROOT/dist/appimagetool"
if [[ ! -x "$APPIMAGETOOL" ]]; then
    echo "==> Downloading appimagetool..."
    ARCH_TOOL="x86_64"
    if [[ "$ARCH" == "aarch64" ]]; then
        ARCH_TOOL="aarch64"
    fi
    curl -sSL -o "$APPIMAGETOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH_TOOL}.AppImage"
    chmod +x "$APPIMAGETOOL"
fi

# 4. Generate AppImage (APPIMAGE_EXTRACT_AND_RUN=1 allows running inside containers / GH Actions)
OUTPUT_NAME="muxlane-${VERSION}-linux-${ARCH}.AppImage"
OUTPUT_PATH="$ROOT/dist/${OUTPUT_NAME}"

echo "==> Generating AppImage..."
export ARCH="$ARCH"
export APPIMAGE_EXTRACT_AND_RUN=1
"$APPIMAGETOOL" "$APPDIR" "$OUTPUT_PATH"

# Generate sha256 checksum
(cd "$ROOT/dist" && sha256sum "$OUTPUT_NAME" > "${OUTPUT_NAME}.sha256")
echo "==> Successfully created ${OUTPUT_PATH}"
