//! LmuxApp：根组件。侧栏（机器树+通知）+ 贴边终端网格。
use crate::term_view::TermView;
use crate::text_field::TextField;
use gpui::{
    canvas, div, prelude::*, px, relative, rgba, Context, Entity, FocusHandle, Focusable,
    MouseButton, ParentElement, Pixels, Point, Render, SharedString, Styled, Window,
};
use lmux_core::model::{AgentId, Snapshot};
use lmux_core::{PaneId, PaneNode, SplitAxis};
use lmux_server::LmuxServer;
use lmux_term::VTerm;
use std::collections::HashMap;
use std::sync::Arc;

gpui::actions!(lmux, [TogglePalette, ClosePalette, CloseTab, NewShellTab]);

// ── 主题（prototype.html 的亮色板）──
const BG0: u32 = 0xf6f5f1ff;
const BG1: u32 = 0xefede7ff;
const BG2: u32 = 0xe4e2daff;
const LINE: u32 = 0xcfccc0ff;
const FG0: u32 = 0x2a2e38ff;
const FG1: u32 = 0x565c6bff;
const ACCENT: u32 = 0x3d6cd8ff;
const GREEN: u32 = 0x5c9e3aff;
const YELLOW: u32 = 0xc08a2dff;
const RED: u32 = 0xd64557ff;
const CYAN: u32 = 0x2a92b0ff;

fn status_color(status: &lmux_core::model::AgentStatus) -> u32 {
    use lmux_core::model::AgentStatus::*;
    match status {
        Working => YELLOW,
        Blocked => RED,
        Done => CYAN,
        Idle => GREEN,
    }
}

#[derive(Clone)]
struct DragTab {
    agent: AgentId,
    from_pane: PaneId,
}

struct DragGhost {
    label: SharedString,
    offset: Point<Pixels>,
}
impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.offset.x.max(px(0.0)))
            .pt(self.offset.y.max(px(0.0)))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .bg(rgba(0xffffffff))
                    .border_1()
                    .border_color(rgba(ACCENT))
                    .text_size(px(11.))
                    .text_color(rgba(FG0))
                    .child(self.label.clone()),
            )
    }
}

#[derive(Clone)]
struct DividerDrag;

struct DividerDragGhost;

impl Render for DividerDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(1.)).h(px(1.))
    }
}

#[derive(Clone)]
struct SplitDrag {
    split_id: String,
    divider: usize,
    axis: SplitAxis,
    start: Point<Pixels>,
    sizes: Vec<f32>,
}

fn status_marker(status: &lmux_core::model::AgentStatus, frame: usize) -> String {
    if *status == lmux_core::model::AgentStatus::Working {
        const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
        FRAMES[frame % FRAMES.len()].to_string()
    } else {
        "●".into()
    }
}

fn shell_split_launch_cfg(
    agent_id: AgentId,
    cwd: std::path::PathBuf,
    socket: String,
    hook_token: String,
    tmux_session: String,
) -> lmux_term::LaunchCfg {
    lmux_term::LaunchCfg {
        agent: agent_id.clone(),
        agent_type: lmux_core::model::AgentType::Shell,
        cwd,
        env: vec![
            ("LMUX_AGENT_ID".into(), agent_id.clone()),
            ("LMUX_SOCKET".into(), socket),
            ("LMUX_HOOK_TOKEN".into(), hook_token),
        ],
        program_override: None,
        args: vec![],
        cols: 120,
        rows: 32,
        tmux_session: Some(tmux_session),
    }
}

fn resolve_local_project_path(raw_path: &str) -> Option<std::path::PathBuf> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() {
        return None;
    }
    let expanded = if raw_path == "~" {
        std::env::var_os("HOME").map(std::path::PathBuf::from)?
    } else if let Some(rest) = raw_path.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)?
            .join(rest)
    } else {
        std::path::PathBuf::from(raw_path)
    };
    expanded.canonicalize().ok().filter(|path| path.is_dir())
}

fn effective_notification_body(
    status: lmux_core::model::AgentStatus,
    message: Option<String>,
) -> String {
    let normalized = message
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return match status {
            lmux_core::model::AgentStatus::Blocked => "等待输入".into(),
            lmux_core::model::AgentStatus::Done => "任务已完成".into(),
            _ => status.as_str().into(),
        };
    }
    let mut chars = normalized.chars();
    let body: String = chars.by_ref().take(180).collect();
    if chars.next().is_some() {
        format!("{body}…")
    } else {
        body
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectAuthMode {
    SshConfig,
    PublicKey,
    Password,
}

pub struct LmuxApp {
    focus: FocusHandle,
    server: Arc<LmuxServer>,
    /// 递归 pane tree；每个 Leaf 内是 TabGroup（参考 muxel）
    pane_tree: PaneNode,
    active_pane: PaneId,
    maximized_pane: Option<PaneId>,
    terms: HashMap<AgentId, Entity<TermView>>,
    mirror_cancel: HashMap<AgentId, Arc<std::sync::atomic::AtomicBool>>,
    active: Option<AgentId>,
    last_snapshot: Snapshot,
    /// 远程机器（本地快照缓存：host name → snapshot）
    remotes: Vec<Arc<lmux_client::RemoteHost>>,
    remote_snaps: HashMap<String, Snapshot>,
    remote_states: HashMap<String, lmux_client::RemoteState>,
    /// 通知中心（新事件 unshift，上限 50）
    notifications: Vec<Notification>,
    palette_open: bool,
    palette_index: usize,
    presets: Vec<lmux_core::AgentPreset>,
    connect_dialog: bool,
    connect_input: Entity<TextField>,
    connect_auth_mode: ConnectAuthMode,
    connect_focus_index: usize,
    connect_username: Entity<TextField>,
    connect_password: Entity<TextField>,
    connect_key_path: Entity<TextField>,
    project_dialog: bool,
    remote_project_dialog: Option<String>,
    remote_project_input: Entity<TextField>,
    project_input: Entity<TextField>,
    dialog_error: Option<String>,
    remote_event_tx: tokio::sync::mpsc::Sender<lmux_client::ClientEvent>,
    new_session_project: Option<lmux_core::model::ProjectId>,
    session_menu: Option<SessionMenu>,
    tree_menu: Option<TreeMenu>,
    delete_confirm: Option<DeleteConfirm>,
    delete_error: Option<String>,
    bootstrap_confirm: Option<BootstrapConfirm>,
    bootstrap_error: Option<String>,
    store_path: std::path::PathBuf,
    split_drag: Option<SplitDrag>,
    split_metrics: Arc<std::sync::Mutex<HashMap<String, f32>>>,
    spinner_frame: usize,
    collapsed_machines: std::collections::HashSet<String>,
    collapsed_projects: std::collections::HashSet<String>,
}

#[derive(Clone)]
struct SessionMenu {
    agent: AgentId,
    position: Point<Pixels>,
    remote: bool,
}

#[derive(Clone)]
enum DeleteTarget {
    LocalProject {
        project: String,
        label: String,
    },
    RemoteProject {
        host: String,
        project: String,
        label: String,
    },
    RemoteMachine {
        host: String,
    },
}

#[derive(Clone)]
struct TreeMenu {
    target: DeleteTarget,
    position: Point<Pixels>,
}

fn dismiss_context_menus(
    session_menu: &mut Option<SessionMenu>,
    tree_menu: &mut Option<TreeMenu>,
) -> bool {
    let had_open_menu = session_menu.is_some() || tree_menu.is_some();
    *session_menu = None;
    *tree_menu = None;
    had_open_menu
}

#[derive(Clone)]
struct DeleteConfirm {
    target: DeleteTarget,
    affected_sessions: usize,
}

#[derive(Clone)]
struct BootstrapConfirm {
    host: String,
    install: bool,
    upgrade: bool,
    binary: Option<String>,
}

#[derive(Clone)]
pub struct Notification {
    pub agent: AgentId,
    pub from: lmux_core::model::AgentStatus,
    pub to: lmux_core::model::AgentStatus,
    pub message: Option<String>,
    pub time: String,
    pub unread: bool,
}

impl Focusable for LmuxApp {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LmuxApp {
    pub fn new(
        cx: &mut Context<Self>,
        server: Arc<LmuxServer>,
        connect_to: Vec<String>,
        persisted: lmux_store::PersistedApp,
        store_path: std::path::PathBuf,
    ) -> Self {
        let state = Arc::clone(&server.state);
        // 本地状态事件 → 通知列表
        if let Ok(mut local_rx) = server.state.try_read().map(|s| s.events.subscribe()) {
            cx.spawn(async move |this, cx| loop {
                match local_rx.recv().await {
                    Ok(ev) => {
                        if ev.event == lmux_core::protocol::events::AGENT_STATUS {
                            if let Ok(p) = serde_json::from_value::<
                                lmux_core::protocol::AgentStatusEvent,
                            >(ev.params)
                            {
                                this.update(cx, |this, cx| {
                                    this.push_notification(p.agent, p.from, p.to, p.message);
                                    cx.notify();
                                })
                                .ok();
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            })
            .detach();
        }

        // 每 1s 拉一次快照（P0 轮询；P1 换事件驱动）
        let state_for_poll = state.clone();
        cx.spawn(async move |this, cx| loop {
            let snap = {
                let state = state_for_poll.clone();
                cx.background_spawn(async move { state.read().await.snapshot() })
                    .await
            };
            this.update(cx, |this, cx| {
                this.last_snapshot = snap;
                cx.notify();
            })
            .ok();
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
        })
        .detach();

        // working spinner 动画；仅更新一个 frame，不做业务轮询。
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            if this
                .update(cx, |this, cx| {
                    this.spinner_frame = (this.spinner_frame + 1) % 8;
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        })
        .detach();

        // ── 远程机器接入（socket 直连；SSH 隧道在 tunnel.rs）
        let mut remotes = Vec::new();
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        for saved in &persisted.remote_configs {
            let target = lmux_client::parse_target(&saved.target);
            let name = match &target {
                lmux_client::Target::Socket(path) => {
                    path.rsplit('/').next().unwrap_or(path).to_string()
                }
                lmux_client::Target::Ssh { host, .. } => host.clone(),
            };
            let auth = match &saved.auth {
                lmux_store::PersistedRemoteAuth::SshConfig => lmux_client::SshAuth::SshConfig,
                lmux_store::PersistedRemoteAuth::PublicKey {
                    username,
                    identity_file,
                } => lmux_client::SshAuth::PublicKey {
                    username: username.clone(),
                    identity_file: identity_file.clone(),
                },
            };
            let remote = lmux_client::RemoteHost::new(
                lmux_client::HostCfg {
                    name,
                    target,
                    auth,
                    retry_base_ms: 500,
                },
                tx.clone(),
            );
            server.rt_spawn(Arc::clone(&remote).run_loop());
            remotes.push(remote);
        }
        for target in &connect_to {
            let parsed = lmux_client::parse_target(target);
            let name = match &parsed {
                lmux_client::Target::Socket(path) => {
                    path.rsplit('/').next().unwrap_or(path).to_string()
                }
                lmux_client::Target::Ssh { host, .. } => host.clone(),
            };
            let cfg = lmux_client::HostCfg {
                name,
                target: parsed,
                auth: lmux_client::SshAuth::SshConfig,
                retry_base_ms: 500,
            };
            if remotes
                .iter()
                .any(|remote: &Arc<lmux_client::RemoteHost>| remote.cfg.name == cfg.name)
            {
                continue;
            }
            let host = lmux_client::RemoteHost::new(cfg, tx.clone());
            server.rt_spawn(std::clone::Clone::clone(&host).run_loop());
            remotes.push(host);
        }

        // 远程事件泵：StateChanged(Online) 更新快照缓存并触发 UI 刷新
        {
            cx.spawn(async move |this, cx| {
                let mut rx = rx;
                loop {
                    let Some(ev) = rx.recv().await else { break };
                    this.update(cx, |this, cx| {
                        match ev {
                            lmux_client::ClientEvent::StateChanged { host, state } => {
                                if let lmux_client::RemoteState::Online(snap) = &state {
                                    this.remote_snaps.insert(host.clone(), snap.clone());
                                }
                                this.remote_states.insert(host, state);
                            }
                            lmux_client::ClientEvent::StatusChanged {
                                host,
                                agent,
                                from,
                                to,
                                message,
                            } => {
                                if let Some(snap) = this.remote_snaps.get_mut(&host) {
                                    if let Some(a) = snap.agents.iter_mut().find(|a| a.id == agent)
                                    {
                                        a.status = to;
                                    }
                                }
                                this.push_notification(agent, from, to, message);
                            }
                            _ => {}
                        }
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        }

        let initial_snapshot = state.try_read().map(|s| s.snapshot()).unwrap_or_default();
        let first_agent = initial_snapshot.agents.first().map(|a| a.id.clone());
        let valid: std::collections::HashSet<AgentId> = initial_snapshot
            .agents
            .iter()
            .map(|a| a.id.clone())
            .collect();
        let mut restored_tree = persisted.pane_tree.clone();
        restored_tree.retain_agents(&valid);
        let restored_active = persisted
            .active_pane
            .filter(|id| restored_tree.group(id).is_some())
            .unwrap_or_else(|| restored_tree.first_pane_id());
        let connect_input = cx.new(|cx| TextField::new("nuc 或 192.168.1.20", cx));
        let connect_username = cx.new(|cx| TextField::new("用户名（可选）", cx));
        let connect_password = cx.new(|cx| TextField::new_secure("密码", cx));
        let connect_key_path = cx.new(|cx| TextField::new("私钥路径（可选）", cx));
        let project_input = cx.new(|cx| TextField::new("~/projects/my-project", cx));
        let remote_project_input = cx.new(|cx| TextField::new("~/projects/remote-project", cx));
        let mut app = LmuxApp {
            focus: cx.focus_handle(),
            server,
            pane_tree: restored_tree,
            active_pane: restored_active,
            maximized_pane: persisted
                .maximized_pane
                .filter(|id| persisted.pane_tree.group(id).is_some()),
            terms: HashMap::new(),
            mirror_cancel: HashMap::new(),
            active: None,
            last_snapshot: initial_snapshot,
            remotes,
            remote_snaps: HashMap::new(),
            remote_states: HashMap::new(),
            notifications: Vec::new(),
            palette_open: false,
            palette_index: 0,
            presets: lmux_core::builtin_presets(lmux_term::default_shell_program()),
            connect_dialog: false,
            connect_input,
            connect_auth_mode: ConnectAuthMode::SshConfig,
            connect_focus_index: 0,
            connect_username,
            connect_password,
            connect_key_path,
            project_dialog: false,
            remote_project_dialog: None,
            remote_project_input,
            project_input,
            dialog_error: None,
            remote_event_tx: tx.clone(),
            new_session_project: None,
            session_menu: None,
            tree_menu: None,
            delete_confirm: None,
            delete_error: None,
            bootstrap_confirm: None,
            bootstrap_error: None,
            store_path,
            split_drag: None,
            split_metrics: Arc::new(std::sync::Mutex::new(HashMap::new())),
            spinner_frame: 0,
            collapsed_machines: std::collections::HashSet::new(),
            collapsed_projects: std::collections::HashSet::new(),
        };
        app.active = app
            .pane_tree
            .group(&app.active_pane)
            .and_then(|group| group.active.clone());
        let opened: Vec<_> = app
            .last_snapshot
            .agents
            .iter()
            .filter(|agent| app.pane_tree.pane_for_agent(&agent.id).is_some())
            .map(|agent| agent.id.clone())
            .collect();
        for agent in opened {
            let session = app.server.sessions.blocking_lock().get(&agent).cloned();
            if let Some(session) = session {
                let term = cx.new(|cx| TermView::new_local(agent.clone(), session, cx));
                app.terms.insert(agent, term);
            }
        }
        // 仅 UI 自动化使用；真实交互仍由用户点击 agent 打开 tab。
        if std::env::var("LMUX_TEST_AUTO_OPEN").as_deref() == Ok("1") {
            if let Some(id) = first_agent {
                app.open_agent(&id, cx);
            }
        }
        app
    }

    fn persist(&self) {
        let mut projects = self.last_snapshot.projects.clone();
        for p in &mut projects {
            p.agents.clear();
        }
        let remote_configs = self
            .remotes
            .iter()
            .filter_map(|host| {
                let target = match &host.cfg.target {
                    lmux_client::Target::Socket(path) => path.clone(),
                    lmux_client::Target::Ssh { host, socket } if socket.is_empty() => host.clone(),
                    lmux_client::Target::Ssh { host, socket } => format!("{host}:{socket}"),
                };
                let auth = match &host.cfg.auth {
                    lmux_client::SshAuth::SshConfig => lmux_store::PersistedRemoteAuth::SshConfig,
                    lmux_client::SshAuth::PublicKey {
                        username,
                        identity_file,
                    } => lmux_store::PersistedRemoteAuth::PublicKey {
                        username: username.clone(),
                        identity_file: identity_file.clone(),
                    },
                    // 密码永不持久化；重启后用户需要重新添加该连接。
                    lmux_client::SshAuth::Password { .. } => return None,
                };
                Some(lmux_store::PersistedRemote { target, auth })
            })
            .collect();
        let app = lmux_store::PersistedApp {
            version: lmux_store::STORE_VERSION,
            initialized: true,
            projects,
            remotes: vec![],
            remote_configs,
            sessions: self
                .last_snapshot
                .agents
                .iter()
                .filter_map(|a| {
                    Some(lmux_store::PersistedSession {
                        agent_id: a.id.clone(),
                        project_id: a.project.clone(),
                        agent_type: a.agent_type,
                        title: a.title.clone(),
                        tmux_session: a.tmux_session.clone()?,
                    })
                })
                .collect(),
            pane_tree: self.pane_tree.clone(),
            active_pane: Some(self.active_pane.clone()),
            maximized_pane: self.maximized_pane.clone(),
            window: None,
        };
        if let Err(e) = lmux_store::save(&self.store_path, &app) {
            tracing::warn!(error = %e, "persist state failed");
        }
    }

    fn push_notification(
        &mut self,
        agent: AgentId,
        from: lmux_core::model::AgentStatus,
        to: lmux_core::model::AgentStatus,
        message: Option<String>,
    ) {
        let body = effective_notification_body(to, message);
        if from == to {
            if let Some(existing) = self
                .notifications
                .iter_mut()
                .find(|item| item.agent == agent && item.to == to)
            {
                existing.message = Some(body);
                existing.unread = true;
            }
            return;
        }
        // blocked / done 才进通知中心（working/idle 刷屏没意义）
        if !matches!(
            to,
            lmux_core::model::AgentStatus::Blocked | lmux_core::model::AgentStatus::Done
        ) {
            return;
        }
        self.notifications.insert(
            0,
            Notification {
                agent,
                from,
                to,
                message: Some(body.clone()),
                time: now_hhmm(),
                unread: true,
            },
        );
        if self.notifications.len() > 50 {
            self.notifications.truncate(50);
        }
        let summary = match to {
            lmux_core::model::AgentStatus::Blocked => "lmux · 等待输入",
            lmux_core::model::AgentStatus::Done => "lmux · 任务完成",
            _ => "lmux",
        };
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .summary(summary)
                .body(&body)
                .appname("lmux")
                .show();
        });
    }

    fn activate_tab(&mut self, pane: &PaneId, agent: &AgentId) {
        if let Some(group) = self.pane_tree.group_mut(pane) {
            group.open(agent.clone());
        }
        self.active_pane = pane.clone();
        self.active = Some(agent.clone());
        self.maximized_pane = None;
        // 本地 Done 会话查看后回到 Idle（herdr seen 语义）。
        if self.last_snapshot.agent(agent).is_some() {
            let changed = !self
                .server
                .state
                .blocking_write()
                .mark_seen(agent)
                .is_empty();
            if changed {
                self.server.dirty.bump();
            }
        }
        if let Some(n) = self.notifications.iter_mut().find(|n| &n.agent == agent) {
            n.unread = false;
        }
        self.persist();
    }

    fn open_agent(&mut self, agent: &AgentId, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.activate_tab(&pane, agent);
            cx.notify();
            return;
        }
        if !self.terms.contains_key(agent) {
            let sess = {
                let map = futures_lite_block(&self.server.sessions);
                map.get(agent).cloned()
            };
            if let Some(sess) = sess {
                let term = cx.new(|cx| TermView::new_local(agent.clone(), sess, cx));
                self.terms.insert(agent.clone(), term);
            } else {
                return;
            }
        }
        let pane = self.active_pane.clone();
        self.pane_tree.open_tab(&pane, agent.clone());
        self.activate_tab(&pane, agent);
        cx.notify();
    }

    fn open_remote_agent(&mut self, agent: &AgentId, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.activate_tab(&pane, agent);
            cx.notify();
            return;
        }
        if !self.terms.contains_key(agent) {
            let vterm = VTerm::new(120, 32);
            vterm.feed("\u{1b}[2m正在 attach 远程终端…\u{1b}[0m\r\n".as_bytes());
            let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
            let term =
                cx.new(|cx| TermView::new_remote(agent.clone(), vterm.clone(), command_tx, cx));
            self.terms.insert(agent.clone(), term);
            // agent → 所属 RemoteHost，禁止全局 endpoint 串台。
            let host_name = self
                .remote_snaps
                .iter()
                .find(|(_, snap)| snap.agents.iter().any(|a| &a.id == agent))
                .map(|(host, _)| host.clone());
            let remote = host_name
                .and_then(|name| self.remotes.iter().find(|h| h.cfg.name == name).cloned());
            if let Some(remote) = remote {
                let command_remote = Arc::clone(&remote);
                let command_agent = agent.clone();
                let command_vterm = vterm.clone();
                self.server.rt_spawn(async move {
                    while let Some(first) = command_rx.recv().await {
                        let mut input = Vec::new();
                        let mut resize = None;
                        let mut collect =
                            |command: crate::term_view::RemoteTermCommand| match command {
                                crate::term_view::RemoteTermCommand::Input(bytes) => {
                                    input.extend(bytes)
                                }
                                crate::term_view::RemoteTermCommand::Resize(cols, rows) => {
                                    resize = Some((cols, rows));
                                }
                            };
                        collect(first);
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        while let Ok(command) = command_rx.try_recv() {
                            collect(command);
                        }
                        let Some(endpoint) = command_remote.endpoint_now() else {
                            command_vterm.feed(
                                b"\r\n\x1b[31mremote input unavailable: disconnected\x1b[0m\r\n",
                            );
                            continue;
                        };
                        if !input.is_empty() {
                            if let Err(error) =
                                lmux_client::send_term_input(&endpoint, &command_agent, &input)
                                    .await
                            {
                                command_vterm.feed(
                                    format!("\r\n\x1b[31mremote input failed: {error}\x1b[0m\r\n")
                                        .as_bytes(),
                                );
                            }
                        }
                        if let Some((cols, rows)) = resize {
                            let _ = lmux_client::resize_term(&endpoint, &command_agent, cols, rows)
                                .await;
                        }
                    }
                });

                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.mirror_cancel
                    .insert(agent.clone(), Arc::clone(&cancelled));
                let agent2 = agent.clone();
                let vterm2 = vterm.clone();
                self.server.rt_spawn(async move {
                    let mut backoff = 250u64;
                    loop {
                        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        let Some(sock) = remote.endpoint_now() else {
                            tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                            backoff = (backoff * 2).min(5_000);
                            continue;
                        };
                        let vterm3 = vterm2.clone();
                        let result =
                            lmux_client::stream_term(&sock, &agent2, move |update| match update {
                                lmux_client::TermUpdate::Resync(bytes) => {
                                    vterm3.feed(b"\x1bc");
                                    vterm3.feed(&bytes);
                                }
                                lmux_client::TermUpdate::Data(bytes) => vterm3.feed(&bytes),
                            })
                            .await;
                        if result.is_ok() || cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        vterm2.feed("\u{1b}[31m镜像流断开，正在重连…\u{1b}[0m\r\n".as_bytes());
                        tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                        backoff = (backoff * 2).min(5_000);
                    }
                });
            }
        }
        let pane = self.active_pane.clone();
        self.pane_tree.open_tab(&pane, agent.clone());
        self.activate_tab(&pane, agent);
        cx.notify();
    }

    fn focus_agent(&self, agent: &AgentId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(term) = self.terms.get(agent) {
            term.focus_handle(cx).focus(window, cx);
        }
    }

    fn close_tab(&mut self, pane: &PaneId, agent: &AgentId, cx: &mut Context<Self>) {
        if !self.pane_tree.close_tab(pane, agent) {
            return;
        }
        if self.pane_tree.pane_for_agent(agent).is_none() {
            self.terms.remove(agent);
            if let Some(cancelled) = self.mirror_cancel.remove(agent) {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        if &self.active_pane == pane {
            self.active = self.pane_tree.group(pane).and_then(|g| g.active.clone());
        }
        self.persist();
        cx.notify();
    }

    fn move_dragged_tab(
        &mut self,
        drag: &DragTab,
        target_pane: &PaneId,
        target_index: usize,
        cx: &mut Context<Self>,
    ) {
        if self
            .pane_tree
            .move_tab(&drag.from_pane, target_pane, &drag.agent, target_index)
        {
            self.active_pane = target_pane.clone();
            self.active = Some(drag.agent.clone());
            self.persist();
            cx.notify();
        }
    }

    fn spawn_preset(
        &mut self,
        preset: &lmux_core::AgentPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lifecycle = Arc::clone(&self.server.lifecycle);
        let _lifecycle = lifecycle.blocking_lock();
        let project = self
            .new_session_project
            .as_ref()
            .and_then(|id| self.last_snapshot.project(id))
            .cloned()
            .or_else(|| {
                self.active
                    .as_ref()
                    .and_then(|id| self.last_snapshot.agent(id))
                    .and_then(|a| self.last_snapshot.project(&a.project))
                    .cloned()
            })
            .or_else(|| self.last_snapshot.projects.first().cloned());
        let Some(project) = project else { return };
        let agent_id = lmux_core::model::new_id(preset.agent_type.as_str());
        let tmux_name = format!("lmux-{}", agent_id);
        let mut cfg = lmux_term::LaunchCfg {
            agent: agent_id.clone(),
            agent_type: preset.agent_type,
            cwd: project.path.clone(),
            env: preset
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            program_override: if preset.agent_type == lmux_core::model::AgentType::Shell {
                None
            } else {
                Some(preset.program.clone())
            },
            args: preset.args.clone(),
            cols: 120,
            rows: 32,
            tmux_session: Some(tmux_name.clone()),
        };
        cfg.env.push(("LMUX_AGENT_ID".into(), agent_id.clone()));
        cfg.env.push((
            "LMUX_SOCKET".into(),
            self.server.socket_path.display().to_string(),
        ));
        cfg.env
            .push(("LMUX_HOOK_TOKEN".into(), self.server.hook_token(&agent_id)));
        let Ok(session) = lmux_term::PtySession::spawn(cfg) else {
            return;
        };
        self.server
            .sessions
            .blocking_lock()
            .insert(agent_id.clone(), Arc::clone(&session));
        let instance = lmux_core::model::AgentInstance {
            id: agent_id.clone(),
            project: project.id.clone(),
            agent_type: preset.agent_type,
            title: preset.label.clone(),
            status: lmux_core::model::AgentStatus::Idle,
            status_since: lmux_core::model::now_secs(),
            seen: true,
            tmux_session: Some(tmux_name),
        };
        self.server
            .state
            .blocking_write()
            .add_agent(project, instance);
        self.server.dirty.bump();
        let term = cx.new(|cx| TermView::new_local(agent_id.clone(), session, cx));
        self.terms.insert(agent_id.clone(), term);
        let pane = self.active_pane.clone();
        self.pane_tree.open_tab(&pane, agent_id.clone());
        self.activate_tab(&pane, &agent_id);
        self.focus_agent(&agent_id, window, cx);
        self.palette_open = false;
        self.new_session_project = None;
        self.persist();
        cx.notify();
    }

    fn spawn_shell_for_pane(&mut self, pane: &PaneId, cx: &mut Context<Self>) -> Option<AgentId> {
        let lifecycle = Arc::clone(&self.server.lifecycle);
        let _lifecycle = lifecycle.blocking_lock();
        let project = self
            .pane_tree
            .group(pane)
            .and_then(|group| group.active.clone().or_else(|| group.tabs.first().cloned()))
            .and_then(|id| self.last_snapshot.agent(&id))
            .and_then(|agent| self.last_snapshot.project(&agent.project))
            .cloned()
            .or_else(|| self.last_snapshot.projects.first().cloned())?;
        let project_id = project.id.clone();
        let agent_id = lmux_core::model::new_id("shell");
        let tmux_name = format!("lmux-{}", agent_id);
        let cfg = shell_split_launch_cfg(
            agent_id.clone(),
            project.path.clone(),
            self.server.socket_path.display().to_string(),
            self.server.hook_token(&agent_id),
            tmux_name.clone(),
        );
        let session = lmux_term::PtySession::spawn(cfg).ok()?;
        self.server
            .sessions
            .blocking_lock()
            .insert(agent_id.clone(), Arc::clone(&session));
        let shell_title = lmux_term::default_shell_program()
            .rsplit('/')
            .next()
            .unwrap_or("shell")
            .to_string();
        self.server.state.blocking_write().add_agent(
            project,
            lmux_core::model::AgentInstance {
                id: agent_id.clone(),
                project: project_id,
                agent_type: lmux_core::model::AgentType::Shell,
                title: shell_title,
                status: lmux_core::model::AgentStatus::Idle,
                status_since: lmux_core::model::now_secs(),
                seen: true,
                tmux_session: Some(tmux_name),
            },
        );
        self.server.dirty.bump();
        let term = cx.new(|cx| TermView::new_local(agent_id.clone(), session, cx));
        self.terms.insert(agent_id.clone(), term);
        Some(agent_id)
    }

    fn new_shell_tab(&mut self, pane: &PaneId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent_id) = self.spawn_shell_for_pane(pane, cx) else {
            return;
        };
        self.pane_tree.open_tab(pane, agent_id.clone());
        self.activate_tab(pane, &agent_id);
        self.focus_agent(&agent_id, window, cx);
        self.persist();
        cx.notify();
    }

    /// 显式分屏：新 pane 始终启动普通 Shell，不复制当前 agent 类型。
    fn split_pane(
        &mut self,
        pane: &PaneId,
        axis: SplitAxis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent_id) = self.spawn_shell_for_pane(pane, cx) else {
            return;
        };
        if let Some(new_pane) = self.pane_tree.split(pane, axis, agent_id.clone()) {
            self.active_pane = new_pane;
            self.active = Some(agent_id.clone());
            self.focus_agent(&agent_id, window, cx);
        }
        self.persist();
        cx.notify();
    }

    fn toggle_maximize(&mut self, pane: &PaneId, cx: &mut Context<Self>) {
        self.maximized_pane = if self.maximized_pane.as_ref() == Some(pane) {
            None
        } else {
            Some(pane.clone())
        };
        self.persist();
        cx.notify();
    }

    fn add_remote_target(&mut self, target: String, cx: &mut Context<Self>) {
        let target = target.trim().to_string();
        if target.is_empty() {
            self.dialog_error = Some("请输入 SSH host 或别名".into());
            cx.notify();
            return;
        }
        let parsed = lmux_client::parse_target(&target);
        let name = match &parsed {
            lmux_client::Target::Socket(path) => {
                path.rsplit('/').next().unwrap_or(path).to_string()
            }
            lmux_client::Target::Ssh { host, .. } => host.clone(),
        };
        if let Some(index) = self.remotes.iter().position(|host| host.cfg.name == name) {
            self.remotes[index].stop();
            self.server
                .runtime
                .block_on(lmux_client::release_remote_tunnel(&name));
            self.remotes.remove(index);
            self.remote_snaps.remove(&name);
            self.remote_states.remove(&name);
        }
        let username = self.connect_username.read(cx).text();
        let auth = match self.connect_auth_mode {
            ConnectAuthMode::SshConfig => lmux_client::SshAuth::SshConfig,
            ConnectAuthMode::PublicKey => lmux_client::SshAuth::PublicKey {
                username: (!username.trim().is_empty()).then(|| username.trim().to_string()),
                identity_file: {
                    let path = self.connect_key_path.read(cx).text();
                    (!path.trim().is_empty()).then(|| path.trim().to_string())
                },
            },
            ConnectAuthMode::Password => {
                let password = self.connect_password.read(cx).text();
                if username.trim().is_empty() || password.is_empty() {
                    self.dialog_error = Some("密码连接需要用户名和密码".into());
                    cx.notify();
                    return;
                }
                lmux_client::SshAuth::Password {
                    username: username.trim().to_string(),
                    password,
                }
            }
        };
        let host = lmux_client::RemoteHost::new(
            lmux_client::HostCfg {
                name,
                target: parsed,
                auth,
                retry_base_ms: 500,
            },
            self.remote_event_tx.clone(),
        );
        self.server.rt_spawn(Arc::clone(&host).run_loop());
        self.remotes.push(host);
        self.persist();
        self.connect_dialog = false;
        self.dialog_error = None;
        self.connect_input.update(cx, |input, cx| input.reset(cx));
        self.connect_username
            .update(cx, |input, cx| input.reset(cx));
        self.connect_password
            .update(cx, |input, cx| input.reset(cx));
        self.connect_key_path
            .update(cx, |input, cx| input.reset(cx));
        cx.notify();
    }

    fn cycle_connect_focus(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fields = match self.connect_auth_mode {
            ConnectAuthMode::SshConfig => vec![self.connect_input.focus_handle(cx)],
            ConnectAuthMode::PublicKey => vec![
                self.connect_input.focus_handle(cx),
                self.connect_username.focus_handle(cx),
                self.connect_key_path.focus_handle(cx),
            ],
            ConnectAuthMode::Password => vec![
                self.connect_input.focus_handle(cx),
                self.connect_username.focus_handle(cx),
                self.connect_password.focus_handle(cx),
            ],
        };
        if backwards {
            self.connect_focus_index = self
                .connect_focus_index
                .checked_sub(1)
                .unwrap_or(fields.len().saturating_sub(1));
        } else {
            self.connect_focus_index = (self.connect_focus_index + 1) % fields.len().max(1);
        }
        if let Some(focus) = fields.get(self.connect_focus_index) {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    fn handle_connect_key(
        &mut self,
        ks: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ks.key.as_str() {
            "escape" => {
                self.connect_dialog = false;
                self.dialog_error = None;
            }
            "tab" => {
                self.cycle_connect_focus(ks.modifiers.shift, window, cx);
                return;
            }
            "enter" => {
                let target = self.connect_input.read(cx).text();
                self.add_remote_target(target, cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn add_local_project(&mut self, raw_path: String, cx: &mut Context<Self>) {
        let raw_path = raw_path.trim();
        if raw_path.is_empty() {
            self.dialog_error = Some("请输入本地项目目录".into());
            cx.notify();
            return;
        }
        let Some(path) = resolve_local_project_path(raw_path) else {
            self.dialog_error = Some("目录不存在或不是文件夹".into());
            cx.notify();
            return;
        };
        if self
            .last_snapshot
            .projects
            .iter()
            .any(|project| project.path == path)
        {
            self.dialog_error = Some("这个项目已经存在".into());
            cx.notify();
            return;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let project = lmux_core::model::Project {
            id: lmux_core::model::new_id("project"),
            name,
            path,
            branch: None,
            agents: vec![],
        };
        if self.server.state.blocking_write().add_project(project) {
            self.last_snapshot = self.server.state.blocking_read().snapshot();
            self.server.dirty.bump();
            self.project_dialog = false;
            self.dialog_error = None;
            self.project_input.update(cx, |input, cx| input.reset(cx));
            self.persist();
        }
        cx.notify();
    }

    fn handle_project_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        match ks.key.as_str() {
            "escape" => {
                self.project_dialog = false;
                self.dialog_error = None;
            }
            "enter" => {
                let path = self.project_input.read(cx).text();
                self.add_local_project(path, cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn delete_session(&mut self, agent: &AgentId, remote: bool, cx: &mut Context<Self>) {
        if remote {
            let host_name = self
                .remote_snaps
                .iter()
                .find(|(_, snap)| snap.agents.iter().any(|a| &a.id == agent))
                .map(|(host, _)| host.clone());
            if let Some(host_name) = host_name {
                if let Some(host) = self
                    .remotes
                    .iter()
                    .find(|h| h.cfg.name == host_name)
                    .cloned()
                {
                    if let Some(sock) = host.endpoint_now() {
                        let id = agent.clone();
                        self.server.rt_spawn(async move {
                            let _ = lmux_client::delete_agent(&sock, &id).await;
                        });
                    }
                }
            }
        } else {
            let session = self.server.sessions.blocking_lock().remove(agent);
            if let Some(sess) = session {
                sess.kill_persistent();
            }
            self.server.state.blocking_write().remove_agent(agent);
            self.server.dirty.bump();
        }
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.pane_tree.close_tab(&pane, agent);
        }
        self.terms.remove(agent);
        if let Some(cancelled) = self.mirror_cancel.remove(agent) {
            cancelled.store(true, std::sync::atomic::Ordering::Release);
        }
        self.notifications.retain(|n| &n.agent != agent);
        if self.active.as_ref() == Some(agent) {
            self.active = self
                .pane_tree
                .group(&self.active_pane)
                .and_then(|g| g.active.clone());
        }
        self.session_menu = None;
        self.persist();
        cx.notify();
    }

    fn cleanup_removed_agents(&mut self, removed: &[AgentId]) {
        let removed: std::collections::HashSet<_> = removed.iter().cloned().collect();
        self.terms.retain(|agent, _| !removed.contains(agent));
        for agent in &removed {
            if let Some(cancelled) = self.mirror_cancel.remove(agent) {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        self.notifications
            .retain(|notification| !removed.contains(&notification.agent));
        let valid: std::collections::HashSet<_> = self
            .last_snapshot
            .agents
            .iter()
            .map(|agent| agent.id.clone())
            .chain(
                self.remote_snaps
                    .values()
                    .flat_map(|snapshot| snapshot.agents.iter().map(|agent| agent.id.clone())),
            )
            .filter(|agent| !removed.contains(agent))
            .collect();
        self.pane_tree.retain_agents(&valid);
        if self
            .active
            .as_ref()
            .is_some_and(|agent| removed.contains(agent))
        {
            self.active = self
                .pane_tree
                .group(&self.active_pane)
                .and_then(|group| group.active.clone());
        }
        if self
            .maximized_pane
            .as_ref()
            .is_some_and(|pane| self.pane_tree.group(pane).is_none())
        {
            self.maximized_pane = None;
        }
    }

    fn begin_delete(&mut self, target: DeleteTarget, cx: &mut Context<Self>) {
        let affected_sessions = match &target {
            DeleteTarget::LocalProject { project, .. } => self
                .last_snapshot
                .agents
                .iter()
                .filter(|agent| &agent.project == project)
                .count(),
            DeleteTarget::RemoteProject { host, project, .. } => self
                .remote_snaps
                .get(host)
                .map(|snapshot| {
                    snapshot
                        .agents
                        .iter()
                        .filter(|agent| &agent.project == project)
                        .count()
                })
                .unwrap_or(0),
            DeleteTarget::RemoteMachine { .. } => 0,
        };
        self.tree_menu = None;
        self.delete_error = None;
        self.delete_confirm = Some(DeleteConfirm {
            target,
            affected_sessions,
        });
        cx.notify();
    }

    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(confirm) = self.delete_confirm.clone() else {
            return;
        };
        match confirm.target {
            DeleteTarget::LocalProject { project, .. } => {
                let lifecycle = Arc::clone(&self.server.lifecycle);
                let _lifecycle = lifecycle.blocking_lock();
                let agents: Vec<_> = self
                    .server
                    .state
                    .blocking_read()
                    .agents
                    .iter()
                    .filter(|agent| agent.project == project)
                    .map(|agent| agent.id.clone())
                    .collect();
                let sessions: HashMap<_, _> = {
                    let mut live = self.server.sessions.blocking_lock();
                    agents
                        .iter()
                        .filter_map(|agent| {
                            live.remove(agent).map(|session| (agent.clone(), session))
                        })
                        .collect()
                };
                let mut destroyed = Vec::new();
                let mut failed = Vec::new();
                for agent in &agents {
                    if sessions
                        .get(agent)
                        .is_none_or(|session| session.kill_persistent())
                    {
                        destroyed.push(agent.clone());
                    } else {
                        failed.push(agent.clone());
                    }
                }
                {
                    let mut subs = self.server.subs.blocking_lock();
                    for agent in &destroyed {
                        subs.mark_agent_exit(agent);
                    }
                }
                {
                    let mut state = self.server.state.blocking_write();
                    for agent in &destroyed {
                        state.remove_agent(agent);
                    }
                    if failed.is_empty() {
                        state.projects.retain(|item| item.id != project);
                    }
                }
                self.last_snapshot = self.server.state.blocking_read().snapshot();
                self.server.dirty.bump();
                self.cleanup_removed_agents(&destroyed);
                if failed.is_empty() {
                    self.delete_confirm = None;
                } else {
                    self.delete_error =
                        Some(format!("{} 个 tmux 会话未能销毁，项目仍保留", failed.len()));
                }
                self.persist();
                cx.notify();
            }
            DeleteTarget::RemoteProject { host, project, .. } => {
                let endpoint = self
                    .remotes
                    .iter()
                    .find(|remote| remote.cfg.name == host)
                    .and_then(|remote| remote.endpoint_now());
                let Some(endpoint) = endpoint else {
                    self.delete_error = Some("远端当前不可连接，未执行删除".into());
                    cx.notify();
                    return;
                };
                let runtime = self.server.runtime.clone();
                let project_for_rpc = project.clone();
                cx.spawn(async move |this, cx| {
                    let result = runtime
                        .spawn(async move {
                            lmux_client::delete_project(&endpoint, &project_for_rpc).await
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| match result {
                        Ok(Ok(result)) => {
                            if let Some(snapshot) = this.remote_snaps.get_mut(&host) {
                                if result.failed_agents.is_empty() {
                                    snapshot.projects.retain(|item| item.id != project);
                                }
                                snapshot
                                    .agents
                                    .retain(|agent| !result.destroyed_agents.contains(&agent.id));
                            }
                            this.cleanup_removed_agents(&result.destroyed_agents);
                            if result.failed_agents.is_empty() {
                                this.delete_confirm = None;
                            } else {
                                this.delete_error = Some(format!(
                                    "{} 个远端 tmux 会话未能销毁，项目仍保留",
                                    result.failed_agents.len()
                                ));
                            }
                            this.persist();
                            cx.notify();
                        }
                        Ok(Err(error)) => {
                            this.delete_error = Some(error.to_string());
                            cx.notify();
                        }
                        Err(error) => {
                            this.delete_error = Some(error.to_string());
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            DeleteTarget::RemoteMachine { host } => {
                let removed_agents: Vec<_> = self
                    .remote_snaps
                    .get(&host)
                    .map(|snapshot| {
                        snapshot
                            .agents
                            .iter()
                            .map(|agent| agent.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(remote) = self
                    .remotes
                    .iter()
                    .find(|remote| remote.cfg.name == host)
                    .cloned()
                {
                    remote.stop();
                }
                let release_host = host.clone();
                self.server.rt_spawn(async move {
                    lmux_client::release_remote_tunnel(&release_host).await;
                });
                self.remotes.retain(|remote| remote.cfg.name != host);
                self.remote_snaps.remove(&host);
                self.remote_states.remove(&host);
                self.cleanup_removed_agents(&removed_agents);
                self.delete_confirm = None;
                self.persist();
                cx.notify();
            }
        }
    }

    fn render_session_menu(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(menu) = self.session_menu.clone() else {
            return div().into_any_element();
        };
        div()
            .absolute()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                if dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu) {
                    cx.notify();
                }
            }))
            .on_any_mouse_down(
                cx.listener(|_this, _event: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                }),
            )
            .left(menu.position.x)
            .top(menu.position.y)
            .w(px(180.))
            .bg(rgba(0xffffffff))
            .border_1()
            .border_color(rgba(LINE))
            .shadow_lg()
            .child(
                div()
                    .id("session-delete")
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(rgba(RED))
                    .hover(|s| s.bg(rgba(0xffeeeeff)))
                    .on_click(cx.listener({
                        let id = menu.agent.clone();
                        let remote = menu.remote;
                        move |this, _ev, _window, cx| this.delete_session(&id, remote, cx)
                    }))
                    .child(if menu.remote {
                        "删除远程会话"
                    } else {
                        "删除会话"
                    }),
            )
            .child(
                div()
                    .id("session-menu-cancel")
                    .px_3()
                    .py_2()
                    .text_size(px(11.))
                    .hover(|s| s.bg(rgba(BG2)))
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.session_menu = None;
                        cx.notify();
                    }))
                    .child("取消"),
            )
            .into_any_element()
    }

    fn render_tree_menu(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(menu) = self.tree_menu.clone() else {
            return div().into_any_element();
        };
        let label = match &menu.target {
            DeleteTarget::LocalProject { .. } => "删除项目…",
            DeleteTarget::RemoteProject { .. } => "删除远程项目…",
            DeleteTarget::RemoteMachine { .. } => "删除远程机器…",
        };
        div()
            .absolute()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                if dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu) {
                    cx.notify();
                }
            }))
            .on_any_mouse_down(
                cx.listener(|_this, _event: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                }),
            )
            .left(menu.position.x)
            .top(menu.position.y)
            .w(px(190.))
            .bg(rgba(0xffffffff))
            .border_1()
            .border_color(rgba(LINE))
            .shadow_lg()
            .child(
                div()
                    .id("tree-delete")
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(rgba(RED))
                    .hover(|style| style.bg(rgba(0xffeeeeff)))
                    .on_click(cx.listener({
                        let target = menu.target.clone();
                        move |this, _event, _window, cx| this.begin_delete(target.clone(), cx)
                    }))
                    .child(label),
            )
            .child(
                div()
                    .id("tree-menu-cancel")
                    .px_3()
                    .py_2()
                    .text_size(px(11.))
                    .hover(|style| style.bg(rgba(BG2)))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.tree_menu = None;
                        cx.notify();
                    }))
                    .child("取消"),
            )
            .into_any_element()
    }

    fn render_delete_confirm(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(confirm) = self.delete_confirm.clone() else {
            return div().into_any_element();
        };
        let (title, label, destructive_copy) = match &confirm.target {
            DeleteTarget::LocalProject { label, .. }
            | DeleteTarget::RemoteProject { label, .. } => (
                "删除项目",
                label.clone(),
                format!(
                    "将结束 {} 个 lmux tmux 会话。项目文件和用户默认 tmux 不会删除。",
                    confirm.affected_sessions
                ),
            ),
            DeleteTarget::RemoteMachine { host } => (
                "删除远程机器连接",
                host.clone(),
                "只删除本地连接、镜像和 tunnel；目标机器上的项目、session 与 tmux 全部保留。"
                    .into(),
            ),
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000044))
            .child(
                div()
                    .w(px(460.))
                    .bg(rgba(0xffffffff))
                    .border_1()
                    .border_color(rgba(LINE))
                    .shadow_lg()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(LINE))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(12.))
                            .child(format!("{} · {}", label, destructive_copy)),
                    )
                    .when_some(self.delete_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .px_4()
                                .pt_2()
                                .text_size(px(11.))
                                .text_color(rgba(RED))
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .child(
                                div()
                                    .id("delete-confirm-cancel")
                                    .px_3()
                                    .py_1()
                                    .hover(|style| style.bg(rgba(BG2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.delete_confirm = None;
                                        this.delete_error = None;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("delete-confirm-submit")
                                    .px_3()
                                    .py_1()
                                    .bg(rgba(RED))
                                    .text_color(rgba(0xffffffff))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.confirm_delete(cx)
                                    }))
                                    .child("确认删除"),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn confirm_bootstrap(&mut self, cx: &mut Context<Self>) {
        let Some(confirm) = self.bootstrap_confirm.clone() else {
            return;
        };
        let Some(remote) = self
            .remotes
            .iter()
            .find(|remote| remote.cfg.name == confirm.host)
            .cloned()
        else {
            self.bootstrap_error = Some("远程机器已不存在".into());
            cx.notify();
            return;
        };
        let runtime = self.server.runtime.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move {
                    if confirm.upgrade {
                        remote.upgrade_and_retry().await
                    } else if confirm.install {
                        remote.install_and_start().await
                    } else {
                        remote
                            .start_and_retry(confirm.binary.as_deref().unwrap_or("lmux"))
                            .await
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(())) => {
                    this.bootstrap_confirm = None;
                    this.bootstrap_error = None;
                    cx.notify();
                }
                Ok(Err(error)) => {
                    this.bootstrap_error = Some(error.to_string());
                    cx.notify();
                }
                Err(error) => {
                    this.bootstrap_error = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn render_bootstrap_confirm(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(confirm) = self.bootstrap_confirm.clone() else {
            return div().into_any_element();
        };
        let action = if confirm.upgrade {
            "更新并重启"
        } else if confirm.install {
            "安装并启动"
        } else {
            "启动并重连"
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000044))
            .child(
                div()
                    .w(px(480.))
                    .bg(rgba(0xffffffff))
                    .border_1()
                    .border_color(rgba(LINE))
                    .shadow_lg()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(format!("{}远端 lmux", action)),
                    )
                    .child(div().px_4().text_size(px(12.)).child(format!(
                        "SSH 已连接到 {}。将使用当前认证方式{} headless 进程。",
                        confirm.host,
                        if confirm.upgrade {
                            "上传新版本并重启"
                        } else if confirm.install {
                            "上传当前 lmux 并启动"
                        } else {
                            "启动"
                        }
                    )))
                    .when_some(self.bootstrap_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .px_4()
                                .pt_2()
                                .text_size(px(11.))
                                .text_color(rgba(RED))
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .child(
                                div()
                                    .id("bootstrap-cancel")
                                    .px_3()
                                    .py_1()
                                    .hover(|style| style.bg(rgba(BG2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.bootstrap_confirm = None;
                                        this.bootstrap_error = None;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("bootstrap-submit")
                                    .px_3()
                                    .py_1()
                                    .bg(rgba(ACCENT))
                                    .text_color(rgba(0xffffffff))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.confirm_bootstrap(cx)
                                    }))
                                    .child(action),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_connect_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let input = self.connect_input.clone();
        let username = self.connect_username.clone();
        let password = self.connect_password.clone();
        let key_path = self.connect_key_path.clone();
        let auth_mode = self.connect_auth_mode;
        let error = self.dialog_error.clone();
        div()
            .absolute()
            .occlude()
            .top(px(180.))
            .left(px(520.))
            .w(px(520.))
            .bg(rgba(0xffffffff))
            .border_1()
            .border_color(rgba(LINE))
            .shadow_lg()
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, window, cx| {
                if matches!(ev.keystroke.key.as_str(), "enter" | "escape" | "tab") {
                    this.handle_connect_key(&ev.keystroke, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgba(LINE))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("连接远程机器"),
            )
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .text_size(px(11.))
                    .text_color(rgba(FG1))
                    .child("输入 SSH Host 或 ~/.ssh/config 别名；socket 自动发现"),
            )
            .child(div().mx_4().mt_3().child(input))
            .child(
                div()
                    .mx_4()
                    .mt_2()
                    .flex()
                    .border_1()
                    .border_color(rgba(LINE))
                    .child(
                        div()
                            .id("auth-config")
                            .flex_1()
                            .px_2()
                            .py_1()
                            .text_size(px(11.))
                            .when(auth_mode == ConnectAuthMode::SshConfig, |item| {
                                item.bg(rgba(ACCENT)).text_color(rgba(0xffffffff))
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.connect_auth_mode = ConnectAuthMode::SshConfig;
                                cx.notify();
                            }))
                            .child("SSH 配置"),
                    )
                    .child(
                        div()
                            .id("auth-key")
                            .flex_1()
                            .px_2()
                            .py_1()
                            .text_size(px(11.))
                            .when(auth_mode == ConnectAuthMode::PublicKey, |item| {
                                item.bg(rgba(ACCENT)).text_color(rgba(0xffffffff))
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.connect_auth_mode = ConnectAuthMode::PublicKey;
                                cx.notify();
                            }))
                            .child("SSH 公钥"),
                    )
                    .child(
                        div()
                            .id("auth-password")
                            .flex_1()
                            .px_2()
                            .py_1()
                            .text_size(px(11.))
                            .when(auth_mode == ConnectAuthMode::Password, |item| {
                                item.bg(rgba(ACCENT)).text_color(rgba(0xffffffff))
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.connect_auth_mode = ConnectAuthMode::Password;
                                cx.notify();
                            }))
                            .child("用户名密码"),
                    ),
            )
            .when(auth_mode == ConnectAuthMode::PublicKey, |dialog| {
                dialog
                    .child(div().mx_4().mt_2().child(username.clone()))
                    .child(div().mx_4().mt_2().child(key_path))
            })
            .when(auth_mode == ConnectAuthMode::Password, |dialog| {
                dialog
                    .child(div().mx_4().mt_2().child(username))
                    .child(div().mx_4().mt_2().child(password))
            })
            .when_some(error, |dialog, error| {
                dialog.child(
                    div()
                        .mx_4()
                        .mt_2()
                        .text_size(px(11.))
                        .text_color(rgba(RED))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .id("connect-cancel")
                            .px_3()
                            .py_1()
                            .hover(|s| s.bg(rgba(BG2)))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.connect_dialog = false;
                                this.dialog_error = None;
                                cx.notify();
                            }))
                            .child("取消"),
                    )
                    .child(
                        div()
                            .id("connect-submit")
                            .px_3()
                            .py_1()
                            .bg(rgba(ACCENT))
                            .text_color(rgba(0xffffffff))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                let target = this.connect_input.read(cx).text();
                                this.add_remote_target(target, cx);
                            }))
                            .child("连接"),
                    ),
            )
            .into_any_element()
    }

    fn spawn_remote_shell(
        &mut self,
        host: String,
        project: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let endpoint = self
            .remotes
            .iter()
            .find(|remote| remote.cfg.name == host)
            .and_then(|remote| remote.endpoint_now());
        let Some(endpoint) = endpoint else {
            return;
        };
        let runtime = self.server.runtime.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = runtime
                .spawn(async move { lmux_client::spawn_shell_agent(&endpoint, &project).await })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Ok(Ok(agent)) = result {
                    let agent_id = agent.id.clone();
                    if let Some(snapshot) = this.remote_snaps.get_mut(&host) {
                        if let Some(project) = snapshot
                            .projects
                            .iter_mut()
                            .find(|project| project.id == agent.project)
                        {
                            project.agents.push(agent_id.clone());
                        }
                        snapshot.agents.push(agent);
                    }
                    this.open_remote_agent(&agent_id, cx);
                    this.focus_agent(&agent_id, window, cx);
                    this.persist();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn submit_remote_project(&mut self, host: String, path: String, cx: &mut Context<Self>) {
        let endpoint = self
            .remotes
            .iter()
            .find(|remote| remote.cfg.name == host)
            .and_then(|remote| remote.endpoint_now());
        let Some(endpoint) = endpoint else {
            self.dialog_error = Some("远端尚未连接".into());
            cx.notify();
            return;
        };
        if path.trim().is_empty() {
            self.dialog_error = Some("请输入远端已有目录".into());
            cx.notify();
            return;
        }
        let runtime = self.server.runtime.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { lmux_client::add_project(&endpoint, path.trim()).await })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(project)) => {
                    if let Some(snapshot) = this.remote_snaps.get_mut(&host) {
                        if !snapshot.projects.iter().any(|item| item.id == project.id) {
                            snapshot.projects.push(project);
                        }
                    }
                    this.remote_project_dialog = None;
                    this.dialog_error = None;
                    this.persist();
                    cx.notify();
                }
                Ok(Err(error)) => {
                    this.dialog_error = Some(error.to_string());
                    cx.notify();
                }
                Err(error) => {
                    this.dialog_error = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn render_remote_project_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(host) = self.remote_project_dialog.clone() else {
            return div().into_any_element();
        };
        let input = self.remote_project_input.clone();
        div()
            .absolute()
            .occlude()
            .top(px(180.))
            .left(px(520.))
            .w(px(520.))
            .bg(rgba(0xffffffff))
            .border_1()
            .border_color(rgba(LINE))
            .shadow_lg()
            .on_key_down(
                cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            this.remote_project_dialog = None;
                            this.dialog_error = None;
                            cx.stop_propagation();
                            cx.notify();
                        }
                        "enter" => {
                            if let Some(host) = this.remote_project_dialog.clone() {
                                let path = this.remote_project_input.read(cx).text();
                                this.submit_remote_project(host, path, cx);
                            }
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                }),
            )
            .child(
                div()
                    .px_4()
                    .py_3()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("在 {host} 添加项目")),
            )
            .child(
                div()
                    .px_4()
                    .text_size(px(11.))
                    .text_color(rgba(FG1))
                    .child("输入远端已存在的目录；不会上传或删除项目文件"),
            )
            .child(div().mx_4().mt_3().child(input))
            .when_some(self.dialog_error.clone(), |dialog, error| {
                dialog.child(
                    div()
                        .mx_4()
                        .mt_2()
                        .text_size(px(11.))
                        .text_color(rgba(RED))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .id("remote-project-cancel")
                            .px_3()
                            .py_1()
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.remote_project_dialog = None;
                                this.dialog_error = None;
                                cx.notify();
                            }))
                            .child("取消"),
                    )
                    .child(
                        div()
                            .id("remote-project-submit")
                            .px_3()
                            .py_1()
                            .bg(rgba(ACCENT))
                            .text_color(rgba(0xffffffff))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                if let Some(host) = this.remote_project_dialog.clone() {
                                    let path = this.remote_project_input.read(cx).text();
                                    this.submit_remote_project(host, path, cx);
                                }
                            }))
                            .child("添加"),
                    ),
            )
            .into_any_element()
    }

    fn render_project_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let input = self.project_input.clone();
        let error = self.dialog_error.clone();
        div()
            .absolute()
            .occlude()
            .top(px(180.))
            .left(px(520.))
            .w(px(520.))
            .bg(rgba(0xffffffff))
            .border_1()
            .border_color(rgba(LINE))
            .shadow_lg()
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                if matches!(ev.keystroke.key.as_str(), "enter" | "escape") {
                    this.handle_project_key(&ev.keystroke, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgba(LINE))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("添加本地项目"),
            )
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .text_size(px(11.))
                    .text_color(rgba(FG1))
                    .child("输入已有项目目录；远程项目由连接机器后自动发现"),
            )
            .child(div().mx_4().mt_3().child(input))
            .when_some(error, |dialog, error| {
                dialog.child(
                    div()
                        .mx_4()
                        .mt_2()
                        .text_size(px(11.))
                        .text_color(rgba(RED))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .id("project-cancel")
                            .px_3()
                            .py_1()
                            .hover(|s| s.bg(rgba(BG2)))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.project_dialog = false;
                                this.dialog_error = None;
                                cx.notify();
                            }))
                            .child("取消"),
                    )
                    .child(
                        div()
                            .id("project-submit")
                            .px_3()
                            .py_1()
                            .bg(rgba(ACCENT))
                            .text_color(rgba(0xffffffff))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                let path = this.project_input.read(cx).text();
                                this.add_local_project(path, cx);
                            }))
                            .child("添加"),
                    ),
            )
            .into_any_element()
    }

    fn handle_palette_key(
        &mut self,
        ks: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let available: Vec<lmux_core::AgentPreset> = self
            .presets
            .iter()
            .filter(|p| p.installed())
            .cloned()
            .collect();
        match ks.key.as_str() {
            "h" => {
                let pane = self.active_pane.clone();
                self.split_pane(&pane, SplitAxis::Horizontal, window, cx);
                self.palette_open = false;
                return;
            }
            "v" => {
                let pane = self.active_pane.clone();
                self.split_pane(&pane, SplitAxis::Vertical, window, cx);
                self.palette_open = false;
                return;
            }
            "x" => {
                let pane = self.active_pane.clone();
                self.close_split_pane(&pane, cx);
                self.palette_open = false;
                return;
            }
            "m" => {
                let pane = self.active_pane.clone();
                self.toggle_maximize(&pane, cx);
                self.palette_open = false;
                return;
            }
            "up" => self.palette_index = self.palette_index.saturating_sub(1),
            "down" => {
                if !available.is_empty() {
                    self.palette_index = (self.palette_index + 1).min(available.len() - 1);
                }
            }
            "enter" => {
                if let Some(preset) = available.get(self.palette_index).cloned() {
                    self.spawn_preset(&preset, window, cx);
                    return;
                }
            }
            "escape" => self.palette_open = false,
            _ => {}
        }
        cx.notify();
    }

    fn render_palette(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut panel = div()
            .absolute()
            .top(px(70.))
            .left(px(480.))
            .w(px(520.))
            .max_h(px(560.))
            .overflow_hidden()
            .bg(rgba(0xffffffff))
            .border_1()
            .border_color(rgba(LINE))
            .shadow_lg()
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgba(LINE))
                    .text_size(px(13.))
                    .text_color(rgba(0x8b90a0ff))
                    .child("输入命令…  Ctrl K / Esc"),
            );
        for (index, preset) in self
            .presets
            .clone()
            .into_iter()
            .filter(|p| p.installed())
            .enumerate()
        {
            let preset_for_click = preset.clone();
            let preset_label = preset.label.clone();
            let preset_program = preset.program.clone();
            panel = panel.child(
                div()
                    .id(gpui::ElementId::Name(
                        format!("preset-{}", preset.id).into(),
                    ))
                    .flex()
                    .items_center()
                    .px_4()
                    .py_2()
                    .text_size(px(12.))
                    .when(index == self.palette_index, |el| el.bg(rgba(BG2)))
                    .hover(|s| s.bg(rgba(BG2)))
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.spawn_preset(&preset_for_click, window, cx)
                    }))
                    .child(format!("＋ 新建 {}", preset_label))
                    .child(
                        div()
                            .ml_auto()
                            .text_size(px(10.))
                            .text_color(rgba(0x8b90a0ff))
                            .child(preset_program),
                    ),
            );
        }
        let pane = self.active_pane.clone();
        for (id, label, axis) in [
            (
                "cmd-split-h",
                "↔  水平分屏  [h]",
                Some(SplitAxis::Horizontal),
            ),
            ("cmd-split-v", "↕  垂直分屏  [v]", Some(SplitAxis::Vertical)),
            ("cmd-max", "⛶  最大化 / 还原  [m]", None),
        ] {
            panel = panel.child(
                div()
                    .id(id)
                    .px_4()
                    .py_2()
                    .text_size(px(12.))
                    .hover(|s| s.bg(rgba(BG2)))
                    .on_click(cx.listener({
                        let pane = pane.clone();
                        move |this, _ev, window, cx| {
                            if let Some(axis) = axis {
                                this.split_pane(&pane, axis, window, cx);
                            } else {
                                this.toggle_maximize(&pane, cx);
                            }
                            this.palette_open = false;
                            cx.notify();
                        }
                    }))
                    .child(label),
            );
        }
        if self.pane_tree.leaf_count() > 1 {
            let pane = self.active_pane.clone();
            panel = panel.child(
                div()
                    .id("cmd-close-pane")
                    .px_4()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(rgba(RED))
                    .hover(|s| s.bg(rgba(0xffeeeeff)))
                    .on_click(cx.listener(move |this, _ev, _window, cx| {
                        this.close_split_pane(&pane, cx);
                        this.palette_open = false;
                    }))
                    .child("×  关闭当前分屏  [x]"),
            );
        }
        panel
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(rgba(LINE))
                    .text_size(px(11.))
                    .text_color(rgba(0x8b90a0ff))
                    .child("连接远程机器：lmux --connect <remote lmux.sock>"),
            )
            .into_any_element()
    }

    fn close_split_pane(&mut self, pane: &PaneId, cx: &mut Context<Self>) {
        if self.pane_tree.leaf_count() <= 1 {
            return;
        }
        if let Some(next) = self.pane_tree.without_pane(pane) {
            self.pane_tree = next;
            if let Ok(mut metrics) = self.split_metrics.lock() {
                metrics.clear();
            }
            self.maximized_pane = None;
            self.active_pane = self.pane_tree.first_pane_id();
            self.active = self
                .pane_tree
                .group(&self.active_pane)
                .and_then(|g| g.active.clone());
            self.persist();
            cx.notify();
        }
    }

    fn start_split_drag(
        &mut self,
        split_id: String,
        divider: usize,
        axis: SplitAxis,
        start: Point<Pixels>,
    ) {
        if let Some((_, sizes)) = self.pane_tree.split_info(&split_id) {
            self.split_drag = Some(SplitDrag {
                split_id,
                divider,
                axis,
                start,
                sizes,
            });
        }
    }

    fn update_split_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.split_drag.clone() else {
            return;
        };
        let extent = self
            .split_metrics
            .lock()
            .ok()
            .and_then(|m| m.get(&drag.split_id).copied())
            .unwrap_or(1.0)
            .max(1.0);
        if drag.divider + 1 >= drag.sizes.len() {
            return;
        }
        let delta_px = if drag.axis == SplitAxis::Horizontal {
            f32::from(position.x - drag.start.x)
        } else {
            f32::from(position.y - drag.start.y)
        };
        let delta = delta_px / extent;
        let pair_total = drag.sizes[drag.divider] + drag.sizes[drag.divider + 1];
        if !pair_total.is_finite() || pair_total <= 0.0 {
            return;
        }
        let min = 0.05_f32.min(pair_total / 2.0);
        let left = (drag.sizes[drag.divider] + delta).clamp(min, pair_total - min);
        let mut next = drag.sizes.clone();
        next[drag.divider] = left;
        next[drag.divider + 1] = pair_total - left;
        if self.pane_tree.update_split_sizes(&drag.split_id, next) {
            cx.notify();
        }
    }

    fn end_split_drag(&mut self) {
        if self.split_drag.take().is_some() {
            self.persist();
        }
    }

    fn render_pane_node(
        &mut self,
        node: PaneNode,
        snap: &Snapshot,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match node {
            PaneNode::Split {
                id,
                axis,
                children,
                sizes,
            } => {
                let metrics = Arc::clone(&self.split_metrics);
                let metric_id = id.clone();
                let mut container = div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .when(axis == SplitAxis::Horizontal, |el| el.flex_row())
                    .when(axis == SplitAxis::Vertical, |el| el.flex_col())
                    .child(
                        canvas(
                            move |bounds, _window, _cx| {
                                let extent = if axis == SplitAxis::Horizontal {
                                    f32::from(bounds.size.width)
                                } else {
                                    f32::from(bounds.size.height)
                                };
                                if let Ok(mut map) = metrics.lock() {
                                    map.insert(metric_id.clone(), extent);
                                }
                            },
                            |_bounds, _state, _window, _cx| {},
                        )
                        .absolute()
                        .size_full(),
                    );
                for (index, child) in children.into_iter().enumerate() {
                    if index > 0 {
                        let split_id = id.clone();
                        container = container.child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("divider-{id}-{index}").into(),
                                ))
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .when(axis == SplitAxis::Horizontal, |el| {
                                    el.w(px(48.))
                                        .h_full()
                                        .ml(px(-23.))
                                        .mr(px(-23.))
                                        .child(div().w(px(2.)).h_full().bg(rgba(LINE)))
                                })
                                .when(axis == SplitAxis::Vertical, |el| {
                                    el.h(px(48.))
                                        .w_full()
                                        .mt(px(-23.))
                                        .mb(px(-23.))
                                        .child(div().h(px(2.)).w_full().bg(rgba(LINE)))
                                })
                                .hover(|s| s.bg(rgba(0xe8e6de88)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this, ev: &gpui::MouseDownEvent, _window, _cx| {
                                            this.start_split_drag(
                                                split_id.clone(),
                                                index - 1,
                                                axis,
                                                ev.position,
                                            );
                                        },
                                    ),
                                )
                                .on_drag(DividerDrag, |_, _offset, _window, cx| {
                                    cx.new(|_| DividerDragGhost)
                                })
                                .on_mouse_move(cx.listener(
                                    |this, ev: &gpui::MouseMoveEvent, _window, cx| {
                                        if this.split_drag.is_some() {
                                            this.update_split_drag(ev.position, cx);
                                        }
                                    },
                                ))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _ev, _window, _cx| this.end_split_drag()),
                                ),
                        );
                    }
                    let rendered = self.render_pane_node(child, snap, cx);
                    let share = sizes
                        .get(index)
                        .copied()
                        .unwrap_or(1.0 / sizes.len().max(1) as f32);
                    container = container.child(
                        div()
                            .flex()
                            .flex_basis(relative(share))
                            .flex_grow_0()
                            .flex_shrink_1()
                            .min_w_0()
                            .min_h_0()
                            .child(rendered),
                    );
                }
                container.into_any_element()
            }
            PaneNode::Leaf { group } => {
                let pane_id = group.id.clone();
                let active_id = group.active.clone();
                let mut tabs = div()
                    .flex()
                    .items_center()
                    .h(px(30.))
                    .bg(rgba(BG2))
                    .border_b_1()
                    .border_color(rgba(LINE));
                for tab_id in group.tabs.clone() {
                    let is_active = active_id.as_ref() == Some(&tab_id);
                    let pane_for_tab = pane_id.clone();
                    let tab = div()
                        .id(gpui::ElementId::Name(format!("tab-{tab_id}").into()))
                        .flex()
                        .items_center()
                        .gap_2()
                        .h_full()
                        .px_3()
                        .text_size(px(11.))
                        .text_color(rgba(if is_active { ACCENT } else { FG1 }))
                        .font_weight(if is_active {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .when(is_active, |el| el.bg(rgba(0xffffffff)))
                        .border_r_1()
                        .border_color(rgba(LINE))
                        .hover(|s| s.bg(rgba(0xf2f1ecff)))
                        .on_click(cx.listener({
                            let id = tab_id.clone();
                            let pane = pane_for_tab.clone();
                            move |this, _ev, window, cx| {
                                this.activate_tab(&pane, &id);
                                this.focus_agent(&id, window, cx);
                                cx.notify();
                            }
                        }))
                        .on_drag(
                            DragTab {
                                agent: tab_id.clone(),
                                from_pane: pane_for_tab.clone(),
                            },
                            {
                                let label: SharedString = agent_label(snap, &tab_id).into();
                                move |_, offset, _, cx| {
                                    let label = label.clone();
                                    cx.new(move |_| DragGhost { label, offset })
                                }
                            },
                        )
                        // drop on tab = insert before it（同组重排/跨 pane）
                        .on_drop::<DragTab>(cx.listener({
                            let pane = pane_id.clone();
                            let slot = group.tabs.iter().position(|a| a == &tab_id).unwrap_or(0);
                            move |this, drag: &DragTab, _window, cx| {
                                this.move_dragged_tab(drag, &pane, slot, cx)
                            }
                        }))
                        .child(session_title(snap, &tab_id))
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("tab-close-{tab_id}").into()))
                                .text_color(rgba(0x8b90a0ff))
                                .on_click(cx.listener({
                                    let id = tab_id.clone();
                                    let pane = pane_id.clone();
                                    move |this, _ev, _window, cx| {
                                        cx.stop_propagation();
                                        this.close_tab(&pane, &id, cx);
                                    }
                                }))
                                .child("×"),
                        );
                    tabs = tabs.child(tab);
                }
                // 显式分屏/最大化 controls：没有隐式 split。
                tabs = tabs.child(
                    div()
                        .ml_auto()
                        .flex()
                        .items_center()
                        .h_full()
                        .text_color(rgba(FG1))
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("new-tab-{pane_id}").into()))
                                .px_2()
                                .text_size(px(14.))
                                .hover(|s| s.bg(rgba(0xf2f1ecff)).text_color(rgba(ACCENT)))
                                .on_click(cx.listener({
                                    let pane = pane_id.clone();
                                    move |this, _ev, window, cx| {
                                        this.new_shell_tab(&pane, window, cx)
                                    }
                                }))
                                .child("＋"),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("split-h-{pane_id}").into()))
                                .px_2()
                                .hover(|s| s.bg(rgba(0xf2f1ecff)))
                                .on_click(cx.listener({
                                    let pane = pane_id.clone();
                                    move |this, _ev, window, cx| {
                                        this.split_pane(&pane, SplitAxis::Horizontal, window, cx)
                                    }
                                }))
                                .child("↔"),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("split-v-{pane_id}").into()))
                                .px_2()
                                .hover(|s| s.bg(rgba(0xf2f1ecff)))
                                .on_click(cx.listener({
                                    let pane = pane_id.clone();
                                    move |this, _ev, window, cx| {
                                        this.split_pane(&pane, SplitAxis::Vertical, window, cx)
                                    }
                                }))
                                .child("↕"),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("maximize-{pane_id}").into()))
                                .px_2()
                                .hover(|s| s.bg(rgba(0xf2f1ecff)))
                                .on_click(cx.listener({
                                    let pane = pane_id.clone();
                                    move |this, _ev, _window, cx| this.toggle_maximize(&pane, cx)
                                }))
                                .child(if self.maximized_pane.as_ref() == Some(&pane_id) {
                                    "❐"
                                } else {
                                    "⛶"
                                }),
                        )
                        .when(self.pane_tree.leaf_count() > 1, |controls| {
                            controls.child(
                                div()
                                    .id(gpui::ElementId::Name(
                                        format!("close-pane-{pane_id}").into(),
                                    ))
                                    .px_2()
                                    .text_color(rgba(RED))
                                    .hover(|s| s.bg(rgba(0xffeeeeff)))
                                    .on_click(cx.listener({
                                        let pane = pane_id.clone();
                                        move |this, _ev, _window, cx| {
                                            this.close_split_pane(&pane, cx)
                                        }
                                    }))
                                    .child("×"),
                            )
                        }),
                );

                let content = active_id
                    .as_ref()
                    .and_then(|id| self.terms.get(id).cloned());
                let tab_count = group.tabs.len();
                let target_pane = pane_id.clone();
                let mut pane = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .on_drop::<DragTab>(cx.listener(move |this, drag: &DragTab, _window, cx| {
                        this.move_dragged_tab(drag, &target_pane, tab_count, cx)
                    }))
                    .drag_over::<DragTab>(|s, _, _, _| s.bg(rgba(0xf5f7ffff)))
                    .child(tabs);
                if let Some(term) = content {
                    pane = pane.child(div().flex_1().min_h_0().child(term));
                } else {
                    pane = pane.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgba(0x8b90a0ff))
                            .child("从左侧选择 agent 打开 tab"),
                    );
                }
                pane.into_any_element()
            }
        }
    }
}
/// P0 临时：短超时阻塞拿会话表（调用方保证持有少量临界区）
fn futures_lite_block(
    m: &Arc<tokio::sync::Mutex<HashMap<AgentId, Arc<lmux_term::PtySession>>>>,
) -> MutexGuardMap<'_> {
    match m.try_lock() {
        Ok(g) => g,
        Err(_) => {
            std::thread::sleep(std::time::Duration::from_millis(2));
            m.blocking_lock()
        }
    }
}
type MutexGuardMap<'a> = tokio::sync::MutexGuard<'a, HashMap<AgentId, Arc<lmux_term::PtySession>>>;

impl Render for LmuxApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = self.last_snapshot.clone();
        let machine_name = snap
            .machine
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "local".into());

        // ── 侧栏：机器树（本地一个 machine 节点 + 项目 + agent）
        let local_machine_key = "local".to_string();
        let local_collapsed = self.collapsed_machines.contains(&local_machine_key);
        let mut tree = div().flex().flex_col().py_1();
        tree = tree.child(
            div()
                .id("machine-local")
                .flex()
                .items_center()
                .gap_1()
                .h(px(38.))
                .px_2()
                .border_b_1()
                .border_color(rgba(LINE))
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgba(FG0))
                .hover(|style| style.bg(rgba(0xe9e7e0ff)))
                .on_click(cx.listener({
                    let key = local_machine_key.clone();
                    move |this, _event, _window, cx| {
                        if !this.collapsed_machines.remove(&key) {
                            this.collapsed_machines.insert(key.clone());
                        }
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .w(px(16.))
                        .text_color(rgba(FG1))
                        .child(if local_collapsed { "▸" } else { "▾" }),
                )
                .child(div().w(px(18.)).text_color(rgba(GREEN)).child("▣"))
                .child(machine_name.clone())
                .child(
                    div()
                        .ml_auto()
                        .px_1()
                        .rounded_sm()
                        .bg(rgba(0xe2eadcff))
                        .text_size(px(9.))
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(rgba(GREEN))
                        .child("本机"),
                )
                .child(
                    div()
                        .id("add-local-project")
                        .w(px(24.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(15.))
                        .text_color(rgba(FG1))
                        .hover(|s| s.bg(rgba(BG2)).text_color(rgba(ACCENT)))
                        .on_click(cx.listener(|this, _ev, window, cx| {
                            cx.stop_propagation();
                            this.project_dialog = true;
                            this.connect_dialog = false;
                            this.dialog_error = None;
                            this.project_input.update(cx, |input, cx| input.reset(cx));
                            this.project_input.focus_handle(cx).focus(window, cx);
                            cx.notify();
                        }))
                        .child("＋"),
                ),
        );
        if !local_collapsed {
            for project in &snap.projects {
                let project_key = format!("local:{}", project.id);
                let project_collapsed = self.collapsed_projects.contains(&project_key);
                let project_branch = project
                    .branch
                    .clone()
                    .filter(|branch| !branch.trim().is_empty());
                let mut pnode = div()
                    .flex()
                    .flex_col()
                    .ml_3()
                    .border_l_1()
                    .border_color(rgba(LINE));
                let project_id_for_add = project.id.clone();
                let local_project_target = DeleteTarget::LocalProject {
                    project: project.id.clone(),
                    label: project.name.clone(),
                };
                pnode = pnode.child(
                    div()
                        .id(gpui::ElementId::Name(
                            format!("project-row-{}", project.id).into(),
                        ))
                        .flex()
                        .items_center()
                        .gap_1()
                        .h(px(32.))
                        .pl_1()
                        .pr_2()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgba(FG0))
                        .hover(|style| style.bg(rgba(0xe9e7e0ff)))
                        .on_click(cx.listener({
                            let key = project_key.clone();
                            move |this, _event, _window, cx| {
                                if !this.collapsed_projects.remove(&key) {
                                    this.collapsed_projects.insert(key.clone());
                                }
                                cx.notify();
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener({
                                let target = local_project_target.clone();
                                move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                    this.session_menu = None;
                                    this.tree_menu = Some(TreeMenu {
                                        target: target.clone(),
                                        position: event.position,
                                    });
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            }),
                        )
                        .child(
                            div()
                                .w(px(16.))
                                .text_color(rgba(FG1))
                                .child(if project_collapsed { "▸" } else { "▾" }),
                        )
                        .child(div().w(px(18.)).text_color(rgba(ACCENT)).child("◇"))
                        .child(project.name.clone())
                        .child(
                            div()
                                .ml_auto()
                                .flex()
                                .items_center()
                                .gap_1()
                                .when_some(project_branch, |controls, branch| {
                                    controls.child(
                                        div()
                                            .px_1()
                                            .rounded_sm()
                                            .bg(rgba(0xe7e5deff))
                                            .text_size(px(9.5))
                                            .font_weight(gpui::FontWeight::NORMAL)
                                            .text_color(rgba(FG1))
                                            .child(branch),
                                    )
                                })
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(
                                            format!("project-add-{}", project.id).into(),
                                        ))
                                        .w(px(24.))
                                        .h(px(24.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(15.))
                                        .font_weight(gpui::FontWeight::NORMAL)
                                        .text_color(rgba(FG1))
                                        .hover(|s| s.bg(rgba(BG2)).text_color(rgba(ACCENT)))
                                        .on_click(cx.listener(move |this, _ev, _window, cx| {
                                            cx.stop_propagation();
                                            this.new_session_project =
                                                Some(project_id_for_add.clone());
                                            this.palette_open = true;
                                            dismiss_context_menus(
                                                &mut this.session_menu,
                                                &mut this.tree_menu,
                                            );
                                            cx.notify();
                                        }))
                                        .child("＋"),
                                ),
                        ),
                );
                if !project_collapsed {
                    for agent in snap.agents_of(&project.id) {
                        let id = agent.id.clone();
                        let active = self.active.as_deref() == Some(&id);
                        let mut row = div()
                            .id(gpui::ElementId::Name(id.clone().into()))
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(30.))
                            .pl_3()
                            .pr_2()
                            .border_l_2()
                            .border_color(rgba(if active { ACCENT } else { 0x00000000 }))
                            .text_size(px(11.5))
                            .text_color(rgba(if active { FG0 } else { FG1 }))
                            .hover(|s| s.bg(rgba(0xe9e7e0ff)))
                            .when(active, |el| el.bg(rgba(0xe7edfaff)))
                            .on_click(cx.listener({
                                let id = id.clone();
                                move |this, _ev, window, cx| {
                                    this.open_agent(&id, cx);
                                    this.focus_agent(&id, window, cx);
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener({
                                    let id = id.clone();
                                    move |this, ev: &gpui::MouseDownEvent, _window, cx| {
                                        this.tree_menu = None;
                                        this.session_menu = Some(SessionMenu {
                                            agent: id.clone(),
                                            position: ev.position,
                                            remote: false,
                                        });
                                        this.palette_open = false;
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                }),
                            );
                        row = row
                            .child(
                                div()
                                    .w(px(16.))
                                    .text_color(rgba(status_color(&agent.status)))
                                    .child(status_marker(&agent.status, self.spinner_frame)),
                            )
                            .child(truncate(&agent.title, 28))
                            .child(
                                div()
                                    .ml_auto()
                                    .text_size(px(9.5))
                                    .px_1()
                                    .rounded_sm()
                                    .bg(rgba(BG2))
                                    .text_color(rgba(FG1))
                                    .child(agent.agent_type.as_str().to_string()),
                            );
                        pnode = pnode.child(row);
                    }
                }
                tree = tree.child(pnode);
            }
        }

        // ── NOTIFICATIONS（侧栏下段）
        let mut notif_section = div()
            .flex()
            .flex_col()
            .mt_2()
            .border_t_1()
            .border_color(rgba(LINE))
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .text_size(px(10.))
                    .text_color(rgba(0x8b90a0ff))
                    .child("NOTIFICATIONS"),
            );
        for n in self.notifications.iter().take(8) {
            let nid = n.agent.clone();
            let color = status_color(&n.to);
            notif_section =
                notif_section.child(
                    div()
                        .id(gpui::ElementId::Name(format!("notif-{}", nid).into()))
                        .flex()
                        .flex_col()
                        .px_2()
                        .py_1()
                        .ml_2()
                        .border_l_2()
                        .border_color(rgba(color))
                        .when(n.unread, |el| el.bg(rgba(0xefe9f2ff)))
                        .hover(|s| s.bg(rgba(BG2)))
                        .text_size(px(11.))
                        .on_click(cx.listener({
                            let agent = n.agent.clone();
                            move |this, _ev, window, cx| {
                                // 标记已读 + 跳转
                                if let Some(nn) =
                                    this.notifications.iter_mut().find(|nn| nn.agent == agent)
                                {
                                    nn.unread = false;
                                }
                                this.open_agent(&agent, cx);
                                this.focus_agent(&agent, window, cx);
                            }
                        }))
                        .child(div().text_color(rgba(FG0)).child(
                            n.message.clone().unwrap_or_else(|| {
                                format!("{} → {}", n.from.as_str(), n.to.as_str())
                            }),
                        ))
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(rgba(0x8b90a0ff))
                                .child(format!("{} · {}", truncate(&n.agent, 26), n.time)),
                        ),
                );
        }
        if self.notifications.is_empty() {
            notif_section = notif_section.child(
                div()
                    .px_2()
                    .py_2()
                    .text_size(px(10.5))
                    .text_color(rgba(0x8b90a0ff))
                    .child("暂无通知，agent 停下时会出现在这里"),
            );
        }

        // ── 远程机器分组
        for host in &self.remotes {
            let name = host.cfg.name.clone();
            let machine_target = DeleteTarget::RemoteMachine { host: name.clone() };
            let (dot_color, status_text, remediation) = match self.remote_states.get(&name) {
                Some(lmux_client::RemoteState::Online(_)) => {
                    if self.remote_snaps.contains_key(&name) {
                        (ACCENT, "已连接", None)
                    } else {
                        (0x8b90a0ff, "连接中", None)
                    }
                }
                Some(lmux_client::RemoteState::NeedsInstall { .. }) => {
                    (YELLOW, "未安装", Some((true, false, None)))
                }
                Some(lmux_client::RemoteState::NeedsStart { binary, .. }) => {
                    (YELLOW, "未启动", Some((false, false, Some(binary.clone()))))
                }
                Some(lmux_client::RemoteState::NeedsUpgrade { .. }) => {
                    (YELLOW, "需要更新", Some((false, true, None)))
                }
                Some(lmux_client::RemoteState::AuthenticationFailed(_)) => (RED, "认证失败", None),
                Some(lmux_client::RemoteState::Connecting(stage)) => (YELLOW, stage.label(), None),
                Some(lmux_client::RemoteState::Offline(_)) => (0x8b90a0ff, "离线", None),
                _ => (0x8b90a0ff, "连接中", None),
            };
            let machine_key = format!("remote:{name}");
            let machine_collapsed = self.collapsed_machines.contains(&machine_key);
            let snap_ref = self.remote_snaps.get(&name);
            let remediation_host = name.clone();
            let remote_project_host = name.clone();
            let mut rnode = div().flex().flex_col().mt_2().child(
                div()
                    .id(gpui::ElementId::Name(format!("machine-row-{name}").into()))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(38.))
                    .px_2()
                    .border_b_1()
                    .border_color(rgba(LINE))
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgba(FG0))
                    .hover(|style| style.bg(rgba(0xe9e7e0ff)))
                    .on_click(cx.listener({
                        let key = machine_key.clone();
                        move |this, _event, _window, cx| {
                            if !this.collapsed_machines.remove(&key) {
                                this.collapsed_machines.insert(key.clone());
                            }
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .w(px(16.))
                            .text_color(rgba(FG1))
                            .child(if machine_collapsed { "▸" } else { "▾" }),
                    )
                    .child(div().w(px(18.)).text_color(rgba(dot_color)).child("▣"))
                    .child(name.clone())
                    .child(
                        div()
                            .ml_auto()
                            .px_1()
                            .rounded_sm()
                            .bg(rgba(BG2))
                            .text_size(px(9.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgba(dot_color))
                            .child(status_text.to_string()),
                    )
                    .child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("remote-project-add-{remote_project_host}").into(),
                            ))
                            .px_1()
                            .text_size(px(15.))
                            .text_color(rgba(FG1))
                            .hover(|style| style.bg(rgba(BG2)).text_color(rgba(ACCENT)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                cx.stop_propagation();
                                this.remote_project_dialog = Some(remote_project_host.clone());
                                this.dialog_error = None;
                                this.remote_project_input
                                    .update(cx, |input, cx| input.reset(cx));
                                this.remote_project_input.focus_handle(cx).focus(window, cx);
                                cx.notify();
                            }))
                            .child("＋"),
                    )
                    .when_some(remediation, |row, (install, upgrade, binary)| {
                        row.child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("remote-remediate-{remediation_host}").into(),
                                ))
                                .px_2()
                                .py_1()
                                .text_size(px(10.))
                                .text_color(rgba(ACCENT))
                                .hover(|style| style.bg(rgba(BG2)))
                                .on_click(cx.listener({
                                    let host = remediation_host.clone();
                                    move |this, _event, _window, cx| {
                                        this.bootstrap_error = None;
                                        this.bootstrap_confirm = Some(BootstrapConfirm {
                                            host: host.clone(),
                                            install,
                                            upgrade,
                                            binary: binary.clone(),
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                }))
                                .child(if install { "安装…" } else { "启动…" }),
                        )
                    })
                    .child(
                        div()
                            .px_1()
                            .text_size(px(14.))
                            .text_color(rgba(FG1))
                            .hover(|style| style.bg(rgba(BG2)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(
                                    move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                        this.session_menu = None;
                                        this.tree_menu = Some(TreeMenu {
                                            target: machine_target.clone(),
                                            position: event.position,
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    },
                                ),
                            )
                            .child("…"),
                    ),
            );
            if !machine_collapsed {
                if let Some(rsnap) = snap_ref {
                    for project in &rsnap.projects {
                        let project_key = format!("remote:{name}:{}", project.id);
                        let project_collapsed = self.collapsed_projects.contains(&project_key);
                        let remote_project_target = DeleteTarget::RemoteProject {
                            host: name.clone(),
                            project: project.id.clone(),
                            label: project.name.clone(),
                        };
                        let spawn_host = name.clone();
                        let spawn_project = project.id.clone();
                        let mut pnode = div()
                            .flex()
                            .flex_col()
                            .ml_3()
                            .border_l_1()
                            .border_color(rgba(LINE));
                        pnode = pnode.child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("remote-project-row-{name}-{}", project.id).into(),
                                ))
                                .flex()
                                .items_center()
                                .gap_1()
                                .h(px(32.))
                                .pl_1()
                                .pr_2()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgba(FG0))
                                .hover(|style| style.bg(rgba(0xe9e7e0ff)))
                                .on_click(cx.listener({
                                    let key = project_key.clone();
                                    move |this, _event, _window, cx| {
                                        if !this.collapsed_projects.remove(&key) {
                                            this.collapsed_projects.insert(key.clone());
                                        }
                                        cx.notify();
                                    }
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener({
                                        let target = remote_project_target.clone();
                                        move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                            this.session_menu = None;
                                            this.tree_menu = Some(TreeMenu {
                                                target: target.clone(),
                                                position: event.position,
                                            });
                                            cx.stop_propagation();
                                            cx.notify();
                                        }
                                    }),
                                )
                                .child(
                                    div()
                                        .w(px(16.))
                                        .text_color(rgba(FG1))
                                        .child(if project_collapsed { "▸" } else { "▾" }),
                                )
                                .child(div().w(px(18.)).text_color(rgba(ACCENT)).child("◇"))
                                .child(project.name.clone())
                                .child(
                                    div()
                                        .ml_auto()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .px_1()
                                                .rounded_sm()
                                                .bg(rgba(0xe3e9f6ff))
                                                .text_size(px(9.))
                                                .font_weight(gpui::FontWeight::NORMAL)
                                                .text_color(rgba(ACCENT))
                                                .child("远程"),
                                        )
                                        .child(
                                            div()
                                                .id(gpui::ElementId::Name(
                                                    format!(
                                                        "remote-session-add-{name}-{}",
                                                        project.id
                                                    )
                                                    .into(),
                                                ))
                                                .w(px(24.))
                                                .h(px(24.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(15.))
                                                .font_weight(gpui::FontWeight::NORMAL)
                                                .text_color(rgba(FG1))
                                                .hover(|style| {
                                                    style.bg(rgba(BG2)).text_color(rgba(ACCENT))
                                                })
                                                .on_click(cx.listener(
                                                    move |this, _event, window, cx| {
                                                        cx.stop_propagation();
                                                        this.spawn_remote_shell(
                                                            spawn_host.clone(),
                                                            spawn_project.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .child("＋"),
                                        ),
                                ),
                        );
                        if !project_collapsed {
                            for agent in rsnap.agents_of(&project.id) {
                                let id = agent.id.clone();
                                let active = self.active.as_deref() == Some(&id);
                                let mut row = div()
                                    .id(gpui::ElementId::Name(id.clone().into()))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .h(px(30.))
                                    .pl_3()
                                    .pr_2()
                                    .border_l_2()
                                    .border_color(rgba(if active { ACCENT } else { 0x00000000 }))
                                    .text_size(px(11.5))
                                    .text_color(rgba(if active { FG0 } else { FG1 }))
                                    .hover(|s| s.bg(rgba(0xe9e7e0ff)))
                                    .when(active, |el| el.bg(rgba(0xe7edfaff)))
                                    .on_click(cx.listener({
                                        let id = id.clone();
                                        move |this, _ev, window, cx| {
                                            this.open_remote_agent(&id, cx);
                                            this.focus_agent(&id, window, cx);
                                        }
                                    }))
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener({
                                            let id = id.clone();
                                            move |this, ev: &gpui::MouseDownEvent, _window, cx| {
                                                this.tree_menu = None;
                                                this.session_menu = Some(SessionMenu {
                                                    agent: id.clone(),
                                                    position: ev.position,
                                                    remote: true,
                                                });
                                                this.palette_open = false;
                                                cx.stop_propagation();
                                                cx.notify();
                                            }
                                        }),
                                    );
                                row = row
                                    .child(
                                        div()
                                            .w(px(16.))
                                            .text_color(rgba(status_color(&agent.status)))
                                            .child(status_marker(
                                                &agent.status,
                                                self.spinner_frame,
                                            )),
                                    )
                                    .child(truncate(&agent.title, 24))
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_size(px(9.5))
                                            .px_1()
                                            .rounded_sm()
                                            .bg(rgba(BG2))
                                            .text_color(rgba(ACCENT))
                                            .child(agent.agent_type.as_str().to_string()),
                                    );
                                pnode = pnode.child(row);
                            }
                        }
                        rnode = rnode.child(pnode);
                    }
                }
            }
            tree = tree.child(rnode);
        }

        // notifications 在 sidebar 底部固定渲染，不随机器树滚动。

        // ── 终端网格：PaneTree 递归渲染。默认一个 pane + 多 tabs；显式 ↔/↕ 才 split。
        let render_tree = if let Some(max) = &self.maximized_pane {
            self.pane_tree
                .group(max)
                .cloned()
                .map(|group| PaneNode::Leaf { group })
                .unwrap_or_else(|| self.pane_tree.clone())
        } else {
            self.pane_tree.clone()
        };
        let grid = div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(rgba(BG0))
            .child(self.render_pane_node(render_tree, &snap, cx));

        // ── 根布局：侧栏 + 网格（贴边、零 padding）
        let mut root = div()
            .id("lmux-root")
            .relative()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| {
                this.palette_open = !this.palette_open;
                if this.palette_open {
                    this.palette_index = 0;
                    this.focus.focus(window, cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseTab, _window, cx| {
                let pane = this.active_pane.clone();
                if let Some(agent) = this
                    .pane_tree
                    .group(&pane)
                    .and_then(|group| group.active.clone())
                {
                    this.close_tab(&pane, &agent, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NewShellTab, window, cx| {
                let pane = this.active_pane.clone();
                this.new_shell_tab(&pane, window, cx);
            }))
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, window, cx| {
                if this.palette_open {
                    this.handle_palette_key(&ev.keystroke, window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_drag_move::<DividerDrag>(cx.listener(
                |this, ev: &gpui::DragMoveEvent<DividerDrag>, _window, cx| {
                    if this.split_drag.is_some() {
                        this.update_split_drag(ev.event.position, cx);
                    }
                },
            ))
            .on_drop::<DividerDrag>(cx.listener(|this, _drag, _window, _cx| {
                this.end_split_drag();
            }))
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _window, cx| {
                if this.split_drag.is_some() {
                    this.update_split_drag(ev.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, _cx| this.end_split_drag()),
            )
            .on_action(cx.listener(|this, _: &ClosePalette, _window, cx| {
                if this.palette_open
                    || this.session_menu.is_some()
                    || this.tree_menu.is_some()
                    || this.delete_confirm.is_some()
                    || this.bootstrap_confirm.is_some()
                    || this.connect_dialog
                    || this.project_dialog
                    || this.remote_project_dialog.is_some()
                {
                    this.palette_open = false;
                    dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu);
                    this.delete_confirm = None;
                    this.bootstrap_confirm = None;
                    this.connect_dialog = false;
                    this.project_dialog = false;
                    this.remote_project_dialog = None;
                    this.dialog_error = None;
                    cx.notify();
                }
            }))
            .flex()
            .size_full()
            .bg(rgba(BG0))
            .text_color(rgba(FG0))
            .font_family("JetBrains Mono")
            .child(
                div()
                    .w(px(280.))
                    .flex()
                    .flex_col()
                    .bg(rgba(BG1))
                    .border_r_1()
                    .border_color(rgba(LINE))
                    .child(div().flex_1().overflow_hidden().child(tree))
                    .child(
                        div()
                            .max_h(px(220.))
                            .overflow_hidden()
                            .border_t_1()
                            .border_color(rgba(LINE))
                            .child(notif_section),
                    )
                    .child(
                        div()
                            .id("connect-machine")
                            .mx_2()
                            .mb_2()
                            .px_3()
                            .py_2()
                            .border_1()
                            .border_color(rgba(LINE))
                            .text_size(px(11.))
                            .text_color(rgba(FG1))
                            .hover(|s| s.bg(rgba(BG2)))
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                this.connect_dialog = true;
                                this.connect_focus_index = 0;
                                this.project_dialog = false;
                                this.dialog_error = None;
                                this.connect_input.update(cx, |input, cx| input.reset(cx));
                                this.connect_username
                                    .update(cx, |input, cx| input.reset(cx));
                                this.connect_password
                                    .update(cx, |input, cx| input.reset(cx));
                                this.connect_key_path
                                    .update(cx, |input, cx| input.reset(cx));
                                this.connect_input.focus_handle(cx).focus(window, cx);
                                cx.notify();
                            }))
                            .child("＋ 连接远程机器"),
                    ),
            )
            .child(grid);
        if self.palette_open {
            root = root.child(self.render_palette(cx));
        }
        if self.connect_dialog {
            root = root.child(self.render_connect_dialog(cx));
        }
        if self.project_dialog {
            root = root.child(self.render_project_dialog(cx));
        }
        if self.remote_project_dialog.is_some() {
            root = root.child(self.render_remote_project_dialog(cx));
        }
        if self.session_menu.is_some() {
            root = root.child(self.render_session_menu(cx));
        }
        if self.tree_menu.is_some() {
            root = root.child(self.render_tree_menu(cx));
        }
        if self.delete_confirm.is_some() {
            root = root.child(self.render_delete_confirm(cx));
        }
        if self.bootstrap_confirm.is_some() {
            root = root.child(self.render_bootstrap_confirm(cx));
        }
        root
    }
}

fn now_hhmm() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, m) = ((t / 3600 + 8) % 24, (t / 60) % 60); // UTC+8
    format!("{h:02}:{m:02}")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        let t: String = s.chars().take(n).collect();
        format!("{t}…")
    } else {
        s.to_string()
    }
}

fn session_title(snap: &Snapshot, agent: &AgentId) -> String {
    snap.agent(agent)
        .map(|a| {
            let title = a.title.trim();
            if title.is_empty() {
                a.agent_type.as_str().to_string()
            } else {
                title.to_string()
            }
        })
        .unwrap_or_else(|| "session".into())
}

fn agent_label(snap: &Snapshot, agent: &AgentId) -> String {
    snap.agent(agent)
        .map(|a| format!("{} · {}", a.agent_type.as_str(), a.status.as_str()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_body_preserves_content_and_has_status_fallbacks() {
        assert_eq!(
            effective_notification_body(
                lmux_core::model::AgentStatus::Done,
                Some("  完成了修复\n并通过测试  ".into()),
            ),
            "完成了修复 并通过测试"
        );
        assert_eq!(
            effective_notification_body(lmux_core::model::AgentStatus::Done, Some("  ".into())),
            "任务已完成"
        );
        assert_eq!(
            effective_notification_body(lmux_core::model::AgentStatus::Blocked, None),
            "等待输入"
        );
    }

    #[test]
    fn local_project_path_requires_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_local_project_path(dir.path().to_str().unwrap()),
            Some(dir.path().canonicalize().unwrap())
        );
        assert!(resolve_local_project_path("/definitely/missing/lmux-project").is_none());
    }

    #[test]
    fn dismissing_context_menus_clears_session_and_tree_menus() {
        let mut session_menu = Some(SessionMenu {
            agent: "agent-1".into(),
            position: Point::new(px(10.), px(20.)),
            remote: false,
        });
        let mut tree_menu = Some(TreeMenu {
            target: DeleteTarget::LocalProject {
                project: "project-1".into(),
                label: "demo".into(),
            },
            position: Point::new(px(30.), px(40.)),
        });

        assert!(dismiss_context_menus(&mut session_menu, &mut tree_menu));
        assert!(session_menu.is_none());
        assert!(tree_menu.is_none());
        assert!(!dismiss_context_menus(&mut session_menu, &mut tree_menu));
    }

    #[test]
    fn explicit_split_launch_config_is_always_shell() {
        let cfg = shell_split_launch_cfg(
            "new-pane".into(),
            std::env::temp_dir(),
            "/tmp/lmux.sock".into(),
            "token".into(),
            "lmux-new-pane".into(),
        );
        assert_eq!(cfg.agent_type, lmux_core::model::AgentType::Shell);
        assert!(cfg
            .env
            .iter()
            .any(|(key, value)| key == "LMUX_AGENT_ID" && value == "new-pane"));
        assert!(cfg.program_override.is_none());
        assert_eq!(cfg.tmux_session.as_deref(), Some("lmux-new-pane"));
    }
}
