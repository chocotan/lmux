# lmux 极致架构审查报告

> **基线**：branch `fix/remote-shell-and-alerts`，HEAD `0238527` + 工作区未提交改动，6 crates / ~17,883 行（不含 target）。
> **审查方式**：3 个独立审查 agent 并行（工作区架构、app.rs 方法级拆解、全仓可维护性），关键结论经人工交叉抽查验证（8 项抽查，7 项属实，1 项误报已剔除）。
> **验证基线命令**（全文统一）：`cargo check --workspace && cargo clippy --workspace --all-targets --message-format short && cargo test --workspace`，涉及 UI 行为的阶段额外跑 `scripts/ui-smoke.sh`。
> **日期**：2026-09-03

---

## 0. 执行摘要

**底层三分之二（core/term/store）是健康的。** 问题集中在一个根因上：

**`LmuxServer` 零封装（字段全 `pub`）+ `lmux-app` 越权直写** →
- server 的 RPC handler 业务逻辑在 UI 层被逐行复制了一份（spawn_preset ≈ AGENT_SPAWN handler；confirm_delete 152 行复写 destroy_sessions），两份实现已经漂移；
- UI 线程 ~20 处 `blocking_lock/blocking_write` tokio 锁 + 自认 hack `futures_lite_block`；
- `app.rs` 膨胀为 6632 行上帝对象（`LmuxApp` 59 字段、`render()` 1332 行），其中 ~900 行业务逻辑完全不可测（全文件仅 5 个单测，全是纯函数）。

先做阶段 1/2（依赖边矫正 + ServerApi 抽取），一半严重问题连带消失，app.rs 自然瘦掉 ~1500 行；之后机械拆文件，风险可控。

### 三方共识区（可信度最高）

| 结论 | 独立提出方 |
|---|---|
| app.rs 用"同 struct 多 `impl` 块分文件"拆（Zed workspace 模式），不引状态管理框架 | 架构审查 + 拆解方案 |
| `LmuxServer` 字段私有化 + 抽 `server/src/api.rs` | 架构审查 + 可维护性 |
| 持久化 4 份拷贝收敛到 `store::PersistedApp::from_snapshot` | 架构审查 + 可维护性 |
| 删 `ui.rs`（未编译的 1 行尸体）、`lmux-client→lmux-server` 移 dev-dependencies | 三方全中 |
| 不拆 core、不并 store、不动 term、不换 RPC 协议、不动 GPUI rev | 三方全中 |

### 唯一分歧及裁决

**通知中心**（app.rs 的 notifications/toasts/toast_seq/error_toast 一组）：
- 架构审查：放 `notifications.rs` 文件模块即可；
- 拆解方案：应拆成**子 Entity**——toast 100ms 动画每帧 `cx.notify()` 触发的是**整窗口重绘**，拆出后只重绘浮层区域，性能收益真实；且输入输出边界干净（输入 `(agent, from, to, message)`，输出仅"点击跳转"一个事件），不落盘不参与 persist。

→ **裁决：先按文件拆（纯移动），在 PR4 单独升级为子 Entity**（定义 `NotificationCenterEvent::JumpToAgent(AgentId)`，LmuxApp 在 `new()` 里 `cx.subscribe` 转发给 `jump_to_agent`）。这是整个拆解序列中唯一含行为语义变更的 PR，可独立回滚。

---

## 第一部分 · 当前架构诊断（按严重度排序）

### 当前依赖图（已逐一验证 Cargo.toml + 源码 use，非猜测）

```
                    ┌──────────────────────────────────────────────────┐
                    │                    lmux-app                      │
                    │  (唯一二进制; bin "lmux"; UI + 业务逻辑大杂烩)      │
                    └──┬───────┬─────────┬─────────┬─────────┬─────────┘
                       │       │         │         │         │
        ┌──────────────▼─┐  ┌──▼──────┐  │    ┌────▼─────┐  ┌▼─────────┐
        │   lmux-client   │  │lmux-term│  │    │lmux-server│ │lmux-store│
        │  RPC + RemoteHost│ │PTY+vterm│  │    │ socket RPC │ │ state.json│
        └──┬────┬────┬────┘  └──┬──────┘  │    └──┬──┬──┬──┘ └──┬───────┘
           │    │    │          │         │       │  │  │        │
           │    │    └──────────│─────────┼───────┘  │  └────────┘
           │    └──► lmux-server│(!!)     │     lmux-term, lmux-store
           │        (仅为 tests/remote.rs) │
           │                             │
           ▼                             ▼
        ┌────────────────────────────────────┐
        │             lmux-core              │
        │ model/protocol/pane/preset/        │
        │ detect/hook/auth (无 lmux 依赖) ✓   │
        └────────────────────────────────────┘
```

分层大方向没错（core 在底、app 在顶、无环），但细节有硬伤。

---

### P0-1 【致命】领域逻辑大规模泄漏进 lmux-app，且与 server RPC handler 逐行重复

app.rs 直接以 `blocking_lock` 操纵 `LmuxServer` 的全部 pub 字段（state/sessions/subs/lifecycle/dirty），把 server 的 RPC handler 逻辑在 UI 层复制了一遍：

| app.rs 位置 | 复制了 server 哪段逻辑 | server 中被复制处 |
|---|---|---|
| `spawn_preset` (1474–1579) | 生成 agent_id → tmux 名 → `LaunchCfg` 注入 `LMUX_AGENT_ID/LMUX_SOCKET/LMUX_HOOK_TOKEN` → `PtySession::spawn` → 写 sessions/state → `dirty.bump()` | `AGENT_SPAWN` handler，lib.rs:379–440，几乎逐行同构 |
| `spawn_shell_for_pane` (1580–1637) | 同上（shell 变体） | 同上 |
| `delete_session` 本地分支 (1869–1902) | sessions.remove → `kill_persistent` → `subs.mark_agent_exit` → `state.remove_agent` → `dirty.bump` | `AGENT_DELETE` handler，lib.rs:462–476 |
| `confirm_delete::LocalProject` (1998–2150, 152 行) | **完整重写** `destroy_sessions`（收集 agent→杀 tmux→区分 destroyed/failed→删 state→条件删 project） | `PROJECT_DELETE` handler + `destroy_sessions()`，lib.rs:216–232, 472–516 |
| `add_local_project` (1809–1852) | canonicalize/查重/默认名 → `add_project` | `PROJECT_ADD` handler，lib.rs:477–511 |
| `mark_agent_working`/`focus_agent`/`activate_tab` | 直写 `state.blocking_write()` 改 status/seen | state.rs 的 `mark_seen/mark_screen_working` |

后果：
1. **同一业务规则两份实现，必然漂移**（已经漂了：app 侧删除不调 `persist_runtime_state`，走 UI 自己的 `persist()`，两条持久化路径写出不同内容）。
2. **UI 线程 ~20 处 `blocking_lock/blocking_write/blocking_read`**（app.rs:747, 1024, 1117, 1284, 1487, 1543, 1558, 1582, 1604, 1611, 1699, 1842–1843, 1898–1902, 2005–2051…）。tokio Mutex 的 `blocking_lock` 在 UI 线程与 async handler 抢锁时卡帧；代码里有自认的 hack：`futures_lite_block`（app.rs:4915–4927，注释「P0 临时：短超时阻塞拿会话表」= try_lock → sleep 2ms → blocking_lock）。若 tokio worker 全忙，UI 线程死锁等待。
3. **`lifecycle` 锁协议被跨层共享**：app 在 UI 线程 `blocking_lock()`（1487, 1582, 2005），server handler 在 async 里 `lock().await`（lib.rs:379, 497）——同一个互斥量两种锁法。

**根因**：`LmuxServer` 所有字段 pub（lib.rs:42–49 实测确认：`pub state / pub runtime / pub sessions / pub subs / pub dirty`）、没有内部 API 层。

### P0-2 【依赖方向错误】lmux-client → lmux-server 是伪依赖

`crates/lmux-client/Cargo.toml:16` 把 `lmux-server` 声明为正式依赖（实测确认），但 `grep -rn lmux_server crates/lmux-client/src/` 结果为空——唯一使用处是集成测试 `tests/remote.rs:4`。后果：client 传递依赖 lmux-store、fs2、bytes，依赖闭包几乎等于整个 workspace；client 是"连远端"的 crate 却编译进了本机 server。应移入 `[dev-dependencies]`（不构成环）。

### P0-3 【职责错位】server 端领域逻辑写在 UI 二进制的 main.rs 里

`main.rs`（423 行）里塞了四段纯服务端逻辑：
- **屏幕状态轮询循环**（main.rs:155–223，~70 行）：每 500ms 遍历 sessions、取 replay tail、`strip_ansi`、喂 `detect::ScreenInput`、处理 exit、写 state、bump dirty——这是 server 的检测引擎驱动器；
- **tmux 会话恢复**（main.rs:258–314）：`tmux has-session` 探活 + 重建 `LaunchCfg`/`AgentInstance`；
- **Hook 注入**（main.rs:226–256）：写 `~/.claude/settings.json`、`~/.codex/config.toml`、装插件；
- **headless 持久化循环**（main.rs:339–370）。

lmux-server crate 有 state.rs/subs.rs，却没有任何 supervisor 模块——「会话生命周期管理」这个 server 的本职散落在 UI 进程入口。

### P1-4 【上帝对象】app.rs 6632 行 / `LmuxApp` 59 字段 / `render()` 1332 行

实测结构：

| 块 | 行数 |
|---|---|
| `pub struct LmuxApp`（字段） | 220–286（59 字段，人工复核确认） |
| `pub fn new`（含 4 个后台 task 装配） | ~384 |
| `render()` 单个方法 | **1332** |
| `render_pane_node` | ~450 |
| `render_settings` | ~419 |
| 各 render_* 对话框/菜单（9 个） | 47+166+105+128+252+186+129+111+139 |
| 业务方法（spawn/delete/bootstrap/remote） | ~900 |

字段混杂至少 12 个正交关注点（见第二部分 §1 字段分组表）。

**可测试性后果**：全文件仅 5 个单测且全是纯函数（`effective_notification_body`、`format_upload_phase`、`resolve_local_project_path`…）。spawn 管线、删除级联、通知去重、palette 模糊匹配、远程重建——全部需要在 GPUI 窗口里才能触达，等于零覆盖。

### P1-5 【协议助手放错 crate】client 依赖 term 只为 base64

`lmux-client/src/lib.rs` 对 `lmux_term` 的全部使用是 `b64_encode/b64_decode`（156, 166, 172, 256 行，实测确认）。而 `b64_*`、`strip_ansi`、`extract_osc_title` 定义在 `lmux-term/src/lib.rs:17–124`，注释自称「wire 协议用」。**wire 编解码助手住在终端模拟 crate**，导致 client 为 base64 拖进 portable-pty + alacritty_terminal。这三个函数应迁往 `lmux-core::protocol`（`strip_ansi`/`extract_osc_title` 本来就是 detect 的输入预处理，main.rs:172–180 正是把 term 的输出喂给 core 的 detect）。

### P1-6 【持久化四份拷贝】Snapshot→PersistedApp 转换重复 4 处

同一「projects 清空 agents + sessions 取 tmux_session + 写盘」转换逻辑：
- `app.rs:768–843 persist()`
- `main.rs:315–334`（启动即落盘）
- `main.rs:344–370`（headless 循环，第三份）
- `server/lib.rs:125–158 persist_runtime_state()`

`PersistedSession` 构造出现 4 次（app.rs:819, main.rs:327, main.rs:358, server lib.rs:143）。应收敛到 store：`PersistedApp::from_snapshot(&Snapshot)`，UI 偏好由 app 覆盖。**（可维护性审查 H1 独立确认同结论）**

### P1-7 【错误字符串当协议】远程版本兼容靠 `contains` 中文字符串分派

- app.rs:3677：`text.contains("远端 lmux 版本过旧")` → 触发升级引导
- app.rs:3739：`text.contains("unknown_method") && text.contains("project.add")` → 转升级流程

（行号以当前工作树实测为准。）错误在 client（anyhow string）→ UI 再解析人类可读文案做控制流。改文案即破坏逻辑，且英文 locale 下直接失效。client 应定义类型化错误 `RemoteCompatError::VersionSkew { expected, actual }` / `MethodUnsupported(method)`。

### P2-8 其他结构性问题

1. **ui.rs 是 1 行死文件**（实测确认）：内容为「语法错误太多，此文件作为 P0 的 UI 模块占位（将在下一步重写）」，未被任何 `mod` 声明引用，**根本未被编译**。
2. **`data_dir()` 双实现**：main.rs:32（私有）与 tunnel.rs:163（`pub fn`，public API！）各自实现 XDG 逻辑，逐字相同。应在 core 提供 `lmux_core::paths::data_dir()`。
3. **i18n 半途而废**：`i18n::text()` 仅 19 处调用，app.rs 内 **128 处硬编码中文**（含 `"取消"`×5、`"设置"`×2、`format_relative_time` 内部按语言硬编码两套格式串）。要么贯彻、要么删掉 i18n，混合状态最差。
4. **`Connection::call` 丢弃事件帧**（client/lib.rs:63–70：`if v.get("event").is_some() { continue; }`）：call 期间到达的事件被静默吞掉；且无请求超时，对端不回包则永久挂起。
5. **bootstrap 二进制定位靠猜**（tunnel.rs:269–277）：`current_exe()` → 猜 `../release/lmux` → fallback 自身，叠加 `LMUX_BOOTSTRAP_BINARY` env，三层启发式。
6. **明文密码入库**：`PersistedRemoteAuth::Password { password: Option<String> }`（store/lib.rs:74–80）直接 JSON 落盘 state.json，缺 secret provider 抽象（keyring / 至少 chmod 600 单独文件）。
7. **通知/声音策略在 UI 内**：`push_notification`（app.rs:872–995, 123 行）做去重/静音/落通知中心，其中「blocked/done 才进通知中心」这类策略是纯逻辑，可下沉测起来。
8. **CI 只有 release.yml 且 untracked**：没有任何 `cargo test/clippy/fmt` 门禁——这就是 app.rs 能烂到 6632 行的直接原因。
9. **`LmuxServer` 三级构造函数链**：`new → new_with_runtime → new_with_runtime_and_auth`，中间级除被 `new` 调用外全仓库 0 引用。
10. **`#[allow(dead_code)]` 滥用**：tunnel.rs:2 文件级、app.rs:340 `Notification`、app.rs:6341 `AttentionStyle.border_color`（被 compute 但从未被渲染消费——"计算了白算"）、theme.rs:80 `Theme.mode` 无人读。

---

## 第二部分 · LmuxApp 拆解方案（方法级）

### §1 字段分组（app.rs 220–286，59 字段）

| 组 | 字段 | 落盘？ |
|---|---|---|
| A. 环境/基础设施 | `focus`, `server`, `store_path`, `remote_event_tx` | store_path 间接 |
| B. Pane/Tab 布局 | `pane_tree`, `active_pane`, `maximized_pane`, `active`, `split_drag`, `split_metrics` | pane_tree/active_pane 是；maximized 声明 transient 不落 |
| C. 终端缓存 | `terms`, `mirror_cancel` | 否 |
| D. 快照/远程镜像 | `last_snapshot`, `remotes`, `remote_snaps`, `remote_states`, `bootstrap_progress` | last_snapshot/remotes 是 |
| E. 通知中心 | `notifications`, `toasts`, `toast_seq`, `notifications_open`, `error_toast` | 否 |
| F. 外观设置 | `theme_mode`, `font_family`, `language`, `sound_enabled` | 全部是 |
| G. 设置面板 | `settings_open`, `settings_theme_menu`, `settings_font_menu`, `settings_language_menu` | 否 |
| H. 命令面板 | `palette_open`, `palette_index`, `palette_scroll`, `palette_input`, `presets`, `new_session_target` | 否 |
| I. 连接对话框 | `connect_dialog`, `connect_input`, `connect_auth_mode`, `connect_focus_index`, `connect_username`, `connect_password`, `connect_key_path` | 否 |
| J. 项目对话框 | `project_dialog`, `remote_project_dialog`, `remote_project_input`, `project_input`, `dialog_error` | 否 |
| K. 菜单/确认框 | `session_menu`, `tree_menu`, `delete_confirm`, `delete_error`, `bootstrap_confirm`, `bootstrap_error` | 否 |
| L. 动画/侧栏 | `spinner_frame`, `pulse_phase`, `collapsed_machines`, `collapsed_projects` | 否 |

### §2 目标文件结构（app.rs → app/ 目录）

`src/app.rs` → `src/app/` 目录，`mod.rs` 保留 struct 定义、`new`、`persist`、`find_agent`、`should_animate`、Render impl 骨架。

| 新文件 | 职责 | 迁入字段 | 迁入方法（当前行号） | 迁入自由函数/类型 |
|---|---|---|---|---|
| `icons.rs` | SVG 资产注册与图标查询 | — | — | 10 个 `*_ICON` 常量、`SVG_ASSETS`、`svg_asset()`(77)、`panel_icon()`(81) |
| `widgets.rs` | 与状态无关的渲染小件与格式化 | — | — | `truncate`(6424)、`format_bytes`(6433)、`format_upload_phase`(6450)、`format_relative_time`(4928)、`render_pi_loading_spinner`(6297)、`render_status_indicator`(6385)、`compute_attention_style`(6349)、`AttentionStyle`(6342)、`DragGhost`(94)、`DividerDragGhost`(121) 的 Render impl |
| `app/panes.rs` | pane 树/tab 导航/分屏拖拽状态机 | B 组（声明留 mod.rs） | `activate_tab`(996)、`select_tab_n/next_tab/prev_tab/cycle_active_pane`(1307-1393)、`close_tab`(1427)、`move_dragged_tab`(1456)、`spawn_shell_for_pane`(1580)、`new_shell_tab`(1638)、`split_pane`(1650)、`toggle_maximize`(1671)、`close_split_pane`(4369)、`start_split_drag`(4406)、`update_split_drag`(4424)、`end_split_drag`(4458)、`render_pane_node`(4464-4914) | `DragTab`(89)、`SplitDrag`(114)、`DividerDrag`(119)、`shell_split_launch_cfg`(138) |
| `app/sessions.rs` | Agent 会话打开/聚焦/删除/term 缓存 | C 组 | `mark_agent_working`(1034)、`create_local_term`(1068)、`create_remote_term`(1087)、`open_agent`(1109)、`open_remote_agent`(1139)、`focus_agent`(1254)、`jump_to_agent`(1294)、`delete_session`(1869)、`cleanup_removed_agents`(1924) | `futures_lite_block`(4915)（阶段 2 后删除） |
| `app/remotes.rs` | 远程主机接入/引导/删除流程 | D 组 | `add_remote_target`(1681)、`begin_delete`(1968)、`confirm_delete`(1998)、`cancel_bootstrap_for_host`(2472)、`confirm_bootstrap`(2484)、`spawn_remote_agent`(3568)、`submit_remote_project`(3649) | — |
| `app/notifications.rs`（→ 子 Entity） | 通知/toast/声音/桌面通知生命周期 | E 组（移入新 struct） | `push_notification`(872)（签名改造，见 §4） | `Notification`(341)、`ToastNotification`(354)、`effective_notification_body`(179)、`render_notifications_popover`(2708) |
| `app/palette.rs` | 命令面板状态与执行 | H 组（不动，impl 分文件） | `palette_project_path`(3956)、`compute_palette_items`(3992)、`execute_palette_item`(4096)、`handle_palette_key`(4156)、`render_palette`(4229) | `PaletteItem`(364)、`NewSessionTarget`(212) |
| `app/dialogs.rs` | 连接/本地项目/远程项目三个输入对话框 | I、J 组 | `cycle_connect_focus`(1751)、`handle_connect_key`(1784)、`add_local_project`(1809)、`handle_project_key`(1853)、`render_connect_dialog`(3381)、`render_remote_project_dialog`(3714)、`render_project_dialog`(3844) | `ConnectAuthMode`(205)、`resolve_local_project_path`(162) |
| `app/menus.rs` | 右键菜单与两个模态确认框 | K 组 | `render_session_menu`(2151)、`render_tree_menu`(2199)、`render_delete_confirm`(2366)、`render_bootstrap_confirm`(2542) | `SessionMenu`(287)、`DeleteTarget`(294)、`TreeMenu`(310)、`dismiss_context_menus`(315)、`DeleteConfirm`(326)、`BootstrapConfirm`(332) |
| `app/settings.rs` | 设置页与外观切换 | F、G 组 | `toggle_theme`(1394)、`apply_theme_to_inputs`(1394-1426)、`dismiss_settings_menus`(2671)、`set_theme/set_font_family/set_language`(2677-2708)、`render_settings`(2961-3380) | `FONT_FAMILIES`(72)、`DEFAULT_FONT_FAMILY` |
| `app/sidebar.rs` | 侧栏机器树与底部按钮条渲染 | L 组 | render() 内联树渲染整体抽出：`render_machine_tree`、`render_sidebar_footer`（源码 5011-5770、5944-6125） | — |

`render()` 最终只剩：根布局、18 个 `on_action` 绑定（5797-5898）、全局 `on_key_down` 分发（5900-5927）、分屏拖拽全局监听（5928-5943）、浮层挂载（6243-6278），目标 **<300 行**。

### §3 拆分策略与 GPUI 取舍

**第一级（主体，占 80% 工作量）：「字段留主体 + impl 分文件」。**
Rust 允许同一 struct 的多个 `impl` 块分布在不同文件（`impl LmuxApp { ... }` 在 panes.rs 里，字段仍定义于 app/mod.rs）。零行为变更的纯移动：`cx.listener(|this, ...|)` 闭包仍拿到 `&mut LmuxApp`，`cx.subscribe`/`cx.spawn` 的 `this.update` 目标类型不变，`persist()` 仍能读到所有字段。**GPUI 取舍**：cx.listener 的强类型绑定天然反对把 UI 回调搬出主体——palette 的 `execute_palette_item` 直接调 `split_pane/close_split_pane/toggle_maximize/toggle_theme`，若拆成子 Entity 要么定义 20+ 个事件要么造命令 enum 再回流，纯粹搬运复杂度。Zed 自身（workspace.rs 数千行 + 分域 impl）就是这个模式。

**第二级（唯一推荐拆出的子 Entity）：通知中心。**
理由：(a) 输入输出边界干净；(b) 独立 `cx.notify()` 有真实性能收益（toast 动画不再触发整窗口重绘）；(c) 不落盘、不参与 persist。跳转事件定义 `NotificationCenterEvent::JumpToAgent(AgentId)`。

**第三级（渲染辅助模块）：widgets.rs / sidebar.rs。**
`render_pane_node`、机器树渲染保持为 `impl LmuxApp` 方法（listener 需要 `cx` + `this`），只是物理搬家；`render_status_indicator` 等纯函数接收 `&Theme` 值参数，放 widgets.rs 无约束。

**明确不拆成 Entity 的**：palette（回调面太宽）、三个对话框（`dialog_error` 共享错误槽，提交逻辑深度耦合 `remotes`/`last_snapshot`/`server`）、设置页（纯外观）。

### §4 具体风险点

1. **persist() 序列化面**：读 `last_snapshot`、`remotes`→`PersistedRemote`（auth 枚举双向映射，768-842）、`pane_tree`、`active_pane`、外观四字段。字段声明留在 app/mod.rs 则 persist 不用改。PR4 后确认 persist 不引用通知字段（当前确实不引用，安全）。
2. **cx.subscribe 归属**：`create_local_term`(1077)/`create_remote_term`(1099) 内 `cx.subscribe(&term, … mark_agent_working)` 的订阅者是 LmuxApp，两方法与 `mark_agent_working` 必须同在 sessions.rs（或保持 pub(crate) 可见）。
3. **deferred/后台任务的 self 引用**：`new()` 里 4 个 `cx.spawn` 循环（本地事件泵、秒级轮询、动画 tick、远程事件泵）持有 `WeakEntity<LmuxApp>`——**new() 不搬**。动画 tick 里的 `toasts.retain`/`error_toast` 清理在 PR4 后改为 `this.notifications.update(cx, …)`，`should_animate` 拆成"app 侧 attention 查询 + 通知侧 has_activity"。
4. **push_notification 签名改造**（PR4 唯一非纯移动）：拆出后在**调用点**先用留在 app 侧的 helper 解析 `(machine_name, project_name, agent_type, focused)`，组装 `NotificationDraft` 传给子 Entity；`sound_enabled` 作参数传入或在子 Entity 存快照 + setter。
5. **tests 模块迁移映射**（全部 `use super::*` 即可无缝跟随）：notification_body_* → notifications.rs；upload_progress_text_* → widgets.rs；local_project_path_* → dialogs.rs；dismissing_context_menus_* → menus.rs；explicit_split_launch_config_* → panes.rs。
6. **render() 切分顺序**：先叶后干——(1) 浮层已独立直接搬；(2) 机器树 ~760 行内联块抽 `render_machine_tree`（保持 `&mut self` 方法形态，不要改成传参自由函数）；(3) 底部按钮条；(4) `render_pane_node` 随 PR2 走。根部 action 绑定与 on_key_down **永远留在 mod.rs**。
7. **UI 冒烟依赖**：`LMUX_TEST_AUTO_OPEN`（app.rs:760）与 `scripts/ui-smoke.sh` 的像素断言是主要回归网；smoke 明确**不覆盖**分隔线鼠标拖拽，PR2 后需一次人工拖拽验证。

### §5 PR 迁移序列（7 个）

| PR | 内容 | 预计 diff | 验证 |
|---|---|---|---|
| **PR1 资产与纯函数下沉** | icons.rs + widgets.rs 全部条目，main.rs 改 import；删除 ui.rs | ~800 行（纯移动） | cargo check/test/clippy -p lmux-app；smoke 全量 |
| **PR2 panes.rs** | panes 全部方法 + 类型 + 测试 | ~1100 行 | cargo test；smoke（split/maximize/close-tab）；**人工：拖分隔线、双击等分** |
| **PR3 sessions.rs + remotes.rs** | 两模块全部条目 | ~1400 行 | cargo test + smoke；人工：连接对话框打开/取消、bootstrap 引导 |
| **PR4 NotificationCenter 子 Entity** | E 组字段迁出 + push_notification 签名改造 + JumpToAgent 事件 + 动画 tick 改造 | ~650 行（含 ~80 行真实重构） | cargo test；smoke 的 02-working-spinner/done-notification 段；人工：toast 6s 消失、点击跳转、聚焦 agent 不弹 toast |
| **PR5 palette.rs** | H 组方法 + 类型 | ~800 行 | smoke 的 ctrl+k 序列是现成 E2E 回归 |
| **PR6 dialogs.rs + menus.rs + settings.rs** | 三模块全部条目 + 测试 | ~1700 行 | cargo test + clippy；人工：设置下拉互斥、主题切换联动、Tab/Shift-Tab 循环、右键菜单、删除确认 |
| **PR7 sidebar.rs + render 收缩 + mod.rs 定稿** | 机器树/底部按钮条抽出；new() 内 remotes 恢复段抽 `restore_remotes()`；字段按 §1 分组重排加注释 | ~1000 行 | cargo test + smoke 全量；人工：树折叠、hover 才显示的 + 按钮、远程 remediation 徽标 |

原则：**每个 PR 是纯移动或单一重构，绝不混合**。

### §6 term_view.rs / text_field.rs 审查结论

**term_view.rs（1327 行）**：已是规范的独立 Entity（`EventEmitter<TermEnterEvent>` + Focusable + 专属 InputHandler），与 app.rs 拆分解耦，**不必随动**。三个顺带问题：
1. `Render::render`（723-1180，~450 行）里 canvas paint 闭包与滚动条/鼠标报告/IME 混在一起——可后续把 paint 闭包抽为 `paint.rs` 自由函数，中优先级；
2. `new_remote` 用 `cx.spawn(std::future::pending)` 占位 `_drain` 字段——建议改 `Option<Task>` 或至少加注释；
3. `render` 内 `writer.set_focused(focused)` 是渲染期副作用，GPUI 惯例上应移到 focus 订阅，属既有债务。

**text_field.rs（674 行）**：独立 Entity，职责单一（单行输入 + utf16/IME 正确处理），无需处理。唯一不一致：用 `dark_mode: bool` 而非接收 `Theme`，低优先级。

---

## 第三部分 · 全仓可维护性发现

### 高严重度（补充第一部分的增量发现）

**H2. `handle_conn` 巨石函数 352 行、嵌套达 17 个缩进级**（server/lib.rs:240-591）：13 个 RPC 方法内联在一个 match 里，`AGENT_SPAWN` 分支 ~75 行、`PROJECT_DELETE` ~55 行。→ 拆成 `handle_hello/handle_spawn/...` 独立方法，`handle_conn` 只做 dispatch 与帧循环（阶段 2 抽 API 后顺带完成）。

**H3. 远程键盘输入每次击键新建一条 socket 连接**（client/lib.rs:146/204/251/269/283/300/314）：`send_term_input`/`resize_term`/`delete_agent`/`add_project`/`delete_project`/`spawn_agent`/`fetch_snapshot` 每次都 `open(socket)` 新建 UnixStream。远程输入路径（app.rs:1196）走这条：每批按键 = 隧道 socket 新建 + 请求 + 关闭。→ `RemoteHost` 维护长连接或连接池，per-op helpers 改收 `&mut Connection`。

**H4. `read_frame` 逐字节读取**（core/protocol.rs:227-240）：`let mut one = [0u8; 1]` 每字节一次 `read()`。服务端包了 BufReader（lib.rs:252），但客户端 `Connection::call` 直读裸 stream（lib.rs:63），`stream_term` 的 `ResponseReader` 同样裸读（lib.rs:126）——term.data 高频流按字节 syscall。→ `read_frame` 改收 `&mut BufReader`，客户端两处包 BufReader。

**H5. async 上下文执行阻塞 fork/tmux 子进程命令**（server/lib.rs:424）：`PtySession::spawn(cfg)` 在 `handle_conn` 的 async fn 里直接执行——内部含 `configure_tmux_server()`（session.rs:434-452，同步跑 6 次 `std::process::Command`）、`update_tmux_environment()`（每个 env var 一次 tmux 命令）。`kill_persistent()` 同样（lib.rs:219/450）。tmux 慢/卡时整个 tokio worker 被占。→ spawn/kill 路径包 `tokio::task::spawn_blocking`。

**H6. 协议无前向兼容**：`AgentType`/`AgentStatus`/`SplitAxis`（core/model.rs:39/77、pane.rs:11）serde 无 unknown fallback——远端新版本新增枚举值后，旧客户端整个快照解析失败而非降级。协议版本是 magic number：server lib.rs:281 `protocol: 2` 与 client host.rs:584 `hello.protocol >= 2` 硬编码，无 `PROTOCOL_VERSION` 常量；features 比较是裸字符串。→ 定义常量模块 + 关键 enum 加 `#[serde(other)]` 兜底变体。

**H7. 死代码清单**（被 `#[allow]` 压住的警告）：
- `ui.rs`（1 行，未编译尸体）
- `tunnel.rs:547-616` `start_tunnel_socat`（~70 行 socat 回退）零调用者
- `tunnel.rs:624-633` `sanitize` 零调用者（且与 session.rs:513 `sanitize_tmux_name` 重复实现）
- `tunnel.rs:209-212` `RemoteProbe` 单变体枚举（过度建模残留）
- `term/session.rs:129/277` `reader_handle: Mutex<Option<JoinHandle>>` 只写不读，线程从不 join
- `server/state.rs:196-201` `apply_screen_update` 零调用者，且是无任何 await 的伪 async fn（`report_hook` state.rs:161 同病）
- `core/lib.rs:8` 根级 re-export `StatusUpdate`/`ScreenStatusUpdate`/`HookEvent` 全 workspace 0 外部引用
- `core/hook.rs:125-145/174-190` `uninstall_claude_hooks`/`uninstall_codex_notify` 只有测试调用——hooks 注入是**单向门**，无产品路径执行卸载
- `client/lib.rs:100-105` `RequestWriter::raw` 零调用者；`term/session.rs:268-272` `pub async fn write` 注释自认"兼容旧测试"

**H8. i18n 半途而废**：见 P2-8.3。

**H9. 本地/远程侧栏树渲染 ~140 行近似复制粘贴**（app.rs:5100-5200 vs 5600-5760）：agent row 的 div 构造除 `open_agent` vs `open_remote_agent`、`session_menu.remote` 布尔值和 id 前缀外逐字相同；project 行 `＋` 按钮两个版本 ~50 行重复。→ 提取 `render_agent_row`/`render_project_row`，本地/远程只传回调差异。

**H10. `LmuxServer` 三级构造函数链**：见 P2-8.9。

### 中严重度

**M1. 术语一套概念四种叫法**：模型 `AgentInstance` / 持久化 `PersistedSession` / UI 中文"会话" / 右键菜单 `SessionMenu` / 事件 `agent.status_changed`。"host" 概念 4 个名字：`RemoteHost.cfg.name`、`Target::Ssh{host}`、`machine_id`/`machine_name`、`endpoint`——app.rs:1158-1165 靠遍历 `remote_snaps` 反查 "agent→host" 正是这种混乱的代价。→ 写一页术语表；`PersistedSession` 改名 `PersistedAgent`。

**M2. AgentId 字符串前缀成为隐式跨模块协议**：`detect/mod.rs:236-239` 用 `agent.split('_').next()` 从 ID 反推 agent 类型——"`<type>_<ulid>`" 格式是 core/term/server 三方隐式契约。前缀字符串散布 9 处。→ `AgentInstance` 已有 `agent_type` 字段，检测入口直接传类型，删除前缀解析。

**M3. 重复实现的工具函数**：`data_dir()` 双实现（main.rs:32 vs tunnel.rs:163）；`default_true()` 双实现（model.rs:112 vs preset.rs:21）；`now_secs()`（model.rs:145）与 `now_millis()`（session.rs:526）分散两 crate；sanitize 双实现（H7）。

**M4. host.rs `run_loop` 230 行、嵌套 14 级**（client/host.rs:506-735）：NeedsInstall/NeedsStart/AuthenticationFailed/NeedsUpgrade 四分支各复制一份 `tokio::select! { sleep 30s / retry.notified() }`；`install_and_start`(410-440) 与 `upgrade_and_retry`(455-487) 除一行外逐行相同。→ 提取 `wait_retry_or_30s()` 与 `upload_then_start(install_fn)`。

**M5. 双向 `SshAuth ↔ PersistedRemoteAuth` 映射散布两处手写 match**（app.rs:483-505 加载 / 778-810 持久化）：加一种认证方式要同步改 4 个臂。→ `From`/`Into` 实现放 client 或 store 一侧。

**M6. 状态刷新"1s 全量轮询 + 事件广播"双通道互相踩**（app.rs:417-431）：轮询每秒全量 clone `Snapshot`，覆盖了事件通道刚做的 `seen` 优化（app.rs:553 的 `snap.agent_mut(active)` 手工修补是证据）。→ 本地状态走事件通道 + dirty 触发定向重拉，删 1s 定时轮询。

**M7. main.rs 500ms 巡检循环持 sessions 锁做 CPU 密集工作**（main.rs:160-190）：锁内对每个 session 的 64KB replay tail 做 `strip_ansi` + `extract_osc_title`，锁住期间 server 的 `term.subscribe`/`term.input` 全部排队。→ 锁内只 clone replay snapshot，锁外做解析。

**M8. UI 线程 blocking 锁**：见 P0-1（20 处 + `futures_lite_block` hack，实测 17 处 blocking_lock/blocking_write，统计口径差异不影响结论）。

**M9. 远程镜像流喂 VTerm 却不触发 UI 重绘**（app.rs:1213-1240）：mirror task 在 `rt_spawn` 里 `vterm.feed(bytes)`，**没有对应的 `cx.notify()`**；重绘依赖 1s 轮询或 100ms 动画循环"碰巧"触发。本地 TermView 是有 `cx.notify()` 的（term_view.rs:305-310）。远程终端输出延迟是"最多 1s、忙碌时 100ms"的未声明行为。→ mirror task 的 `on_update` 回调里通过 entity weak handle 触发 `cx.notify()`。**（潜伏 bug，建议阶段 4 修掉）**

**M10. 后台任务生命周期无管理**：mirror task 与 remote 命令泵永生，仅靠 `mirror_cancel` AtomicBool 手工取消；`sound.rs:25/85` 每次播放/桌面通知 `std::thread::spawn` 新线程（突发 50 条通知 = 50 线程）；server 每 4ms 的 subs 泵与每连接 reader task 的 JoinHandle 全部丢弃。→ TaskGroup/cancellation token 统一管理；sound 用单专用线程 + channel。

**M11. 依赖声明与实际使用脱节**：server 的 `thiserror/ulid/base64` 在 src/ 中 0 处使用；term 的 `ulid/thiserror` 0 处使用；client 的 `lmux-server` 是正式依赖但 src/ 0 引用。

**M12. 事件/方法名常量体系只覆盖一半**：常量存在于 protocol.rs:66-94，但 integration.rs:226 裸写 `"term.data"`、:290 裸写 `"agent.status_changed"`；features 比较两边裸字符串。→ 补 `features::*` 常量；测试改用常量。

**M13. hook.rs 单文件 597 行混四种关注点**：`#[cfg(test)] mod tests` 插在文件中间（261-349）；三个 JS 字符串（REPORT_SCRIPT/OPENCODE_PLUGIN/PI_EXTENSION）各自复制了相同的 `lmuxEnv()` 和 `report()` 实现 ~40 行 ×3。→ JS 脚本移到 `hooks/` 目录 `include_str!`；Rust 侧拆 `hook/claude.rs`、`hook/codex.rs`、`hook/plugins.rs`。

**M14. `BootstrapProgress` 位打包进单个 AtomicU64**（client/host.rs:196-228）：`(overall << 8) | phase` 编码 + 解码还丢失 `done_bytes/total_bytes`。为省一把锁引入手工位布局。→ 换 `Mutex<Option<BootstrapProgress>>`。

**M15. hook.rs 死参数**：`claude_hooks_value`（hook.rs:26-29）的 `_agent_id`、`_socket` 已废弃但保留在签名里，调用方还在认真传值。

### 低严重度

**L1. 测试覆盖结构性缺口**（各 crate 测试数：core 39 / term 29 / server 18 / client 12 / store 5 / app 16）：
- host.rs `run_loop` 重连/退避/NeedsX 状态机零测试（可对 `ensure_tunnel` 抽 trait 后 mock）
- tunnel.rs `ssh_command` 参数构造无 golden 测试
- `DetectionEngine::HOOK_AUTHORITY_WINDOW`（5 分钟窗口）无时间推进测试
- subs.rs `needs_resync` 置位后 sink 永远 Full 的死锁路径未测
- app 侧 `cleanup_removed_agents`/`move_dragged_tab` 等组合逻辑无测（pane 树本体有 core 的优秀单测兜底）

**L2. 集成测试任务泄漏**：server/tests/integration.rs 12 个测试各自 `tokio::spawn(serve())`，仅一个 abort 了 task。→ `spawn_server` 返回 guard，Drop 时 abort。

**L3. `Connection::call` 静默丢弃事件帧 + 无超时**：见 P2-8.4。

**L4. `VTerm` 所有方法吞 poison 锁**（vterm.rs）：`.lock().map(...).unwrap_or_default()` ×10 处——锁中毒后所有查询静默返回默认值（空屏幕），排障极难。→ poison 时 `tracing::error!` 一次。

**L5. 注释考古化与中英混用**：大量"参考 muxel/herdr/remote-agent/pocket-studio"与 "P0/P1/P3" 里程碑标记描述历史决策时点而非当下代码。→ 设计出处集中到 docs/，代码注释只留当下语义。

**L6. 杂项**：`hostname()`（main.rs:44-64）先查 Windows 的 `COMPUTERNAME` 再查 `HOSTNAME`，linux 下前两个几乎必空；hello features 列表只登 4 项，`term.subscribe`/`events.subscribe` 恒支持却未登；`MAX_FRAME` 1MiB 上限与 512KB replay 的组合上界无回归测试锁定。

### 残余风险（未深挖，交后续）

- `panic = "abort"`（根 Cargo.toml:50）+ PTY reader 线程：任何子线程 panic 直接杀整个 GUI 进程。
- 密码经 `ssh-secret-*` 临时文件 + askpass（tunnel.rs:43-68）的时序窗口，未做安全评审。
- gpui pinned 到 zed 仓库单 commit，上游 API 漂移无防护策略。

---

## 第四部分 · 目标架构

### 目标 crate 图（箭头 = 依赖）

```
                              ┌────────────────────────────┐
                              │         lmux-app           │
                              │  bin "lmux"：纯 UI + 组装    │
                              │  不再直写 server 内部字段     │
                              └──┬──────┬──────────┬───────┘
                                 │      │          │
                 ┌───────────────▼──┐ ┌─▼────────┐ │
                 │    lmux-server    │ │lmux-store│ │
                 │  api.rs (ServerApi)│ │(不变)    │ │
                 │  supervisor.rs ◄──┼─┤          │ │
                 │  rpc.rs(薄 handler)│ └──────────┘ │
                 └──┬──────┬─────────┘              │
                    │      │        ┌───────────────▼──┐
                    │      │        │   lmux-client     │
                    │      │        │ rpc.rs(Connection)│
                    │      │        │ remote.rs         │
                    │      │        │ term_stream.rs ◄──┼─ (镜像流+重连，从 app 下沉)
                    │      │        │ tunnel.rs/bootstrap│
                    │      │        └────┬──────────────┘
                    │      │             │   ✂ 删除 client→server、client→term 两条边
                    │      │             │
              ┌─────▼──────▼─────────────▼────┐
              │           lmux-core           │
              │ model/protocol(+b64)/detect    │
              │ pane/preset/hook/auth/paths    │
              └──────────────┬────────────────┘
                             │
                      ┌──────▼──────┐
                      │  lmux-term  │   (PTY/vterm/tmux，内聚，基本不动)
                      └─────────────┘
```

变化清单：
1. ✂ `client → server` 移入 `[dev-dependencies]`；
2. ✂ `client → term` 删除（b64/strip_ansi/extract_osc_title 迁 `core::protocol`）；
3. ➕ `server::api`：LmuxServer 字段私有化，暴露 async 命令方法；
4. ➕ `server::supervisor`：承接 main.rs 的屏幕轮询/tmux 恢复/exit 清理；
5. ➕ `client::term_stream`：承接 `open_remote_agent` 的镜像 attach 循环（含重连退避与 5ms 输入合批）；
6. ➕ `core::paths`：唯一 `data_dir()`；
7. store 保留为叶子 crate。

### 新增模块规格

- `server/src/api.rs` ~300 行：`spawn_agent(cfg)→AgentInstance`、`delete_agent`、`delete_project→DeleteScopeResult`、`add_project`、`mark_seen`、`mark_working`、`persist()`；内部持 lifecycle 锁；现有 RPC handler 全部改为一行委托（handle_conn 从 ~350 行降到 ~180 行）。
- `server/src/supervisor.rs` ~200 行：run_screen_poll / restore_tmux_sessions。
- `client/src/term_stream.rs` ~150 行：`run_term_mirror(remote, agent, vterm, cancel)`（含 5ms 输入合批与 `cx.notify` 触发）。
- `core/src/protocol.rs` 追加 b64 助手（或独立 `core/src/wire.rs` ~130 行）。

### 拆分后可测清单（现阶段全测不了 → 拆后纯 Rust 单测）

spawn 管线（preset→LaunchCfg→env 注入→AgentInstance）、删除级联（destroyed/failed 语义）、Snapshot→PersistedApp 转换与迁移、通知去重/seen 策略、palette 检索排序、remote 重建映射、镜像流重连退避（mock endpoint）。

---

## 第五部分 · 统一执行序列

### 阶段 0：止血 + 死代码（半天，风险≈0）
- 删除 `src/ui.rs`（未编译占位）
- 删除死代码：`start_tunnel_socat`、`sanitize`（tunnel.rs:547/624）、`reader_handle` 字段（session.rs:129/277）、`apply_screen_update`（state.rs:196）、`RequestWriter::raw`（client lib.rs:100）、`PtySession::write`（session.rs:268）、`RemoteProbe` 单变体枚举降级、core lib.rs 未用 re-export
- 清理未用依赖：server 的 `thiserror/ulid/base64`、term 的 `ulid/thiserror`
- 4 个 `#[allow(dead_code)]` 逐个换成"使用或删除"（`AttentionStyle.border_color`、`Theme.mode` 删字段；uninstall hooks 接 `--uninstall-hooks` CLI）
- 验证：基线命令 + `cargo build -p lmux-app`

### 阶段 1：依赖边矫正（1 天，风险低）
- core：protocol.rs 增加 `b64_encode/b64_decode`；`strip_ansi/extract_osc_title` 迁入 core（term 保留 `pub use` 转发一个版本周期，或直接一次改完——调用点只有 main.rs:172–180 和 client/lib.rs 4 处）
- client：`lmux-server` 移 dev-deps；删除对 `lmux-term` 的依赖
- core：加 `paths::data_dir()`；main.rs:32 与 tunnel.rs:163 改委托
- 验证：基线命令；`cargo tree -p lmux-client --depth 1` 确认无 server/term

### 阶段 2：ServerApi 抽取（2–3 天，**本重构的核心**，风险中）
- 新建 `server/src/api.rs`：lib.rs:379–516 的 AGENT_SPAWN/AGENT_DELETE/PROJECT_ADD/PROJECT_DELETE handler 主体提为 async 方法（内部统一持 lifecycle 锁、统一 persist_runtime_state）；RPC handler 改一行委托（顺带拆 handle_conn 的 13 个 match 臂为独立 handle_* 方法，解决 H2）
- `LmuxServer` 字段改 `pub(crate)`/私有，对外只留 API + 订阅句柄 + socket_path
- app.rs 六处直写点改调 API：spawn_preset/spawn_shell_for_pane → `cx.background_spawn(server.spawn_agent(...))`；delete_session/confirm_delete → `delete_agent/delete_project`（删 152 行级联复写）；add_local_project → `add_project`；mark_agent_working/focus_agent 的 blocking_write → `mark_working/mark_seen`
- 删除 `futures_lite_block` 与全部 UI 线程 blocking 锁（~20 处清零）
- **风险**：spawn 从同步变异步，~5 处调用点改 `cx.spawn` 回调式；focus 时机需在 await 后执行——逐点过，别用 block_on 倒退
- 验证：基线命令 + ui-smoke.sh（spawn shell、删 tab、删 project、palette spawn 四路径手测）；`grep -rn "blocking_lock\|blocking_write\|futures_lite_block" crates/lmux-app/src` 为零

### 阶段 3：持久化收敛 + supervisor 下沉（1–2 天，风险低-中）
- store：`PersistedApp::from_snapshot(&Snapshot)` + `set_ui_prefs(...)`；4 份拷贝收敛为 1
- `server/src/supervisor.rs`：迁入 main.rs 屏幕轮询（顺带修 M7：锁内只 clone、锁外解析）、tmux 恢复；main.rs 与 headless 分支改调用
- hook 注入迁 `app/src/bootstrap.rs`（main.rs 降到 ~120 行）
- 验证：基线命令；手工验证重启恢复 pane 树/远端列表/主题

### 阶段 4：性能 + 正确性快修（1 天，风险低）
- `read_frame` 改 BufReader + 客户端两处包装（H4）
- `RemoteHost` 长连接/连接池，per-op helpers 改收 `&mut Connection`（H3）
- server spawn/kill 路径包 `spawn_blocking`（H5）
- mirror task 补 `cx.notify()`（M9，潜伏 bug）
- 关键 enum 加 `#[serde(other)]`；`PROTOCOL_VERSION`/`features::*` 常量（H6/M12）
- `BootstrapProgress` 换 `Mutex<Option<…>>`（M14）
- sound.rs 单线程 + channel（M10 部分）
- 侧栏 agent/project 行渲染提取公共函数（H9，为阶段 5 铺路）
- host.rs `run_loop` 提取 `wait_retry_or_30s`/`upload_then_start`（M4）
- 验证：基线命令 + ui-smoke.sh；远程终端输出延迟人工感知对比

### 阶段 5：app.rs 机械拆解（3–5 天）
按第二部分 §5 的 PR1–PR7 序列执行。每个 PR 纯移动或单一重构，绝不混合。
同步把 `open_remote_agent` 的镜像循环与 5ms 输入合批下沉 `client::term_stream.rs`（阶段 4 若已做则此处只是接线）。
验证：每拆一个模块跑一次 `cargo check -p lmux-app`；全部完成后基线命令 + ui-smoke.sh 全量 + 人工拖拽分隔线。

### 阶段 6：收尾（按需排期）
- client 类型化错误（`VersionSkew/MethodUnsupported`），替换 app.rs:3677/3739 字符串分派（P1-7）
- `Connection::call` 加超时 + 事件帧不丢弃（P2-8.4/L3）
- i18n 二选一：补全 128 处 or 删 i18n 模块止血（P2-8.3/H8）
- 密码存储改 secret provider 或至少单独 0600 文件（P2-8.6）
- `SshAuth ↔ PersistedRemoteAuth` 的 From/Into 收敛（M5）
- 状态刷新删 1s 轮询走事件通道（M6，可与阶段 4 合并）
- hook.rs 拆分 + JS 脚本 `include_str!`（M13）
- 术语表 + `PersistedSession` 改名（M1）、AgentId 前缀解析删除（M2）
- VTerm poison 锁加 error log（L4）、集成测试 spawn guard（L2）、注释考古清理（L5）
- **CI：新增 ci.yml（fmt --check + clippy -D warnings + test + ui-smoke 可选 job）——没有门禁，这次拆完还会长回去**

---

## 第六部分 · 不建议动的部分（防过度重构）

1. **lmux-core 不拆 crate**。model/protocol/detect/hook/auth 共 ~2600 行、7 个模块、边界清晰、测试覆盖好（detect 10 个、pane 9 个、protocol 4 个）。lib.rs 18 行的 re-export 完全正常。
2. **lmux-store 不合并**。236 行看着薄，但它是唯一带版本迁移语义（migrate/STORE_VERSION）和原子写（tmp+rename）的地方，被 server 与 app 双方使用；合并会造成反向依赖。要修的是转换逻辑 4 份拷贝，不是 crate 本身。
3. **lmux-term 不动**。session/vterm/replay 是全 workspace 内聚性最好的 crate，与 GPUI 解耦干净，测试充分。tmux 依赖、`kill_persistent` 语义是产品架构决策（GUI 关闭=detach），保持。
4. **不换 RPC 协议、不引事件总线框架**。newline-JSON + 1MiB 帧 + 订阅设计有 `HelloResult.features` 版本协商位，够用。
5. **不动 GPUI rev、不抽象 UI 框架、不搞 MVU/Elm**。GPUI 的 Entity/Context 模型已经就是状态管理。
6. **不把 pane 树模型挪出 core、不把 TermView 拆 Entity 化**——term_view.rs 1327 行虽大，但它是单一渲染域。
7. **`LmuxApp` 保持单一根 Entity**；palette 是否升子 Entity 留到阶段 5 之后按实际疼痛决定。
8. **`lifecycle` 锁语义保留**（spawn 与 project-delete 互斥防半建状态），只是收进 ServerApi 内部，别改成 Channel/Actor——规模不需要。

---

## 快速修复 Top 10（每项 ≤ 半天，合计约 2 天清完）

1. 删除 `crates/lmux-app/src/ui.rs`
2. 删除死代码清单（H7 全部条目）
3. 清理未用依赖（server 的 thiserror/ulid/base64、term 的 ulid/thiserror、client 的 lmux-server 移 dev-deps）
4. 4 个 `#[allow(dead_code)]` 清零
5. 协议常量集中（PROTOCOL_VERSION + features::* + 测试裸字符串替换）
6. `PersistedApp::from_snapshot` 一处实现替换 4 份复制（收益最大的单点）
7. `handle_conn` 拆 13 个 handle_* 方法（纯机械移动）
8. 客户端 BufReader + `read_frame` 用 `read_until`（term.data 吞吐立刻受益）
9. 合并重复实现（data_dir / default_true / SshAuth↔PersistedRemoteAuth From / host.rs 4 份 30s-wait 块）
10. 侧栏 agent/project 行渲染提取公共函数（为 app.rs 拆分铺路）

---

## 附 · 审查方法与可信度说明

- 三个独立审查 agent 分别覆盖：工作区架构（94 次工具调用）、app.rs 方法级拆解（全文 6632 行通读）、全仓可维护性（102 次工具调用）。
- 关键结论经人工抽查验证：client→server 伪依赖 ✓、LmuxServer 字段全 pub ✓、client 依赖 term 仅为 b64 ✓、中文字符串错误分派 ✓、ui.rs 未被编译 ✓、LmuxApp 59 字段 ✓、blocking 锁 17+ 处 ✓。
- 剔除的误报：审查 A 声称"工作区当前无法编译（app.rs:4964 多余 `}`）"——实测 `cargo check -p lmux-app` 干净通过，系审查时撞上文件编辑中间态。本报告其余行号以当前工作树为准，执行各阶段前建议重新核对行号（代码在活跃变动中）。
