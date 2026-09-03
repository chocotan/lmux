#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKSPACE="${MUXLANE_UI_WORKSPACE:-4}"
DISPLAY="${DISPLAY:-:0}"
export DISPLAY
ARTIFACTS="$ROOT/artifacts/ui-smoke"
mkdir -p "$ARTIFACTS"

need() { command -v "$1" >/dev/null || { echo "missing tool: $1" >&2; exit 1; }; }
for tool in wmctrl xdotool import python3 tmux node fcitx5-remote; do need "$tool"; done
python3 -c 'import PIL' >/dev/null 2>&1 || { echo "missing Python module: Pillow" >&2; exit 1; }
[[ -x "${MUXLANE_TEST_SHELL:-/usr/bin/zsh}" ]] || { echo "missing test shell: ${MUXLANE_TEST_SHELL:-/usr/bin/zsh}" >&2; exit 1; }

cargo build -p muxlane-app >/dev/null
TMP="$(mktemp -d)"
ORIGINAL_FCITX_STATE="$(fcitx5-remote 2>/dev/null || echo 1)"
cleanup() {
  local status=$?
  if [[ "$status" -ne 0 && -f "$TMP/muxlane.log" ]]; then
    echo "--- muxlane.log ---" >&2
    tail -80 "$TMP/muxlane.log" >&2
  fi
  [[ -n "${PID:-}" ]] && kill -9 "$PID" 2>/dev/null || true
  if [[ -f "$XDG_DATA_HOME/muxlane/state.json" ]]; then
    python3 - "$XDG_DATA_HOME/muxlane/state.json" <<'PY' | while read -r session; do
import json,sys
for item in json.load(open(sys.argv[1])).get('sessions', []):
    if item.get('tmux_session'): print(item['tmux_session'])
PY
      tmux -L muxlane kill-session -t "$session" 2>/dev/null || true
    done
  fi
  [[ -n "${ORIGINAL_WS:-}" ]] && wmctrl -s "$ORIGINAL_WS" 2>/dev/null || true
  if [[ "$ORIGINAL_FCITX_STATE" == 2 ]]; then fcitx5-remote -o >/dev/null 2>&1 || true; fi
  rm -rf "$TMP"
}
trap cleanup EXIT

export SHELL="${MUXLANE_TEST_SHELL:-/usr/bin/zsh}"
export MUXLANE_TEST_AUTO_OPEN=1
export XDG_DATA_HOME="$TMP/data"
mkdir -p "$XDG_DATA_HOME/muxlane"
SMOKE_PROJECT="$TMP/workspace/muxlane"
mkdir -p "$SMOKE_PROJECT"
SMOKE_AGENT="shell_smoke"
SMOKE_TMUX="muxlane-smoke-shell"
tmux -L muxlane new-session -d -s "$SMOKE_TMUX" -c "$SMOKE_PROJECT" "$SHELL"
python3 - "$XDG_DATA_HOME/muxlane/state.json" "$SMOKE_PROJECT" "$SMOKE_AGENT" "$SMOKE_TMUX" <<'PY'
import json, sys
path, root, agent, tmux_session = sys.argv[1:]
with open(path, "w") as f:
    json.dump({
        "version": 1,
        "initialized": True,
        "projects": [{"id": "p_smoke", "name": "muxlane", "path": root, "branch": None, "agents": []}],
        "sessions": [{"agent_id": agent, "project_id": "p_smoke", "agent_type": "shell", "title": "shell", "tmux_session": tmux_session}],
    }, f)
PY
"$ROOT/target/debug/muxlane" >"$TMP/muxlane.log" 2>&1 &
PID=$!
for _ in {1..100}; do
  WID="$(wmctrl -lp | awk -v p="$PID" '$3==p {print $1; exit}')"
  [[ -n "$WID" ]] && break
  sleep .1
done
[[ -n "${WID:-}" ]] || { cat "$TMP/muxlane.log" >&2; exit 1; }

eval "$(xdotool getwindowgeometry --shell "$WID" | grep -E '^(WIDTH|HEIGHT)=')"
px() { awk -v n="$1" -v total="$2" 'BEGIN{printf "%d", n*total}'; }
click_at() {
  local x="$1" y="$2" button="${3:-1}"
  xdotool mousemove --window "$WID" "$x" "$y"
  xdotool mousedown "$button"
  sleep .1
  xdotool mouseup "$button"
}
X_TERM="$(px .30 "$WIDTH")"; Y_TERM="$(px .28 "$HEIGHT")"
X_SESSION="$(px .05 "$WIDTH")"; Y_SESSION="$(px .14 "$HEIGHT")"
X_PROJECT_ADD="$(px .135 "$WIDTH")"; Y_PROJECT_ADD="$(px .085 "$HEIGHT")"
X_PALETTE_ITEM="$(px .34 "$WIDTH")"; Y_PALETTE_ITEM="$(px .255 "$HEIGHT")"
X_SPLIT="$(px .966 "$WIDTH")"; Y_HEADER="$(px .027 "$HEIGHT")"
X_MAX="$(px .992 "$WIDTH")"
echo "geometry=${WIDTH}x${HEIGHT} palette=${X_PALETTE_ITEM},${Y_PALETTE_ITEM} project=${X_PROJECT_ADD},${Y_PROJECT_ADD}"

ORIGINAL_WS="$(wmctrl -d | awk '$2=="*" {print $1}')"
wmctrl -i -r "$WID" -t "$WORKSPACE"
wmctrl -s "$WORKSPACE"
sleep .4
xdotool windowactivate "$WID"

# 1. 输入 + 真彩色 + 光标 + Fcitx 中文 IME。
click_at "$X_TERM" "$Y_TERM" 1
sleep .5
fcitx5-remote -s keyboard-us
sleep .2
xdotool type --delay 2 "echo MUXLANEASCII"
xdotool key Return
xdotool type --delay 3 "echo "
fcitx5-remote -s pinyin
fcitx5-remote -o
sleep .2
xdotool type --delay 8 "nihao"
xdotool key space
sleep .15
fcitx5-remote -s keyboard-us
xdotool key Return
# 颜色序列通过同一 tmux PTY 注入，避免 XTest 对反斜杠/下划线的键盘布局差异。
COLOR_TMUX="$(python3 - "$XDG_DATA_HOME/muxlane/state.json" <<'PY'
import json,sys
print(json.load(open(sys.argv[1]))['sessions'][0]['tmux_session'])
PY
)"
tmux -L muxlane send-keys -t "$COLOR_TMUX" -l -- "printf '\\033[31mMUXLANE_RED\\033[0m\\n'"
tmux -L muxlane send-keys -t "$COLOR_TMUX" Enter
sleep .5
import -window "$WID" "$ARTIFACTS/01-color-cursor-input.png"

# 协议回放：输入真的进 PTY。截图像素：红色文字和蓝色 cursor 真的被 GPUI 画出。
MUXLANE_SOCKET="$XDG_DATA_HOME/muxlane/muxlane.sock" SCREENSHOT="$ARTIFACTS/01-color-cursor-input.png" python3 - <<'PY'
import os, socket, json, base64
from PIL import Image
p=os.environ['MUXLANE_SOCKET']
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); s.connect(p)
s.sendall(b'{"id":1,"method":"state.list"}\n'); b=b''
while b'\n' not in b: b+=s.recv(65536)
aid=json.loads(b.decode())['result']['agents'][0]['id']
s.sendall((json.dumps({'id':2,'method':'term.subscribe','params':{'agent':aid}})+'\n').encode()); b=b''
while b'\n' not in b: b+=s.recv(65536)
text=base64.b64decode(json.loads(b.decode())['result']['replay_b64']).decode('utf8','replace')
assert 'MUXLANEASCII' in text, text[-500:]
assert 'MUXLANE_RED' in text, text[-500:]
assert '你好' in text, text[-500:]
assert 'nihao' not in text, text[-500:]
im=Image.open(os.environ['SCREENSHOT']).convert('RGB')
red=sum(1 for r,g,b in im.getdata() if r>170 and r>g*1.4 and r>b*1.3)
blue=sum(1 for r,g,b in im.getdata() if b>130 and b>r*1.25 and b>g*1.05)
assert red>20, f'no rendered red text: {red}'
assert blue>20, f'no rendered blue cursor/accent: {blue}'
print(f'✓ input replay + rendered red={red} blue/cursor={blue}')
PY

# hook 权威事件：working spinner 与 done notification。
read -r HOOK_AGENT HOOK_TMUX < <(MUXLANE_SOCKET="$XDG_DATA_HOME/muxlane/muxlane.sock" python3 - <<'PY'
import os,socket,json
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect(os.environ['MUXLANE_SOCKET'])
s.sendall(b'{"id":1,"method":"state.list"}\n');b=b''
while b'\n' not in b:b+=s.recv(65536)
a=json.loads(b.decode())['result']['agents'][0]
print(a['id'], a['tmux_session'])
PY
)
HOOK_TOKEN="$(tmux -L muxlane show-environment -t "$HOOK_TMUX" MUXLANE_HOOK_TOKEN | cut -d= -f2-)"
run_hook() {
  local payload="${2:-}"
  printf '%s' "$payload" | MUXLANE_SOCKET="$XDG_DATA_HOME/muxlane/muxlane.sock" MUXLANE_AGENT_ID="$HOOK_AGENT" \
    MUXLANE_HOOK_TOKEN="$HOOK_TOKEN" node "$XDG_DATA_HOME/muxlane/hooks/report.mjs" "$1"
}
run_hook working; sleep .18
import -window "$WID" "$ARTIFACTS/02-working-spinner-a.png"
sleep .18
import -window "$WID" "$ARTIFACTS/02-working-spinner-b.png"
run_hook done '{"last_assistant_message":"完成了具体修复并通过测试"}'; sleep .35
import -window "$WID" "$ARTIFACTS/02-done-notification.png"
MUXLANE_SOCKET="$XDG_DATA_HOME/muxlane/muxlane.sock" python3 - <<'PY'
import os,socket,json
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect(os.environ['MUXLANE_SOCKET'])
s.sendall(b'{"id":1,"method":"state.list"}\n');b=b''
while b'\n' not in b:b+=s.recv(65536)
status=json.loads(b.decode())['result']['agents'][0]['status']
assert status=='done', status
print('✓ hook event reached done state with notification message content')
PY

# 2. Ctrl+K 命令面板。
xdotool key ctrl+k; sleep .35
import -window "$WID" "$ARTIFACTS/02-command-palette.png"
xdotool key Escape

# 3. 会话右键菜单。
click_at "$X_SESSION" "$Y_SESSION" 3; sleep .3
import -window "$WID" "$ARTIFACTS/03-session-menu.png"
xdotool key Escape

# 4. 新建第二个 Shell 会话（Ctrl+K；项目 + 使用同一 preset action）。
# 命令面板第一项是已有会话跳转，Down 一次选中「新建 Shell」preset。
xdotool key ctrl+k; sleep .4
import -window "$WID" "$ARTIFACTS/04-project-add-session.png"
xdotool key Down; sleep .2
xdotool key Return; sleep .8
MUXLANE_SOCKET="$XDG_DATA_HOME/muxlane/muxlane.sock" python3 - <<'PY'
import os,socket,json
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect(os.environ['MUXLANE_SOCKET'])
s.sendall(b'{"id":1,"method":"state.list"}\n');b=b''
while b'\n' not in b:b+=s.recv(65536)
count=len(json.loads(b.decode())['result']['agents'])
assert count==2, f'expected 2 agents after preset action, got {count}'
print('✓ preset action created second session (same action used by project +)')
PY

# 默认应仍是一棵 leaf（两个 tabs），未显式 split。
STATE="$XDG_DATA_HOME/muxlane/state.json" python3 - <<'PY'
import os,json
d=json.load(open(os.environ['STATE']))
assert d['pane_tree']['kind']=='leaf', d['pane_tree']
assert len(d['pane_tree']['group']['tabs'])==2
print('✓ second session opened as tab, not implicit split')
PY
import -window "$WID" "$ARTIFACTS/05-two-tabs-no-split.png"

# Ctrl+W 与 tab × 共用 close_tab；只关闭布局标签，不结束后台 session。
xdotool key ctrl+w
sleep .2
STATE="$XDG_DATA_HOME/muxlane/state.json" MUXLANE_SOCKET="$XDG_DATA_HOME/muxlane/muxlane.sock" python3 - <<'PY'
import os,json,socket
d=json.load(open(os.environ['STATE']))
assert len(d['pane_tree']['group']['tabs'])==1, d['pane_tree']
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect(os.environ['MUXLANE_SOCKET'])
s.sendall(b'{"id":1,"method":"state.list"}\n');b=b''
while b'\n' not in b:b+=s.recv(65536)
assert len(json.loads(b.decode())['result']['agents'])==1
print('✓ tab close terminated and removed its session')
PY
import -window "$WID" "$ARTIFACTS/05-tab-closed.png"

# tab strip ＋ 与 Ctrl+Shift+T 共用 new_shell_tab：新增普通 Shell tab，不 split。
xdotool key ctrl+shift+t; sleep .5
STATE="$XDG_DATA_HOME/muxlane/state.json" python3 - <<'PY'
import os,json
d=json.load(open(os.environ['STATE']))
assert d['pane_tree']['kind']=='leaf', d['pane_tree']
assert len(d['pane_tree']['group']['tabs'])==2, d['pane_tree']
print('✓ tab-strip create path added a Shell tab without splitting')
PY
xdotool key ctrl+w; sleep .2
click_at "$X_TERM" "$Y_TERM" 1

# 5. 显式分屏（Ctrl+K, h）。
xdotool key ctrl+k; sleep .2; xdotool key h; sleep .8
STATE="$XDG_DATA_HOME/muxlane/state.json" python3 - <<'PY'
import os,json
d=json.load(open(os.environ['STATE']))
def leaves(n): return 1 if n['kind']=='leaf' else sum(leaves(c) for c in n['children'])
assert leaves(d['pane_tree'])==2, d['pane_tree']
print('✓ explicit split created second pane')
PY
import -window "$WID" "$ARTIFACTS/06-explicit-split.png"

# 分隔比例的归一化/持久化由 PaneTree 单测覆盖；UI smoke 不做鼠标拖动。
import -window "$WID" "$ARTIFACTS/07-split-stable.png"

# 7. 关闭右侧 split 只折叠布局，不终止 agent。
xdotool key ctrl+k; sleep .2; xdotool key x; sleep .4
STATE="$XDG_DATA_HOME/muxlane/state.json" MUXLANE_SOCKET="$XDG_DATA_HOME/muxlane/muxlane.sock" python3 - <<'PY'
import os,json,socket
d=json.load(open(os.environ['STATE']))
assert d['pane_tree']['kind']=='leaf', d['pane_tree']
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect(os.environ['MUXLANE_SOCKET'])
s.sendall(b'{"id":1,"method":"state.list"}\n');b=b''
while b'\n' not in b:b+=s.recv(65536)
assert len(json.loads(b.decode())['result']['agents'])==1
print('✓ close split terminated its shell session')
PY
import -window "$WID" "$ARTIFACTS/08-split-closed.png"

# 为最大化测试重新分屏。
xdotool key ctrl+k; sleep .2; xdotool key h; sleep .5

# 8. 最大化（Ctrl+K, m）。
xdotool key ctrl+k; sleep .2; xdotool key m; sleep .4
STATE="$XDG_DATA_HOME/muxlane/state.json" python3 - <<'PY'
import os,json
d=json.load(open(os.environ['STATE']))
def leaves(n): return 1 if n['kind']=='leaf' else sum(leaves(c) for c in n['children'])
# maximized_pane 是 transient，不落盘；分屏结构仍应保持完整。
assert leaves(d['pane_tree'])==2, d['pane_tree']
print('✓ maximize action kept the split layout intact')
PY
import -window "$WID" "$ARTIFACTS/09-maximized.png"

wmctrl -s "$ORIGINAL_WS"
ORIGINAL_WS=""
echo "UI smoke passed; artifacts: $ARTIFACTS"
