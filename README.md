# lmux

原生 Rust + GPUI 多 Agent 终端客户端。每台机器的 lmux 同时是客户端和服务端；连接远端实例后自动看到该机器上所有项目和会话。

## 当前能力

- GPUI 原生亮色界面：机器/项目/会话树 + 通知中心 + 贴边终端区
- 真终端：`portable-pty` + `alacritty_terminal`
  - `$SHELL`/zsh 正确启动
  - `TERM=xterm-256color`、`COLORTERM=truecolor`
  - ANSI 256 色/truecolor、背景色、bold/italic/underline、可见光标
  - 低延迟同步输入；GPUI InputHandler 支持 Fcitx/IME 中文提交
  - lmux 专用 tmux 开启 mouse/copy-mode 滚动，隐藏 tmux status
  - 真实字体 metrics + force-width shaping；CJK 宽字符、combining marks、行末 cell 与光标对齐
  - 4px terminal viewport 内边距；同一 metrics 驱动 cols/rows、光标、IME bounds 和滚轮坐标
  - 本地 alacritty scrollback；mouse-reporting/alternate-screen 按终端 mode 路由
- 递归 PaneTree：pane 内 tabs；tab 右侧 `＋` 或 Ctrl+Shift+T 创建普通 Shell tab；显式操作才分屏；最大化；tab 拖动重排/跨 pane
- split 支持关闭/递归折叠和拖动 2px 分隔线调整比例，布局与比例重启保留
- 本地会话运行在 lmux 专用 tmux server 中；关闭 GUI 只 detach，重开自动恢复进度
- 侧栏显示 machine/project/session 清晰树线层级和实时 OSC 标题；项目/远程机器提供确认式删除菜单
- 删除项目会销毁该项目 lmux tmux；删除远程机器只忘记本地连接，绝不删除目标机器 session
- 会话右键删除菜单；本地删除会 kill tmux session，远端删除走 `agent.delete`
- 本地 Unix socket API：`state.list`、`term.subscribe`、`events.subscribe`、`agent.report`、`agent.delete`
- SSH 认证支持 SSH config、指定公钥、用户名密码；密码仅驻留当前进程的一次性 askpass secret
- 远端探测区分认证失败/未安装/未启动/离线；确认后可自动上传并启动 `lmux --headless`
- Hook/plugin 权威事件：Claude/Codex/OpenCode/Pi；提取最终 assistant 正文显示在固定通知中心和桌面通知

## 构建运行

```bash
cargo build -p lmux-app
cargo run -p lmux-app
```

使用特定 shell：

```bash
LMUX_SHELL=/usr/bin/zsh cargo run -p lmux-app
```

headless 服务端：

```bash
cargo run -p lmux-app -- --headless
```

连接远端：

```bash
cargo run -p lmux-app -- --connect nuc
cargo run -p lmux-app -- --connect user@192.168.1.20
```

UI 内也可点左下角“连接远程机器”，输入 SSH host 或 `~/.ssh/config` 别名。lmux 自动发现远端 `${XDG_DATA_HOME:-$HOME/.local/share}/lmux/lmux.sock`；直接 socket 路径只作为高级用法保留。

## 打包安装

```bash
scripts/package-linux.sh
# 产物：dist/lmux-0.1.0-linux-x86_64.tar.gz + .sha256

PREFIX="$HOME/.local" packaging/install.sh target/release/lmux
```

当前 release 验收：二进制约 22 MB、压缩包约 8.7 MB；headless `state.list`、desktop entry、安装路径、socket/secret `0600` 均已验证。

## 验证

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/ui-smoke.sh
```

UI smoke 在独立桌面执行，不占用当前工作区，覆盖：

1. zsh + 真彩色 + 光标 + Fcitx 中文输入
2. UI 键盘输入 → InputHandler → PTY → 协议 replay 的端到端验证
3. Ctrl+K 命令面板
4. 会话右键菜单
5. 项目 `＋` 创建会话
6. hook working spinner、done 通知中心
7. 显式分屏、新 panel 为 Shell
8. 关闭 tab/split 只折叠布局但不杀会话、最大化

分隔比例的拖动算法、归一化和非法值由 PaneTree 单测覆盖；UI smoke 不执行鼠标扫描或缓慢拖动。

截图在 [`artifacts/ui-smoke/`](./artifacts/ui-smoke/)。

## 设计文档

- [`../PLAN.md`](../PLAN.md)
- [`../TECH_DESIGN.md`](../TECH_DESIGN.md)
- [`../prototype.html`](../prototype.html)
