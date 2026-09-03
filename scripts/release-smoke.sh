#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARTIFACT="$($ROOT/scripts/package-linux.sh | tail -1)"
ARTIFACT_DIR="$(dirname "$ARTIFACT")"
ARTIFACT_NAME="$(basename "$ARTIFACT")"
(cd "$ARTIFACT_DIR" && sha256sum -c "$ARTIFACT_NAME.sha256")
TMP="$(mktemp -d)"
cleanup(){
  [[ -n "${PID:-}" ]] && kill "$PID" 2>/dev/null || true
  local state="$TMP/data/muxlane/state.json"
  if [[ -f "$state" ]]; then
    python3 - "$state" <<'PY' | while read -r session; do
import json,sys
for item in json.load(open(sys.argv[1])).get('sessions', []):
    if item.get('tmux_session'): print(item['tmux_session'])
PY
      tmux -L muxlane kill-session -t "$session" 2>/dev/null || true
    done
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

tar -xzf "$ARTIFACT" -C "$TMP"
DIR="$(find "$TMP" -mindepth 1 -maxdepth 1 -type d | head -1)"
for f in muxlane README.md muxlane.desktop install.sh; do [[ -e "$DIR/$f" ]] || { echo "missing $f"; exit 1; }; done
[[ -x "$DIR/muxlane" && -x "$DIR/install.sh" ]]
VERSION="$("$DIR/muxlane" --version | awk '{print $2}')"
"$DIR/muxlane" --version | grep -q "^muxlane $VERSION$"

PREFIX="$TMP/prefix" "$DIR/install.sh" "$DIR/muxlane"
"$TMP/prefix/bin/muxlane" --version | grep -q '^muxlane '
grep -q '^Exec=muxlane$' "$TMP/prefix/share/applications/muxlane.desktop"

XDG_DATA_HOME="$TMP/data" "$DIR/muxlane" --headless >"$TMP/headless.log" 2>&1 &
PID=$!
SOCK="$TMP/data/muxlane/muxlane.sock"
for _ in {1..100}; do [[ -S "$SOCK" ]] && break; sleep .05; done
[[ -S "$SOCK" ]]
MUXLANE_SOCKET="$SOCK" MUXLANE_EXPECTED_VERSION="$VERSION" python3 - <<'PY'
import os,socket,json
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect(os.environ['MUXLANE_SOCKET'])
s.sendall(b'{"id":1,"method":"state.list"}\n');b=b''
while b'\n' not in b:b+=s.recv(65536)
r=json.loads(b.decode()); assert r['result']['machine']['version']==os.environ['MUXLANE_EXPECTED_VERSION']
print('✓ release headless state.list')
PY
[[ "$(stat -c %a "$SOCK")" == 600 ]]
[[ "$(stat -c %a "$TMP/data/muxlane/secret")" == 600 ]]
echo "release smoke passed: $ARTIFACT"
