# Muxlane 术语表

本文约定代码、文档和 UI 中同一概念的规范名称。新增命名优先使用“规范名”；“现有别名”只用于兼容既有 API、协议和用户文案。

| 规范名 | 现有别名 | 定义与代码位置 |
| --- | --- | --- |
| **Agent** | `AgentInstance`、`PersistedAgent`、UI“会话”、`SessionMenu`、`agent.status_changed` | 一个可启动、查看和关闭的工作单元。运行时模型是 `muxlane_core::model::AgentInstance`；持久化记录是 `muxlane_store::PersistedAgent`；稳定标识是 `AgentId`。UI 文案可继续称“会话”，但新类型和内部变量使用 `agent`。 |
| **Agent type** | `AgentType`、preset type | Agent 使用的工具类别，例如 Claude、Codex、Pi 或 Shell。规范来源是 `AgentInstance.agent_type`，不得从 `AgentId` 字符串前缀反推。定义在 `crates/muxlane-core/src/model.rs`。 |
| **Agent status** | `AgentStatus`、status event | Agent 的 `Working`、`Blocked`、`Idle`、`Done` 状态。运行时状态在 `AgentInstance.status`；变更事件名为 `agent.status_changed`。`seen` 表示 Done 状态是否已被用户查看。 |
| **Host** | `RemoteHost`、`HostCfg.name`、`Target::Ssh { host }`、machine、endpoint | 运行 muxlane server 和 Agent 的机器。`RemoteHost` 是客户端连接管理器；`Target` 描述连接目标；`MachineInfo.machine_id` 是服务端机器身份；endpoint 是本地可连接的 socket/隧道端点，不是 Host 身份。定义主要位于 `crates/muxlane-client/src/host.rs` 和 `crates/muxlane-core/src/model.rs`。 |
| **Project** | workspace directory、project node | Host 上承载 Agent 的目录及其元数据。模型是 `muxlane_core::model::Project`，由 `ProjectId` 标识；一个 Agent 归属一个 Project。 |
| **Pane** | split leaf、panel | UI 分屏树中的一个叶节点。每个 Pane 持有一个 `TabGroup`；布局模型是 `muxlane_core::PaneNode`，定义在 `crates/muxlane-core/src/pane.rs`。 |
| **Tab** | agent tab、session tab | Pane 内对某个 Agent 的视图入口。`TabGroup.tabs` 保存 `AgentId`，同一 Agent 同时只属于一个 Pane。Tab 不是独立运行时会话。 |
| **Snapshot** | `state.list`、`last_snapshot`、`remote_snaps` | 某个 Host 在一个时刻的完整状态，包括 machine、projects 和 agents。协议模型是 `muxlane_core::model::Snapshot`；本地 UI 缓存为 `last_snapshot`，远端按 Host 缓存在 `remote_snaps`。 |
| **Terminal session** | `PtySession`、tmux session、terminal mirror | Agent 的终端传输与回放资源。`muxlane_term::PtySession` 是本地 PTY；`tmux_session` 是可恢复的 tmux 名称；remote mirror 是终端字节流。它们不是 UI 中 Agent/“会话”的同义类型。 |

## 持久化命名

`PersistedApp.sessions` 的 JSON 字段名保持为 `sessions`，store version 也不变；本次只把 Rust 元素类型从 `PersistedSession` 改为 `PersistedAgent`。因此现有 `state.json` 可原样读取，不需要数据迁移。
