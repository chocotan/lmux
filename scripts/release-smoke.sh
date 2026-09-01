#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARTIFACT="$($ROOT/scripts/package-linux.sh | tail -1)"
sha256sum -c "$ARTIFACT.sha256"
TMP="$(mktemp -d)"
cleanup(){
  [[ -n "${PID:-}" ]] && kill "$PID" 2>/dev/null || true
  local state="$TMP/data/lmux/state.json"
  if [[ -f "$state" ]]; then
    python3 - "$state" <<'PY' | while read -r session; do
import json,sys
for item in json.load(open(sys.argv[1])).get('sessions', []):
    if item.get('tmux_session'): print(item['tmux_session'])
PY
      tmux -L lmux kill-session -t "$session" 2>/dev/null || true
    done
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

tar -xzf "$ARTIFACT" -C "$TMP"
DIR="$(find "$TMP" -mindepth 1 -maxdepth 1 -type d | head -1)"
for f in lmux README.md lmux.desktop install.sh; do [[ -e "$DIR/$f" ]] || { echo "missing $f"; exit 1; }; done
[[ -x "$DIR/lmux" && -x "$DIR/install.sh" ]]
"$DIR/lmux" --version | grep -q '^lmux '

PREFIX="$TMP/prefix" "$DIR/install.sh" "$DIR/lmux"
"$TMP/prefix/bin/lmux" --version | grep -q '^lmux '
grep -q '^Exec=lmux$' "$TMP/prefix/share/applications/lmux.desktop"

XDG_DATA_HOME="$TMP/data" "$DIR/lmux" --headless >"$TMP/headless.log" 2>&1 &
PID=$!
SOCK="$TMP/data/lmux/lmux.sock"
for _ in {1..100}; do [[ -S "$SOCK" ]] && break; sleep .05; done
[[ -S "$SOCK" ]]
LMUX_SOCKET="$SOCK" python3 - <<'PY'
import os,socket,json
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect(os.environ['LMUX_SOCKET'])
s.sendall(b'{"id":1,"method":"state.list"}\n');b=b''
while b'\n' not in b:b+=s.recv(65536)
r=json.loads(b.decode()); assert r['result']['machine']['version']=='0.1.0'
print('✓ release headless state.list')
PY
[[ "$(stat -c %a "$SOCK")" == 600 ]]
[[ "$(stat -c %a "$TMP/data/lmux/secret")" == 600 ]]
echo "release smoke passed: $ARTIFACT"
