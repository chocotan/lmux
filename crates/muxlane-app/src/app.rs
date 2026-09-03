//! MuxlaneApp：根组件。侧栏（机器树+通知）+ 贴边终端网格。
use crate::i18n::{self, Language};
use crate::sound::{self, SoundKind};
use crate::term_view::TermView;
use crate::text_field::TextField;
use crate::theme::{Theme, ThemeMode};
use gpui::{
    canvas, deferred, div, prelude::*, px, relative, rgba, size, svg, App, AssetSource, Bounds,
    Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton, ParentElement, Pixels, Point,
    Render, ScrollHandle, SharedString, Styled, Svg, Window, WindowBounds, WindowOptions,
};
use muxlane_core::model::{AgentId, Snapshot};
use muxlane_core::{PaneId, PaneNode, SplitAxis};
use muxlane_server::MuxlaneServer;
use muxlane_term::VTerm;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

gpui::actions!(
    muxlane,
    [
        TogglePalette,
        CloseTab,
        NewShellTab,
        NextTab,
        PrevTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectTab9,
        ToggleTheme,
    ]
);

const SPLIT_HORIZONTAL_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><rect x='3' y='4' width='18' height='16' rx='2'/><path d='M12 4v16'/></svg>"#;
const SPLIT_VERTICAL_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><rect x='3' y='4' width='18' height='16' rx='2'/><path d='M3 12h18'/></svg>"#;
const MAXIMIZE_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M8 3H3v5M16 3h5v5M21 16v5h-5M3 16v5h5'/></svg>"#;
const RESTORE_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><rect x='7' y='7' width='13' height='13' rx='1'/><path d='M17 7V4a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h3'/></svg>"#;
const CLOSE_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round'><path d='M6 6l12 12M18 6L6 18'/></svg>"#;
const CONNECT_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M7 7h10v10H7z'/><path d='M4 4h10M4 4v10M20 20H10M20 20V10'/></svg>"#;
const THEME_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M20 15.5A8.5 8.5 0 1 1 8.5 4 6.5 6.5 0 0 0 20 15.5z'/></svg>"#;
const NOTIFICATION_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9'/><path d='M10 21h4'/></svg>"#;
const PLUS_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.8' stroke-linecap='round'><path d='M12 5v14M5 12h14'/></svg>"#;
const SETTINGS_ICON: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='#000' stroke-width='1.7' stroke-linejoin='round'><path d='M9.38 5.67L10.72 3.04H13.28L14.62 5.67L17.43 4.76L19.24 6.57L18.33 9.38L20.96 10.72V13.28L18.33 14.62L19.24 17.43L17.43 19.24L14.62 18.33L13.28 20.96H10.72L9.38 18.33L6.57 19.24L4.76 17.43L5.67 14.62L3.04 13.28V10.72L5.67 9.38L4.76 6.57L6.57 4.76Z'/><circle cx='12' cy='12' r='3'/></svg>"#;
const SVG_ASSETS: &[(&str, &[u8])] = &[
    ("icons/split-horizontal.svg", SPLIT_HORIZONTAL_ICON),
    ("icons/split-vertical.svg", SPLIT_VERTICAL_ICON),
    ("icons/maximize.svg", MAXIMIZE_ICON),
    ("icons/restore.svg", RESTORE_ICON),
    ("icons/close.svg", CLOSE_ICON),
    ("icons/connect.svg", CONNECT_ICON),
    ("icons/theme.svg", THEME_ICON),
    ("icons/notification.svg", NOTIFICATION_ICON),
    ("icons/plus.svg", PLUS_ICON),
    ("icons/settings.svg", SETTINGS_ICON),
];
const FONT_FAMILIES: &[&str] = &[
    "Noto Sans Mono",
    "JetBrains Mono",
    "Iosevka",
    "DejaVu Sans Mono",
    "Liberation Mono",
];
const DEFAULT_FONT_FAMILY: &str = "Noto Sans Mono";

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(svg_asset(path).map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

pub fn launch(
    server: Arc<MuxlaneServer>,
    initial_snapshot: Snapshot,
    connect_to: Vec<String>,
    persisted: muxlane_store::PersistedApp,
    store_path: PathBuf,
) {
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.bind_keys([
                KeyBinding::new("ctrl-k", TogglePalette, None),
                KeyBinding::new("ctrl-w", CloseTab, None),
                KeyBinding::new("ctrl-shift-t", NewShellTab, None),
                KeyBinding::new("ctrl-tab", NextTab, None),
                KeyBinding::new("ctrl-shift-tab", PrevTab, None),
                KeyBinding::new("alt-1", SelectTab1, None),
                KeyBinding::new("alt-2", SelectTab2, None),
                KeyBinding::new("alt-3", SelectTab3, None),
                KeyBinding::new("alt-4", SelectTab4, None),
                KeyBinding::new("alt-5", SelectTab5, None),
                KeyBinding::new("alt-6", SelectTab6, None),
                KeyBinding::new("alt-7", SelectTab7, None),
                KeyBinding::new("alt-8", SelectTab8, None),
                KeyBinding::new("alt-9", SelectTab9, None),
            ]);
            let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
            let _ = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Muxlane");
                    cx.new(|cx| {
                        MuxlaneApp::new(
                            cx,
                            Arc::clone(&server),
                            initial_snapshot.clone(),
                            connect_to.clone(),
                            persisted.clone(),
                            store_path.clone(),
                        )
                    })
                },
            );
            cx.activate(true);
        });
}

pub(crate) fn svg_asset(path: &str) -> Option<&'static [u8]> {
    SVG_ASSETS
        .iter()
        .find_map(|(asset_path, data)| (*asset_path == path).then_some(*data))
}

fn panel_icon(data: &[u8], color: u32) -> Svg {
    let path = SVG_ASSETS
        .iter()
        .find_map(|(path, bytes)| (*bytes == data).then_some(*path))
        .expect("panel icon must be registered");
    svg().path(path).size(px(15.)).text_color(rgba(color))
}

struct HoverTip {
    text: SharedString,
}

impl Render for HoverTip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgba(0x1a1d24f2))
            .border_1()
            .border_color(rgba(0x00000066))
            .text_size(px(11.))
            .text_color(rgba(0xffffffff))
            .child(self.text.clone())
    }
}

fn hover_tip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut gpui::App) -> gpui::AnyView {
    let text = text.into();
    move |_, cx| cx.new(|_| HoverTip { text: text.clone() }).into()
}
#[derive(Clone)]
struct DragTab {
    agent: AgentId,
    from_pane: PaneId,
}

struct DragGhost {
    label: SharedString,
    offset: Point<Pixels>,
    theme: Theme,
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
                    .bg(rgba(self.theme.bg2))
                    .border_1()
                    .border_color(rgba(self.theme.line))
                    .text_size(px(11.))
                    .text_color(rgba(self.theme.fg0))
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
    status: muxlane_core::model::AgentStatus,
    message: Option<String>,
) -> String {
    let normalized = message
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return match status {
            muxlane_core::model::AgentStatus::Blocked => "等待输入".into(),
            muxlane_core::model::AgentStatus::Done => "任务已完成".into(),
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum NewSessionTarget {
    Local(muxlane_core::model::ProjectId),
    Remote {
        host: String,
        project: muxlane_core::model::ProjectId,
    },
}

pub struct MuxlaneApp {
    focus: FocusHandle,
    server: Arc<MuxlaneServer>,
    /// 递归 pane tree；每个 Leaf 内是 TabGroup（参考 muxel）
    pane_tree: PaneNode,
    active_pane: PaneId,
    maximized_pane: Option<PaneId>,
    terms: HashMap<AgentId, Entity<TermView>>,
    mirror_cancel: HashMap<AgentId, Arc<std::sync::atomic::AtomicBool>>,
    active: Option<AgentId>,
    last_snapshot: Snapshot,
    /// 远程机器（本地快照缓存：host name → snapshot）
    remotes: Vec<Arc<muxlane_client::RemoteHost>>,
    remote_snaps: HashMap<String, Snapshot>,
    remote_states: HashMap<String, muxlane_client::RemoteState>,
    /// 通知中心（新事件 unshift，上限 50）
    notifications: Vec<Notification>,
    toasts: Vec<ToastNotification>,
    toast_seq: u64,
    theme_mode: ThemeMode,
    font_family: String,
    notifications_open: bool,
    settings_open: bool,
    settings_theme_menu: bool,
    settings_font_menu: bool,
    settings_language_menu: bool,
    sound_enabled: bool,
    language: Language,
    palette_open: bool,
    palette_index: usize,
    palette_scroll: ScrollHandle,
    palette_input: Entity<TextField>,
    presets: Vec<muxlane_core::AgentPreset>,
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
    remote_event_tx: tokio::sync::mpsc::Sender<muxlane_client::ClientEvent>,
    new_session_target: Option<NewSessionTarget>,
    session_menu: Option<SessionMenu>,
    tree_menu: Option<TreeMenu>,
    delete_confirm: Option<DeleteConfirm>,
    delete_error: Option<String>,
    delete_busy: bool,
    bootstrap_confirm: Option<BootstrapConfirm>,
    bootstrap_error: Option<String>,
    store_path: std::path::PathBuf,
    split_drag: Option<SplitDrag>,
    split_metrics: Arc<std::sync::Mutex<HashMap<String, f32>>>,
    spinner_frame: usize,
    pulse_phase: usize,
    collapsed_machines: std::collections::HashSet<String>,
    collapsed_projects: std::collections::HashSet<String>,
    /// 远端安装/升级进度（host → 进度）
    bootstrap_progress: HashMap<String, muxlane_client::BootstrapProgress>,
    /// 远程操作失败的短暂提示
    error_toast: Option<(String, std::time::Instant)>,
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
    pub machine_name: String,
    pub project_name: String,
    pub to: muxlane_core::model::AgentStatus,
    pub message: Option<String>,
    pub unread: bool,
    pub time_secs: u64,
}

#[derive(Clone)]
pub struct ToastNotification {
    pub id: u64,
    pub agent: AgentId,
    pub title: String,
    pub message: String,
    pub status: muxlane_core::model::AgentStatus,
    pub created_at: std::time::Instant,
}

#[derive(Clone)]
enum PaletteItem {
    Preset {
        preset: muxlane_core::AgentPreset,
    },
    Action {
        id: &'static str,
        label: &'static str,
        shortcut: Option<&'static str>,
        icon: &'static [u8],
    },
}

impl Focusable for MuxlaneApp {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MuxlaneApp {
    pub fn new(
        cx: &mut Context<Self>,
        server: Arc<MuxlaneServer>,
        initial_snapshot: Snapshot,
        connect_to: Vec<String>,
        persisted: muxlane_store::PersistedApp,
        store_path: std::path::PathBuf,
    ) -> Self {
        // 本地状态事件 → 通知列表
        let mut local_rx = server.subscribe_events();
        cx.spawn(async move |this, cx| loop {
            match local_rx.recv().await {
                Ok(ev) => {
                    if ev.event == muxlane_core::protocol::events::AGENT_STATUS {
                        if let Ok(p) = serde_json::from_value::<
                            muxlane_core::protocol::AgentStatusEvent,
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

        // 每 1s 拉一次快照（P0 轮询；P1 换事件驱动）
        let server_for_poll = Arc::clone(&server);
        cx.spawn(async move |this, cx| loop {
            let server = Arc::clone(&server_for_poll);
            let snap = cx
                .background_spawn(async move { server.snapshot().await })
                .await;
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

        // working spinner / attention pulse / toast 超时只在确实需要动画时运行。
        cx.spawn(async move |this, cx| {
            let mut anim_tick: usize = 0;
            loop {
                let should_animate = match this.update(cx, |this, _cx| this.should_animate()) {
                    Ok(should_animate) => should_animate,
                    Err(_) => break,
                };
                let delay = if should_animate {
                    std::time::Duration::from_millis(100)
                } else {
                    std::time::Duration::from_millis(250)
                };
                cx.background_executor().timer(delay).await;
                if !should_animate {
                    continue;
                }
                if this
                    .update(cx, |this, cx| {
                        anim_tick = anim_tick.wrapping_add(1);
                        this.spinner_frame = anim_tick % 8; // 100ms 每帧旋转
                        this.pulse_phase = anim_tick % 36; // 3.6s 完整平滑呼吸周期
                        if anim_tick.is_multiple_of(10) {
                            this.toasts.retain(|t| t.created_at.elapsed().as_secs() < 6);
                            if this
                                .error_toast
                                .as_ref()
                                .is_some_and(|(_, created)| created.elapsed().as_secs() >= 8)
                            {
                                this.error_toast = None;
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        // ── 远程机器接入（socket 直连；SSH 隧道在 tunnel.rs）
        let mut remotes = Vec::new();
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        for saved in &persisted.remote_configs {
            let target = muxlane_client::parse_target(&saved.target);
            let name = match &target {
                muxlane_client::Target::Socket(path) => {
                    path.rsplit('/').next().unwrap_or(path).to_string()
                }
                muxlane_client::Target::Ssh { host, .. } => host.clone(),
            };
            let auth = match &saved.auth {
                muxlane_store::PersistedRemoteAuth::SshConfig => muxlane_client::SshAuth::SshConfig,
                muxlane_store::PersistedRemoteAuth::PublicKey {
                    username,
                    identity_file,
                } => muxlane_client::SshAuth::PublicKey {
                    username: username.clone(),
                    identity_file: identity_file.clone(),
                },
                muxlane_store::PersistedRemoteAuth::Password { username, password } => {
                    muxlane_client::SshAuth::Password {
                        username: username.clone(),
                        password: password.clone().unwrap_or_default(),
                    }
                }
            };
            let remote = muxlane_client::RemoteHost::new(
                muxlane_client::HostCfg {
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
            let parsed = muxlane_client::parse_target(target);
            let name = match &parsed {
                muxlane_client::Target::Socket(path) => {
                    path.rsplit('/').next().unwrap_or(path).to_string()
                }
                muxlane_client::Target::Ssh { host, .. } => host.clone(),
            };
            let cfg = muxlane_client::HostCfg {
                name,
                target: parsed,
                auth: muxlane_client::SshAuth::SshConfig,
                retry_base_ms: 500,
            };
            if remotes
                .iter()
                .any(|remote: &Arc<muxlane_client::RemoteHost>| remote.cfg.name == cfg.name)
            {
                continue;
            }
            let host = muxlane_client::RemoteHost::new(cfg, tx.clone());
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
                            muxlane_client::ClientEvent::StateChanged { host, state } => {
                                if let muxlane_client::RemoteState::Online(snap) = &state {
                                    let mut snap = snap.clone();
                                    if let Some(active) = this.active.as_ref() {
                                        if let Some(agent) = snap.agent_mut(active) {
                                            agent.seen = true;
                                        }
                                    }
                                    this.remote_snaps.insert(host.clone(), snap);
                                }
                                // 到达稳态后清除进度显示
                                if !matches!(state, muxlane_client::RemoteState::Connecting(_)) {
                                    this.bootstrap_progress.remove(&host);
                                }
                                this.remote_states.insert(host, state);
                            }
                            muxlane_client::ClientEvent::BootstrapProgress { host, progress } => {
                                this.bootstrap_progress.insert(host, progress);
                            }
                            muxlane_client::ClientEvent::StatusChanged {
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
                                        if to == muxlane_core::model::AgentStatus::Done {
                                            a.seen = this.active.as_ref() != Some(&agent);
                                        }
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
        let theme_mode = persisted
            .theme
            .as_deref()
            .and_then(ThemeMode::from_id)
            .unwrap_or_else(|| {
                if persisted.dark_mode.unwrap_or(false) {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                }
            });
        let font_family = persisted
            .font_family
            .as_deref()
            .filter(|font| FONT_FAMILIES.contains(font))
            .unwrap_or(DEFAULT_FONT_FAMILY)
            .to_string();
        let language = persisted
            .language
            .as_deref()
            .and_then(Language::from_id)
            .unwrap_or_else(Language::detect);
        let palette_input = cx.new(|cx| {
            let mut field = TextField::new("输入命令、项目名或 Agent 名…", cx);
            field.set_theme_mode(theme_mode, cx);
            field
        });

        let connect_input = cx.new(|cx| {
            let mut field = TextField::new("nuc 或 192.168.1.20", cx);
            field.set_theme_mode(theme_mode, cx);
            field
        });

        let connect_username = cx.new(|cx| {
            let mut field = TextField::new("用户名（可选）", cx);
            field.set_theme_mode(theme_mode, cx);
            field
        });

        let connect_password = cx.new(|cx| {
            let mut field = TextField::new_secure("密码", cx);
            field.set_theme_mode(theme_mode, cx);
            field
        });

        let connect_key_path = cx.new(|cx| {
            let mut field = TextField::new("私钥路径（可选）", cx);
            field.set_theme_mode(theme_mode, cx);
            field
        });

        let project_input = cx.new(|cx| {
            let mut field = TextField::new("~/projects/my-project", cx);
            field.set_theme_mode(theme_mode, cx);
            field
        });

        let remote_project_input = cx.new(|cx| {
            let mut field = TextField::new("~/projects/remote-project", cx);
            field.set_theme_mode(theme_mode, cx);
            field
        });

        let mut app = MuxlaneApp {
            focus: cx.focus_handle(),
            server,
            pane_tree: restored_tree,
            active_pane: restored_active,
            // 最大化是 transient 状态（muxel 同款）：不跨重启保留
            maximized_pane: None,
            terms: HashMap::new(),
            mirror_cancel: HashMap::new(),
            active: None,
            last_snapshot: initial_snapshot,
            remotes,
            remote_snaps: HashMap::new(),
            remote_states: HashMap::new(),
            notifications: Vec::new(),
            toasts: Vec::new(),
            toast_seq: 0,
            theme_mode,
            font_family,
            notifications_open: false,
            settings_open: false,
            settings_theme_menu: false,
            settings_font_menu: false,
            settings_language_menu: false,
            sound_enabled: persisted.sound_enabled.unwrap_or(true),
            language,
            palette_open: false,
            palette_index: 0,
            palette_scroll: ScrollHandle::new(),
            palette_input,
            presets: muxlane_core::builtin_presets(muxlane_term::default_shell_program()),
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
            new_session_target: None,
            session_menu: None,
            tree_menu: None,
            delete_confirm: None,
            delete_error: None,
            delete_busy: false,
            bootstrap_confirm: None,
            bootstrap_error: None,
            store_path,
            split_drag: None,
            split_metrics: Arc::new(std::sync::Mutex::new(HashMap::new())),
            spinner_frame: 0,
            pulse_phase: 0,
            collapsed_machines: std::collections::HashSet::new(),
            collapsed_projects: std::collections::HashSet::new(),
            bootstrap_progress: HashMap::new(),
            error_toast: None,
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
            let server = Arc::clone(&app.server);
            let session_agent = agent.clone();
            cx.spawn(async move |this, cx| {
                let session = cx
                    .background_spawn(async move { server.session(&session_agent).await })
                    .await;
                if let Some(session) = session {
                    let _ = this.update(cx, |this, cx| {
                        let term = Self::create_local_term(
                            agent.clone(),
                            session,
                            &this.font_family,
                            Theme::for_mode(this.theme_mode),
                            cx,
                        );
                        this.terms.insert(agent, term);
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        // 仅 UI 自动化使用；真实交互仍由用户点击 agent 打开 tab。
        if std::env::var("MUXLANE_TEST_AUTO_OPEN").as_deref() == Ok("1") {
            if let Some(id) = first_agent {
                app.open_agent(&id, cx);
            }
        }
        app
    }

    fn persist(&self) {
        let remote_configs: Vec<muxlane_store::PersistedRemote> = self
            .remotes
            .iter()
            .map(|host| {
                let target = match &host.cfg.target {
                    muxlane_client::Target::Socket(path) => path.clone(),
                    muxlane_client::Target::Ssh { host, socket } if socket.is_empty() => {
                        host.clone()
                    }
                    muxlane_client::Target::Ssh { host, socket } => format!("{host}:{socket}"),
                };
                let auth = match &host.cfg.auth {
                    muxlane_client::SshAuth::SshConfig => {
                        muxlane_store::PersistedRemoteAuth::SshConfig
                    }
                    muxlane_client::SshAuth::PublicKey {
                        username,
                        identity_file,
                    } => muxlane_store::PersistedRemoteAuth::PublicKey {
                        username: username.clone(),
                        identity_file: identity_file.clone(),
                    },
                    muxlane_client::SshAuth::Password { username, password } => {
                        muxlane_store::PersistedRemoteAuth::Password {
                            username: username.clone(),
                            password: if password.is_empty() {
                                None
                            } else {
                                Some(password.clone())
                            },
                        }
                    }
                };
                muxlane_store::PersistedRemote { target, auth }
            })
            .collect();
        let mut app = muxlane_store::PersistedApp::from_snapshot(&self.last_snapshot);
        app.remotes = remote_configs
            .iter()
            .map(|remote| remote.target.clone())
            .collect();
        app.remote_configs = remote_configs;
        app.pane_tree = self.pane_tree.clone();
        app.active_pane = Some(self.active_pane.clone());
        app.dark_mode = Some(self.theme_mode.is_dark());
        app.theme = Some(self.theme_mode.id().into());
        app.font_family = Some(self.font_family.clone());
        app.sound_enabled = Some(self.sound_enabled);
        app.language = Some(self.language.id().into());
        if let Err(e) = muxlane_store::save(&self.store_path, &app) {
            tracing::warn!(error = %e, "persist state failed");
        }
    }

    fn find_agent(&self, agent: &AgentId) -> Option<muxlane_core::model::AgentInstance> {
        if let Some(a) = self.last_snapshot.agent(agent) {
            return Some(a.clone());
        }
        for snap in self.remote_snaps.values() {
            if let Some(a) = snap.agent(agent) {
                return Some(a.clone());
            }
        }
        None
    }

    fn should_animate(&self) -> bool {
        let attention = |snapshot: &Snapshot| {
            snapshot.agents.iter().any(|agent| {
                matches!(
                    agent.status,
                    muxlane_core::model::AgentStatus::Working
                        | muxlane_core::model::AgentStatus::Blocked
                ) || (agent.status == muxlane_core::model::AgentStatus::Done && !agent.seen)
            })
        };

        !self.toasts.is_empty()
            || self.error_toast.is_some()
            || attention(&self.last_snapshot)
            || self.remote_snaps.values().any(attention)
    }

    fn push_notification(
        &mut self,
        agent: AgentId,
        from: muxlane_core::model::AgentStatus,
        to: muxlane_core::model::AgentStatus,
        message: Option<String>,
    ) {
        let body = effective_notification_body(to, message);
        let focused = self.active.as_ref() == Some(&agent);
        let now_secs = muxlane_core::model::now_secs();

        let (machine_name, project_name) = {
            if let Some(a) = self.last_snapshot.agent(&agent) {
                let proj = self
                    .last_snapshot
                    .projects
                    .iter()
                    .find(|p| p.id == a.project)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "project".into());
                ("local".to_string(), proj)
            } else {
                let mut found = None;
                for (host, snap) in &self.remote_snaps {
                    if let Some(a) = snap.agent(&agent) {
                        let proj = snap
                            .projects
                            .iter()
                            .find(|p| p.id == a.project)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| "project".into());
                        found = Some((host.clone(), proj));
                        break;
                    }
                }
                found.unwrap_or_else(|| ("remote".into(), "project".into()))
            }
        };

        if from == to {
            if let Some(existing) = self
                .notifications
                .iter_mut()
                .find(|item| item.agent == agent && item.to == to)
            {
                existing.message = Some(body.clone());
                existing.unread = !focused;
                existing.time_secs = now_secs;
            }
            return;
        }
        // blocked / done 才进通知中心（working/idle 刷屏没意义）
        if !matches!(
            to,
            muxlane_core::model::AgentStatus::Blocked | muxlane_core::model::AgentStatus::Done
        ) {
            return;
        }
        self.toast_seq += 1;
        self.notifications.insert(
            0,
            Notification {
                agent: agent.clone(),
                machine_name: machine_name.clone(),
                project_name: project_name.clone(),
                to,
                message: Some(body.clone()),
                unread: !focused,
                time_secs: now_secs,
            },
        );
        if self.notifications.len() > 50 {
            self.notifications.truncate(50);
        }

        let toast_title = match to {
            muxlane_core::model::AgentStatus::Blocked => {
                format!("{machine_name} · {project_name} 等待输入")
            }
            muxlane_core::model::AgentStatus::Done => {
                format!("{machine_name} · {project_name} 任务完成")
            }
            _ => format!("{machine_name} · {project_name}"),
        };

        if focused {
            // 当前终端已在看：保留通知记录，但不弹 Toast、不播放声音。
            sound::send_desktop_notification(&toast_title, &body);
            return;
        }

        self.toasts.insert(
            0,
            ToastNotification {
                id: self.toast_seq,
                agent,
                title: toast_title.clone(),
                message: body.clone(),
                status: to,
                created_at: std::time::Instant::now(),
            },
        );
        if self.toasts.len() > 3 {
            self.toasts.truncate(3);
        }

        if self.sound_enabled {
            match to {
                muxlane_core::model::AgentStatus::Blocked => {
                    sound::play_sound(SoundKind::Request);
                }
                muxlane_core::model::AgentStatus::Done => {
                    sound::play_sound(SoundKind::Done);
                }
                _ => {}
            }
        }

        // 系统桌面通知
        sound::send_desktop_notification(&toast_title, &body);
    }

    fn activate_tab(&mut self, pane: &PaneId, agent: &AgentId) {
        self.pane_tree.open_tab(pane, agent.clone());
        // 跨 pane 的显式导航必须揭示目标 pane；同 pane 切 tab 保留 zoom。
        if self
            .maximized_pane
            .as_ref()
            .is_some_and(|maximized| maximized != pane)
        {
            self.maximized_pane = None;
        }
        self.active_pane = pane.clone();
        self.active = Some(agent.clone());
        // 清理当前 agent 的 Toast 与标记通知已读
        self.toasts.retain(|t| &t.agent != agent);
        for n in self.notifications.iter_mut().filter(|n| &n.agent == agent) {
            n.unread = false;
        }
        if let Some(a) = self.last_snapshot.agent_mut(agent) {
            a.seen = true;
            if a.status == muxlane_core::model::AgentStatus::Done {
                a.status = muxlane_core::model::AgentStatus::Idle;
            }
        }
        // 本地 Done 会话查看后回到 Idle（herdr seen 语义）。
        if self.last_snapshot.agent(agent).is_some() {
            let server = Arc::clone(&self.server);
            let agent = agent.clone();
            server.rt_spawn({
                let server = Arc::clone(&server);
                async move { server.mark_seen(&agent).await }
            });
        }
        self.persist();
    }

    fn mark_agent_working(&mut self, agent: &AgentId, cx: &mut Context<Self>) {
        let mut local = false;
        if let Some(a) = self.last_snapshot.agent_mut(agent) {
            local = true;
            if a.status != muxlane_core::model::AgentStatus::Working {
                a.status = muxlane_core::model::AgentStatus::Working;
                a.status_since = muxlane_core::model::now_secs();
                cx.notify();
            }
        } else {
            // 远程没有 mark_working RPC，先更新本地镜像，避免输入后仍显示 Idle。
            for snapshot in self.remote_snaps.values_mut() {
                if let Some(a) = snapshot.agent_mut(agent) {
                    a.status = muxlane_core::model::AgentStatus::Working;
                    a.status_since = muxlane_core::model::now_secs();
                    cx.notify();
                    break;
                }
            }
        }
        // 与屏幕采样同走 DetectionEngine：既避免与引擎内部状态互斥（否则引擎
        // 推导出的候选 idle 会因等于陈旧内部状态而永不提交，spinner 卡死），
        // 又保证 Idle 状态下输入命令立即显示 working 反馈。
        let agent_id = agent.clone();
        if local {
            let server = Arc::clone(&self.server);
            server.rt_spawn({
                let server = Arc::clone(&server);
                async move { server.mark_working(&agent_id).await }
            });
        }
    }

    fn create_local_term(
        agent: AgentId,
        session: Arc<muxlane_term::PtySession>,
        font_family: &str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Entity<TermView> {
        let font_family = font_family.to_string();
        let term = cx.new(|cx| TermView::new_local(agent.clone(), session, font_family, theme, cx));
        cx.subscribe(
            &term,
            |this, _term, ev: &crate::term_view::TermEnterEvent, cx| {
                this.mark_agent_working(&ev.0, cx);
            },
        )
        .detach();
        term
    }

    fn create_remote_term(
        agent: AgentId,
        vterm: VTerm,
        remote_input: tokio::sync::mpsc::UnboundedSender<crate::term_view::RemoteTermCommand>,
        font_family: &str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Entity<TermView> {
        let font_family = font_family.to_string();
        let term = cx.new(|cx| {
            TermView::new_remote(agent.clone(), vterm, remote_input, font_family, theme, cx)
        });
        cx.subscribe(
            &term,
            |this, _term, ev: &crate::term_view::TermEnterEvent, cx| {
                this.mark_agent_working(&ev.0, cx);
            },
        )
        .detach();
        term
    }

    fn open_agent(&mut self, agent: &AgentId, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.activate_tab(&pane, agent);
            cx.notify();
            return;
        }
        if !self.terms.contains_key(agent) {
            let Some(sess) = self.server.try_session(agent) else {
                return;
            };
            let term = Self::create_local_term(
                agent.clone(),
                sess,
                &self.font_family,
                Theme::for_mode(self.theme_mode),
                cx,
            );
            self.terms.insert(agent.clone(), term);
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
            let term = Self::create_remote_term(
                agent.clone(),
                vterm.clone(),
                command_tx,
                &self.font_family,
                Theme::for_mode(self.theme_mode),
                cx,
            );
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
                                muxlane_client::send_term_input(&endpoint, &command_agent, &input)
                                    .await
                            {
                                command_vterm.feed(
                                    format!("\r\n\x1b[31mremote input failed: {error}\x1b[0m\r\n")
                                        .as_bytes(),
                                );
                            }
                        }
                        if let Some((cols, rows)) = resize {
                            let _ =
                                muxlane_client::resize_term(&endpoint, &command_agent, cols, rows)
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
                            muxlane_client::stream_term(
                                &sock,
                                &agent2,
                                move |update| match update {
                                    muxlane_client::TermUpdate::Resync(bytes) => {
                                        vterm3.feed(b"\x1bc");
                                        vterm3.feed(&bytes);
                                    }
                                    muxlane_client::TermUpdate::Data(bytes) => vterm3.feed(&bytes),
                                },
                            )
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

    fn focus_agent(&mut self, agent: &AgentId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(term) = self.terms.get(agent) {
            term.focus_handle(cx).focus(window, cx);
        }
        // 清理当前 agent 的 Toast 与标记通知已读
        self.toasts.retain(|t| &t.agent != agent);
        for n in self.notifications.iter_mut().filter(|n| &n.agent == agent) {
            n.unread = false;
        }
        if let Some(a) = self.last_snapshot.agent_mut(agent) {
            a.seen = true;
            if a.status == muxlane_core::model::AgentStatus::Done {
                a.status = muxlane_core::model::AgentStatus::Idle;
            }
        } else {
            // 远端没有 mark_seen RPC，先同步本地镜像，避免点击后仍持续闪烁。
            for snapshot in self.remote_snaps.values_mut() {
                if let Some(a) = snapshot.agent_mut(agent) {
                    a.seen = true;
                    if a.status == muxlane_core::model::AgentStatus::Done {
                        a.status = muxlane_core::model::AgentStatus::Idle;
                    }
                    break;
                }
            }
        }
        if self.last_snapshot.agent(agent).is_some() {
            let server = Arc::clone(&self.server);
            let agent = agent.clone();
            server.rt_spawn({
                let server = Arc::clone(&server);
                async move { server.mark_seen(&agent).await }
            });
        }
        cx.notify();
    }

    fn jump_to_agent(&mut self, agent: &AgentId, window: &mut Window, cx: &mut Context<Self>) {
        let is_remote = self
            .remote_snaps
            .values()
            .any(|snap| snap.agent(agent).is_some());
        if is_remote {
            self.open_remote_agent(agent, cx);
        } else {
            self.open_agent(agent, cx);
        }
        self.focus_agent(agent, window, cx);
    }

    fn select_tab_n(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.group(&self.active_pane) {
            if let Some(agent) = group.tabs.get(index).cloned() {
                let pane = self.active_pane.clone();
                self.activate_tab(&pane, &agent);
                self.focus_agent(&agent, window, cx);
                cx.notify();
            }
        }
    }

    fn next_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.group(&self.active_pane) {
            if group.tabs.is_empty() {
                return;
            }
            let cur = group
                .active
                .as_ref()
                .and_then(|a| group.tabs.iter().position(|t| t == a))
                .unwrap_or(0);
            let next = (cur + 1) % group.tabs.len();
            if let Some(agent) = group.tabs.get(next).cloned() {
                let pane = self.active_pane.clone();
                self.activate_tab(&pane, &agent);
                self.focus_agent(&agent, window, cx);
                cx.notify();
            }
        }
    }

    fn prev_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.group(&self.active_pane) {
            if group.tabs.is_empty() {
                return;
            }
            let cur = group
                .active
                .as_ref()
                .and_then(|a| group.tabs.iter().position(|t| t == a))
                .unwrap_or(0);
            let prev = if cur == 0 {
                group.tabs.len().saturating_sub(1)
            } else {
                cur - 1
            };
            if let Some(agent) = group.tabs.get(prev).cloned() {
                let pane = self.active_pane.clone();
                self.activate_tab(&pane, &agent);
                self.focus_agent(&agent, window, cx);
                cx.notify();
            }
        }
    }

    fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        self.theme_mode = if self.theme_mode.is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        self.apply_theme_to_inputs(cx);
        self.persist();
        cx.notify();
    }

    fn apply_theme_to_inputs(&mut self, cx: &mut Context<Self>) {
        let mode = self.theme_mode;
        self.palette_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_username
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_password
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_key_path
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.project_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.remote_project_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        let theme = Theme::for_mode(self.theme_mode);
        for term in self.terms.values() {
            term.update(cx, |term, cx| term.set_theme(theme, cx));
        }
    }

    fn close_tab(
        &mut self,
        pane: &PaneId,
        agent: &AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let clears_zoom = self.maximized_pane.as_ref() == Some(pane)
            && self
                .pane_tree
                .group(pane)
                .and_then(|group| group.active.as_ref())
                == Some(agent);
        let remote = self.remote_snaps.values().any(|snapshot| {
            snapshot
                .agents
                .iter()
                .any(|candidate| &candidate.id == agent)
        });
        self.delete_session(agent, remote, window, cx);
        if clears_zoom {
            // 关闭 zoom owner 的选中会话不能把 zoom 转移给下一个会话。
            self.maximized_pane = None;
        }
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
        preset: &muxlane_core::AgentPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.new_session_target.take();
        if let Some(NewSessionTarget::Remote { host, project }) = target.clone() {
            self.palette_open = false;
            self.spawn_remote_agent(host, project, Some(preset.clone()), window, cx);
            return;
        }
        let target_local_id = match target {
            Some(NewSessionTarget::Local(id)) => Some(id),
            _ => None,
        };
        let project = target_local_id
            .as_ref()
            .and_then(|id| self.last_snapshot.project(id))
            .cloned()
            .or_else(|| {
                self.active
                    .as_ref()
                    .and_then(|id| self.last_snapshot.agent(id))
                    .and_then(|agent| self.last_snapshot.project(&agent.project))
                    .cloned()
            })
            .or_else(|| self.last_snapshot.projects.first().cloned());
        let Some(project) = project else { return };
        let params = muxlane_core::protocol::AgentSpawnParams {
            project: project.id.clone(),
            agent_type: Some(preset.agent_type),
            program: (preset.agent_type != muxlane_core::model::AgentType::Shell).then(|| {
                preset
                    .executable_in(&project.path)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| preset.program.clone())
            }),
            args: Some(preset.args.clone()),
            env: Some(preset.env.clone().into_iter().collect()),
            preset_name: Some(preset.label.clone()),
        };
        let server = Arc::clone(&self.server);
        let pane = self.active_pane.clone();
        let project_key = format!("local:{}", project.id);
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let agent = server.spawn_agent(params).await?;
                    let session = server
                        .session(&agent.id)
                        .await
                        .ok_or_else(|| anyhow::anyhow!("spawned agent has no session"))?;
                    Ok::<_, anyhow::Error>((agent, session))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok((agent, session)) => {
                    let agent_id = agent.id.clone();
                    let term = Self::create_local_term(
                        agent_id.clone(),
                        session,
                        &this.font_family,
                        Theme::for_mode(this.theme_mode),
                        cx,
                    );
                    this.collapsed_projects.remove(&project_key);
                    this.terms.insert(agent_id.clone(), term);
                    this.pane_tree.open_tab(&pane, agent_id.clone());
                    this.activate_tab(&pane, &agent_id);
                    this.focus_agent(&agent_id, window, cx);
                    this.palette_open = false;
                    this.new_session_target = None;
                    this.persist();
                    cx.notify();
                }
                Err(error) => {
                    this.error_toast =
                        Some((format!("创建会话失败：{error}"), std::time::Instant::now()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn spawn_shell_for_pane(
        &mut self,
        pane: &PaneId,
        split_axis: Option<SplitAxis>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = self
            .pane_tree
            .group(pane)
            .and_then(|group| group.active.clone().or_else(|| group.tabs.first().cloned()))
            .and_then(|id| self.last_snapshot.agent(&id))
            .and_then(|agent| self.last_snapshot.project(&agent.project))
            .cloned()
            .or_else(|| self.last_snapshot.projects.first().cloned());
        let Some(project) = project else { return };
        let params = muxlane_core::protocol::AgentSpawnParams {
            project: project.id.clone(),
            agent_type: Some(muxlane_core::model::AgentType::Shell),
            program: None,
            args: None,
            env: None,
            preset_name: None,
        };
        let server = Arc::clone(&self.server);
        let pane = pane.clone();
        let project_key = format!("local:{}", project.id);
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let agent = server.spawn_agent(params).await?;
                    let session = server
                        .session(&agent.id)
                        .await
                        .ok_or_else(|| anyhow::anyhow!("spawned agent has no session"))?;
                    Ok::<_, anyhow::Error>((agent, session))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok((agent, session)) => {
                    let agent_id = agent.id.clone();
                    let term = Self::create_local_term(
                        agent_id.clone(),
                        session,
                        &this.font_family,
                        Theme::for_mode(this.theme_mode),
                        cx,
                    );
                    this.collapsed_projects.remove(&project_key);
                    this.terms.insert(agent_id.clone(), term);
                    if let Some(axis) = split_axis {
                        if let Some(new_pane) = this.pane_tree.split(&pane, axis, agent_id.clone())
                        {
                            this.active_pane = new_pane;
                            this.active = Some(agent_id.clone());
                            this.maximized_pane = None;
                        }
                    } else {
                        this.pane_tree.open_tab(&pane, agent_id.clone());
                        this.activate_tab(&pane, &agent_id);
                    }
                    this.focus_agent(&agent_id, window, cx);
                    this.persist();
                    cx.notify();
                }
                Err(error) => {
                    this.error_toast = Some((
                        format!("创建 Shell 失败：{error}"),
                        std::time::Instant::now(),
                    ));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn new_shell_tab(&mut self, pane: &PaneId, window: &mut Window, cx: &mut Context<Self>) {
        self.spawn_shell_for_pane(pane, None, window, cx);
    }

    /// 显式分屏：新 pane 始终启动普通 Shell，不复制当前 agent 类型。
    fn split_pane(
        &mut self,
        pane: &PaneId,
        axis: SplitAxis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.spawn_shell_for_pane(pane, Some(axis), window, cx);
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
        let parsed = muxlane_client::parse_target(&target);
        let name = match &parsed {
            muxlane_client::Target::Socket(path) => {
                path.rsplit('/').next().unwrap_or(path).to_string()
            }
            muxlane_client::Target::Ssh { host, .. } => host.clone(),
        };
        if let Some(index) = self.remotes.iter().position(|host| host.cfg.name == name) {
            self.remotes[index].stop();
            let release_name = name.clone();
            self.server.rt_spawn(async move {
                muxlane_client::release_remote_tunnel(&release_name).await;
            });
            self.remotes.remove(index);
            self.remote_snaps.remove(&name);
            self.remote_states.remove(&name);
        }
        let username = self.connect_username.read(cx).text();
        let auth = match self.connect_auth_mode {
            ConnectAuthMode::SshConfig => muxlane_client::SshAuth::SshConfig,
            ConnectAuthMode::PublicKey => muxlane_client::SshAuth::PublicKey {
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
                muxlane_client::SshAuth::Password {
                    username: username.trim().to_string(),
                    password,
                }
            }
        };
        let host = muxlane_client::RemoteHost::new(
            muxlane_client::HostCfg {
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
        let server = Arc::clone(&self.server);
        let params = muxlane_core::protocol::ProjectAddParams {
            path: path.display().to_string(),
            name: None,
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    server.add_project(params).await?;
                    Ok::<_, anyhow::Error>(server.snapshot().await)
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(snapshot) => {
                    this.last_snapshot = snapshot;
                    this.project_dialog = false;
                    this.dialog_error = None;
                    this.project_input.update(cx, |input, cx| input.reset(cx));
                    this.persist();
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

    fn delete_session(
        &mut self,
        agent: &AgentId,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                    .find(|host| host.cfg.name == host_name)
                    .cloned()
                {
                    if let Some(socket) = host.endpoint_now() {
                        let id = agent.clone();
                        self.server.rt_spawn(async move {
                            let _ = muxlane_client::delete_agent(&socket, &id).await;
                        });
                    }
                }
                if let Some(snapshot) = self.remote_snaps.get_mut(&host_name) {
                    snapshot.agents.retain(|candidate| &candidate.id != agent);
                    for project in &mut snapshot.projects {
                        project.agents.retain(|candidate| candidate != agent);
                    }
                }
            }
            self.finish_delete_session(agent, window, cx);
            return;
        }

        let server = Arc::clone(&self.server);
        let agent = agent.clone();
        cx.spawn_in(window, async move |this, cx| {
            let agent_for_delete = agent.clone();
            let (result, snapshot) = cx
                .background_spawn(async move {
                    let result = server.delete_agent(&agent_for_delete).await;
                    let snapshot = server.snapshot().await;
                    (result, snapshot)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.last_snapshot = snapshot;
                match result {
                    Ok(result) if result.failed_agents.is_empty() => {
                        this.finish_delete_session(&agent, window, cx);
                    }
                    Ok(result) => {
                        this.error_toast = Some((
                            format!(
                                "{} 个 tmux 会话未能销毁，会话仍保留",
                                result.failed_agents.len()
                            ),
                            std::time::Instant::now(),
                        ));
                        cx.notify();
                    }
                    Err(error) => {
                        if this.last_snapshot.agent(&agent).is_none() {
                            this.finish_delete_session(&agent, window, cx);
                        }
                        this.error_toast =
                            Some((format!("删除会话失败：{error}"), std::time::Instant::now()));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn finish_delete_session(
        &mut self,
        agent: &AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.pane_tree.close_tab(&pane, agent);
        }
        self.terms.remove(agent);
        if let Some(cancelled) = self.mirror_cancel.remove(agent) {
            cancelled.store(true, std::sync::atomic::Ordering::Release);
        }
        self.notifications.retain(|item| &item.agent != agent);
        if self.active.as_ref() == Some(agent) {
            self.active = self
                .pane_tree
                .group(&self.active_pane)
                .and_then(|group| group.active.clone());
        }
        self.session_menu = None;
        if let Some(active) = self.active.clone() {
            self.focus_agent(&active, window, cx);
        }
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
        let clear_maximized = self
            .maximized_pane
            .as_ref()
            .and_then(|pane| self.pane_tree.group(pane))
            .map(|group| group.active.is_none())
            .unwrap_or(true);
        if clear_maximized {
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
        if self.delete_busy {
            return;
        }
        let Some(confirm) = self.delete_confirm.clone() else {
            return;
        };
        self.delete_busy = true;
        match confirm.target {
            DeleteTarget::LocalProject { project, .. } => {
                let server = Arc::clone(&self.server);
                let project_for_delete = project.clone();
                let affected_agents: Vec<_> = self
                    .last_snapshot
                    .agents
                    .iter()
                    .filter(|agent| agent.project == project)
                    .map(|agent| agent.id.clone())
                    .collect();
                cx.spawn(async move |this, cx| {
                    let (result, snapshot) = cx
                        .background_spawn(async move {
                            let result = server.delete_project(&project_for_delete).await;
                            let snapshot = server.snapshot().await;
                            (result, snapshot)
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.last_snapshot = snapshot;
                        match result {
                            Ok(result) => {
                                this.cleanup_removed_agents(&result.destroyed_agents);
                                if result.failed_agents.is_empty() {
                                    this.delete_confirm = None;
                                } else {
                                    this.delete_error = Some(format!(
                                        "{} 个 tmux 会话未能销毁，项目仍保留",
                                        result.failed_agents.len()
                                    ));
                                }
                            }
                            Err(error) => {
                                let removed: Vec<_> = affected_agents
                                    .iter()
                                    .filter(|agent| this.last_snapshot.agent(agent).is_none())
                                    .cloned()
                                    .collect();
                                this.cleanup_removed_agents(&removed);
                                this.delete_error = Some(error.to_string());
                            }
                        }
                        this.delete_busy = false;
                        this.persist();
                        cx.notify();
                    });
                })
                .detach();
            }
            DeleteTarget::RemoteProject { host, project, .. } => {
                let endpoint = self
                    .remotes
                    .iter()
                    .find(|remote| remote.cfg.name == host)
                    .and_then(|remote| remote.endpoint_now());
                let Some(endpoint) = endpoint else {
                    self.delete_error = Some("远端当前不可连接，未执行删除".into());
                    self.delete_busy = false;
                    cx.notify();
                    return;
                };
                let project_for_rpc = project.clone();
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_spawn(async move {
                            muxlane_client::delete_project(&endpoint, &project_for_rpc).await
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| match result {
                        Ok(result) => {
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
                            this.delete_busy = false;
                            this.persist();
                            cx.notify();
                        }
                        Err(error) => {
                            this.delete_error = Some(error.to_string());
                            this.delete_busy = false;
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
                    muxlane_client::release_remote_tunnel(&release_host).await;
                });
                self.remotes.retain(|remote| remote.cfg.name != host);
                self.remote_snaps.remove(&host);
                self.remote_states.remove(&host);
                self.cleanup_removed_agents(&removed_agents);
                self.delete_confirm = None;
                self.delete_busy = false;
                self.persist();
                cx.notify();
            }
        }
    }

    fn render_session_menu(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
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
            .bg(rgba(theme.bg1))
            .border_1()
            .border_color(rgba(theme.line))
            .rounded_md()
            .shadow_lg()
            .child(
                div()
                    .id("session-delete")
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(rgba(theme.red))
                    .hover(|s| s.bg(rgba(theme.bg2)))
                    .on_click(cx.listener({
                        let id = menu.agent.clone();
                        let remote = menu.remote;
                        move |this, _ev, window, cx| this.delete_session(&id, remote, window, cx)
                    }))
                    .child(if menu.remote {
                        "删除远程会话"
                    } else {
                        "删除会话"
                    }),
            )
            .into_any_element()
    }

    fn render_tree_menu(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(menu) = self.tree_menu.clone() else {
            return div().into_any_element();
        };
        let menu_el = match &menu.target {
            DeleteTarget::RemoteMachine { host } => {
                let host_name = host.clone();
                let host_name_2 = host.clone();
                let host_name_3 = host.clone();
                let host_obj = self.remotes.iter().find(|r| r.cfg.name == *host).cloned();
                div()
                    .w(px(200.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .id("tree-reconnect")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg0))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                if let Some(h) = &host_obj {
                                    h.reconnect();
                                    this.focus.focus(window, cx);
                                }
                                this.tree_menu = None;
                                cx.notify();
                            }))
                            .child("重新连接"),
                    )
                    .child(
                        div()
                            .id("tree-add-project")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg0))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                this.tree_menu = None;
                                this.remote_project_dialog = Some(host_name.clone());
                                this.dialog_error = None;
                                this.remote_project_input
                                    .update(cx, |input, cx| input.reset(cx));
                                this.remote_project_input.focus_handle(cx).focus(window, cx);
                                cx.notify();
                            }))
                            .child("添加远程项目…"),
                    )
                    .child(
                        div()
                            .id("tree-upgrade-muxlane")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.accent))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.tree_menu = None;
                                this.bootstrap_error = None;
                                this.bootstrap_confirm = Some(BootstrapConfirm {
                                    host: host_name_2.clone(),
                                    install: false,
                                    upgrade: true,
                                    binary: None,
                                });
                                cx.notify();
                            }))
                            .child("更新远端 Muxlane…"),
                    )
                    .child(
                        div()
                            .id("tree-reinstall-muxlane")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg1))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.tree_menu = None;
                                this.bootstrap_error = None;
                                this.bootstrap_confirm = Some(BootstrapConfirm {
                                    host: host_name_3.clone(),
                                    install: true,
                                    upgrade: false,
                                    binary: None,
                                });
                                cx.notify();
                            }))
                            .child("重新部署 / 安装远端 Muxlane…"),
                    )
                    .child(div().h(px(1.)).bg(rgba(theme.line)).my_1())
                    .child(
                        div()
                            .id("tree-delete")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.red))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener({
                                let target = menu.target.clone();
                                move |this, _event, _window, cx| {
                                    this.tree_menu = None;
                                    this.begin_delete(target.clone(), cx);
                                }
                            }))
                            .child("删除远程机器…"),
                    )
            }
            DeleteTarget::LocalProject { .. } | DeleteTarget::RemoteProject { .. } => {
                let label = match &menu.target {
                    DeleteTarget::LocalProject { .. } => "删除项目…",
                    DeleteTarget::RemoteProject { .. } => "删除远程项目…",
                    _ => unreachable!(),
                };
                div()
                    .w(px(190.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .id("tree-delete")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.red))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener({
                                let target = menu.target.clone();
                                move |this, _event, _window, cx| {
                                    this.tree_menu = None;
                                    this.begin_delete(target.clone(), cx);
                                }
                            }))
                            .child(label),
                    )
            }
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
            .child(menu_el)
            .into_any_element()
    }

    fn render_delete_confirm(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(confirm) = self.delete_confirm.clone() else {
            return div().into_any_element();
        };
        let (title, label, destructive_copy) = match &confirm.target {
            DeleteTarget::LocalProject { label, .. }
            | DeleteTarget::RemoteProject { label, .. } => (
                "删除项目",
                label.clone(),
                format!(
                    "将结束 {} 个 muxlane tmux 会话。项目文件和用户默认 tmux 不会删除。",
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
            .bg(rgba(theme.overlay()))
            .child(
                div()
                    .w(px(460.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child(title),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg1))
                            .child(format!("{} · {}", label, destructive_copy)),
                    )
                    .when_some(self.delete_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .px_4()
                                .pt_2()
                                .text_size(px(11.))
                                .text_color(rgba(theme.red))
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
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.delete_confirm = None;
                                        this.delete_error = None;
                                        this.delete_busy = false;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child({
                                let busy = self.delete_busy;
                                div()
                                    .id("delete-confirm-submit")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgba(theme.red))
                                    .text_color(rgba(theme.on_accent))
                                    .cursor_pointer()
                                    .when(!busy, |el| {
                                        el.hover(|style| {
                                            style.bg(rgba(Theme::with_alpha(theme.red, 0xcc)))
                                        })
                                        .active(|style| {
                                            style.bg(rgba(Theme::with_alpha(theme.red, 0x99)))
                                        })
                                    })
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        if !this.delete_busy {
                                            this.confirm_delete(cx);
                                        }
                                    }))
                                    .child(if busy { "删除中…" } else { "确认删除" })
                            }),
                    ),
            )
            .into_any_element()
    }

    fn cancel_bootstrap_for_host(&mut self, host: &str, cx: &mut Context<Self>) {
        if let Some(remote) = self.remotes.iter().find(|r| r.cfg.name == host) {
            remote.cancel_bootstrap();
        }
        self.bootstrap_progress.remove(host);
        if self.bootstrap_confirm.as_ref().map(|c| c.host.as_str()) == Some(host) {
            self.bootstrap_confirm = None;
            self.bootstrap_error = None;
        }
        cx.notify();
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
        let host_name = confirm.host.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if confirm.upgrade {
                        remote.upgrade_and_retry().await
                    } else if confirm.install {
                        remote.install_and_start().await
                    } else {
                        remote
                            .start_and_retry(confirm.binary.as_deref().unwrap_or("muxlane"))
                            .await
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.bootstrap_progress.remove(&host_name);
                match result {
                    Ok(()) => {
                        this.bootstrap_confirm = None;
                        this.bootstrap_error = None;
                        cx.notify();
                    }
                    Err(error) => {
                        let error = error.to_string();
                        if error.contains("已取消") {
                            this.bootstrap_confirm = None;
                            this.bootstrap_error = None;
                        } else {
                            this.bootstrap_error = Some(error);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn render_bootstrap_confirm(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
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
            .bg(rgba(theme.overlay()))
            .child(
                div()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child(format!("{}远端 Muxlane", action)),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg1))
                            .child(format!(
                                "SSH 已连接到 {}。将使用当前认证方式{} headless 进程。",
                                confirm.host,
                                if confirm.upgrade {
                                    "上传新版本并重启"
                                } else if confirm.install {
                                    "上传当前 Muxlane 并启动"
                                } else {
                                    "启动"
                                }
                            )),
                    )
                    .when_some(self.bootstrap_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .px_4()
                                .pt_2()
                                .text_size(px(11.))
                                .text_color(rgba(theme.red))
                                .child(error),
                        )
                    })
                    .when_some(
                        self.bootstrap_progress.get(&confirm.host).cloned(),
                        |dialog, progress| {
                            let overall = progress.phase.overall(progress.percent);
                            let phase_text = format_upload_phase(&progress);
                            dialog
                                .child(
                                    div()
                                        .px_4()
                                        .pt_3()
                                        .text_size(px(11.))
                                        .text_color(rgba(theme.accent))
                                        .child(phase_text),
                                )
                                .child(
                                    div().mx_4().mt_2().h(px(4.)).bg(rgba(theme.bg2)).child(
                                        div()
                                            .w(relative(overall as f32 / 100.0))
                                            .h_full()
                                            .bg(rgba(theme.accent)),
                                    ),
                                )
                        },
                    )
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
                                    .text_color(rgba(theme.fg0))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener({
                                        let host = confirm.host.clone();
                                        move |this, _event, _window, cx| {
                                            this.cancel_bootstrap_for_host(&host, cx);
                                        }
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("bootstrap-submit")
                                    .px_3()
                                    .py_1()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.confirm_bootstrap(cx)
                                    }))
                                    .child(action),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn open_connect_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.connect_dialog = true;
        self.connect_focus_index = 0;
        self.project_dialog = false;
        self.dialog_error = None;
        self.connect_input.update(cx, |input, cx| input.reset(cx));
        self.connect_username
            .update(cx, |input, cx| input.reset(cx));
        self.connect_password
            .update(cx, |input, cx| input.reset(cx));
        self.connect_key_path
            .update(cx, |input, cx| input.reset(cx));
        self.connect_input.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn dismiss_settings_menus(&mut self) {
        self.settings_theme_menu = false;
        self.settings_font_menu = false;
        self.settings_language_menu = false;
    }

    fn set_theme(&mut self, mode: ThemeMode, cx: &mut Context<Self>) {
        self.theme_mode = mode;
        self.dismiss_settings_menus();
        self.apply_theme_to_inputs(cx);
        self.persist();
        cx.notify();
    }

    fn set_font_family(&mut self, font_family: &str, cx: &mut Context<Self>) {
        self.font_family = font_family.to_string();
        self.dismiss_settings_menus();
        let family = self.font_family.clone();
        let theme = Theme::for_mode(self.theme_mode);
        for term in self.terms.values() {
            let family = family.clone();
            term.update(cx, |term, cx| {
                term.set_font_family(family, cx);
                term.set_theme(theme, cx);
            });
        }
        self.persist();
        cx.notify();
    }

    fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        self.language = language;
        self.dismiss_settings_menus();
        self.persist();
        cx.notify();
    }

    fn render_notifications_popover(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let unread_count = self.notifications.iter().filter(|n| n.unread).count();

        div()
            .id("notifications-backdrop")
            .absolute()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.notifications_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("notifications-popover")
                    .occlude()
                    .absolute()
                    .bottom(px(40.))
                    .left(px(8.))
                    .w(px(320.))
                    .max_h(px(420.))
                    .flex()
                    .flex_col()
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_xl()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _event, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        div()
                            .h(px(34.))
                            .px_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(panel_icon(NOTIFICATION_ICON, theme.fg1))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgba(theme.fg0))
                                            .child(i18n::text(
                                                self.language,
                                                "通知中心",
                                                "Notifications",
                                            )),
                                    )
                                    .when(unread_count > 0, |header| {
                                        header.child(
                                            div()
                                                .px_1p5()
                                                .py(px(1.))
                                                .rounded_full()
                                                .bg(rgba(theme.accent))
                                                .text_color(rgba(theme.on_accent))
                                                .text_size(px(9.))
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .child(format!("{unread_count}")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .when(!self.notifications.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .id("clear-notifications")
                                                .cursor_pointer()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_xs()
                                                .text_size(px(10.))
                                                .text_color(rgba(theme.fg2))
                                                .hover(|s| {
                                                    s.bg(rgba(theme.bg2))
                                                        .text_color(rgba(theme.fg0))
                                                })
                                                .on_click(cx.listener(|this, _ev, _window, cx| {
                                                    this.notifications.clear();
                                                    cx.notify();
                                                }))
                                                .child(i18n::text(self.language, "清空", "Clear")),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("close-notifications")
                                            .cursor_pointer()
                                            .px_1()
                                            .text_size(px(14.))
                                            .text_color(rgba(theme.fg2))
                                            .hover(|s| s.text_color(rgba(theme.fg0)))
                                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                                this.notifications_open = false;
                                                cx.notify();
                                            }))
                                            .child("×"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("notifications-scroll")
                            .flex_1()
                            .overflow_y_scroll()
                            .when(self.notifications.is_empty(), |list| {
                                list.child(
                                    div()
                                        .py_8()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(rgba(theme.fg2))
                                                .child(i18n::text(
                                                    self.language,
                                                    "暂无通知",
                                                    "No notifications",
                                                )),
                                        ),
                                )
                            })
                            .children(self.notifications.iter().enumerate().map(|(idx, n)| {
                                let dot_color = match n.to {
                                    muxlane_core::model::AgentStatus::Blocked => theme.yellow,
                                    muxlane_core::model::AgentStatus::Done => theme.green,
                                    muxlane_core::model::AgentStatus::Working => theme.accent,
                                    _ => theme.fg2,
                                };
                                let status_text = match n.to {
                                    muxlane_core::model::AgentStatus::Blocked => {
                                        i18n::text(self.language, "等待输入", "Input required")
                                    }
                                    muxlane_core::model::AgentStatus::Done => {
                                        i18n::text(self.language, "任务完成", "Task completed")
                                    }
                                    muxlane_core::model::AgentStatus::Working => {
                                        i18n::text(self.language, "执行中", "Working")
                                    }
                                    _ => i18n::text(self.language, "空闲", "Idle"),
                                };
                                let agent_id = n.agent.clone();
                                let is_unread = n.unread;
                                let time_str = format_relative_time(n.time_secs, self.language);

                                div()
                                    .id(gpui::ElementId::Name(
                                        format!("notif-popover-item-{idx}").into(),
                                    ))
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .px_3()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(rgba(theme.line))
                                    .when(is_unread, |el| {
                                        el.bg(rgba(Theme::with_alpha(theme.accent, 0x0f)))
                                    })
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .cursor_pointer()
                                    .on_click(cx.listener({
                                        let agent_id = agent_id.clone();
                                        move |this, _ev, window, cx| {
                                            this.notifications_open = false;
                                            this.jump_to_agent(&agent_id, window, cx);
                                        }
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .child(
                                                        div()
                                                            .w(px(6.))
                                                            .h(px(6.))
                                                            .rounded_full()
                                                            .bg(rgba(dot_color)),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                                            .text_color(rgba(theme.fg0))
                                                            .child(format!(
                                                                "{} · {}",
                                                                n.machine_name, n.project_name
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(10.))
                                                            .text_color(rgba(dot_color))
                                                            .child(status_text),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(10.))
                                                    .text_color(rgba(theme.fg2))
                                                    .child(time_str),
                                            ),
                                    )
                                    .when_some(n.message.clone(), |row, msg| {
                                        row.child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(rgba(if is_unread {
                                                    theme.fg0
                                                } else {
                                                    theme.fg1
                                                }))
                                                .child(truncate(&msg, 90)),
                                        )
                                    })
                            })),
                    ),
            )
            .into_any_element()
    }
    fn render_settings(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(48.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.settings_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("settings-page")
                    .relative()
                    .occlude()
                    .w(px(560.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _event, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_key_down(
                        cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                            if event.keystroke.key.as_str() == "escape" {
                                this.settings_open = false;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgba(theme.fg0))
                                    .child(i18n::text(self.language, "设置", "Settings")),
                            )
                            .child(
                                div()
                                    .id("settings-close")
                                    .w(px(24.))
                                    .h(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .text_color(rgba(theme.fg1))
                                    .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.fg0)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.settings_open = false;
                                        cx.notify();
                                    }))
                                    .child("×"),
                            ),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg2))
                            .child(i18n::text(self.language, "主题", "Theme")),
                    )
                    .child(
                        div()
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div().text_size(px(12.)).text_color(rgba(theme.fg0)).child(
                                    i18n::text(self.language, "界面主题", "Interface theme"),
                                ),
                            )
                            .child({
                                let selected = Theme::for_mode(self.theme_mode);
                                let language = self.language;
                                let current_mode = self.theme_mode;
                                div()
                                    .relative()
                                    .child(
                                        div()
                                            .id("settings-theme-select")
                                            .w(px(210.))
                                            .h(px(32.))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .border_1()
                                            .border_color(rgba(theme.line))
                                            .bg(rgba(theme.bg0))
                                            .hover(|s| s.bg(rgba(theme.bg2)))
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                let open = !this.settings_theme_menu;
                                                this.dismiss_settings_menus();
                                                this.settings_theme_menu = open;
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .w(px(24.))
                                                    .h(px(16.))
                                                    .bg(rgba(selected.bg0))
                                                    .border_1()
                                                    .border_color(rgba(selected.accent)),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(11.))
                                                    .text_color(rgba(theme.fg0))
                                                    .child(if self.language == Language::English {
                                                        self.theme_mode.label_en()
                                                    } else {
                                                        self.theme_mode.label()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(rgba(theme.fg1))
                                                    .child(if self.settings_theme_menu {
                                                        "⌃"
                                                    } else {
                                                        "⌄"
                                                    }),
                                            ),
                                    )
                                    .when(self.settings_theme_menu, |anchor| {
                                        anchor.child(
                                            deferred(
                                                div()
                                                    .id("settings-theme-menu")
                                                    .absolute()
                                                    .top_full()
                                                    .left_0()
                                                    .w(px(210.))
                                                    .max_h(px(280.))
                                                    .overflow_y_scroll()
                                                    .bg(rgba(theme.bg1))
                                                    .border_1()
                                                    .border_color(rgba(theme.line))
                                                    .shadow_lg()
                                                    .occlude()
                                                    .children(ThemeMode::ALL.into_iter().map(
                                                        |mode| {
                                                            let selected = mode == current_mode;
                                                            let swatch = Theme::for_mode(mode);
                                                            div()
                                                        .id(gpui::ElementId::Name(
                                                            format!(
                                                                "settings-theme-option-{}",
                                                                mode.id()
                                                            )
                                                            .into(),
                                                        ))
                                                        .h(px(30.))
                                                        .px_2()
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .when(selected, |el| el.bg(rgba(theme.bg2)))
                                                        .when(!selected, |el| {
                                                            el.hover(|s| s.bg(rgba(theme.bg2)))
                                                        })
                                                        .on_click(cx.listener(
                                                            move |this, _event, _window, cx| {
                                                                this.set_theme(mode, cx);
                                                            },
                                                        ))
                                                        .child(
                                                            div()
                                                                .w(px(22.))
                                                                .h(px(14.))
                                                                .bg(rgba(swatch.bg0))
                                                                .border_1()
                                                                .border_color(rgba(swatch.accent)),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .text_size(px(11.))
                                                                .text_color(rgba(theme.fg0))
                                                                .child(
                                                                    if language == Language::English
                                                                    {
                                                                        mode.label_en()
                                                                    } else {
                                                                        mode.label()
                                                                    },
                                                                ),
                                                        )
                                                        .child(if selected { "✓" } else { "" })
                                                        },
                                                    )),
                                            )
                                            .with_priority(1),
                                        )
                                    })
                            }),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg2))
                            .child(i18n::text(self.language, "终端字体", "Terminal font")),
                    )
                    .child(div().px_4().pb_4().flex().justify_end().child({
                        let current_font = self.font_family.clone();
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("settings-font-select")
                                    .w(px(260.))
                                    .h(px(32.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_1()
                                    .border_color(rgba(theme.line))
                                    .bg(rgba(theme.bg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        let open = !this.settings_font_menu;
                                        this.dismiss_settings_menus();
                                        this.settings_font_menu = open;
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(11.))
                                            .text_color(rgba(theme.fg0))
                                            .font_family(self.font_family.clone())
                                            .child(self.font_family.clone()),
                                    )
                                    .child(
                                        div().text_size(px(12.)).text_color(rgba(theme.fg1)).child(
                                            if self.settings_font_menu {
                                                "⌃"
                                            } else {
                                                "⌄"
                                            },
                                        ),
                                    ),
                            )
                            .when(self.settings_font_menu, |anchor| {
                                anchor.child(
                                    deferred(
                                        div()
                                            .id("settings-font-menu")
                                            .absolute()
                                            .top_full()
                                            .left_0()
                                            .w(px(260.))
                                            .max_h(px(280.))
                                            .overflow_y_scroll()
                                            .bg(rgba(theme.bg1))
                                            .border_1()
                                            .border_color(rgba(theme.line))
                                            .shadow_lg()
                                            .occlude()
                                            .children(FONT_FAMILIES.iter().map(|family| {
                                                let selected = current_font == *family;
                                                let family = (*family).to_string();
                                                div()
                                                    .id(gpui::ElementId::Name(
                                                        format!(
                                                            "settings-font-option-{}",
                                                            family.replace(' ', "-")
                                                        )
                                                        .into(),
                                                    ))
                                                    .h(px(30.))
                                                    .px_2()
                                                    .flex()
                                                    .items_center()
                                                    .when(selected, |el| el.bg(rgba(theme.bg2)))
                                                    .when(!selected, |el| {
                                                        el.hover(|s| s.bg(rgba(theme.bg2)))
                                                    })
                                                    .on_click(cx.listener({
                                                        let family = family.clone();
                                                        move |this, _event, _window, cx| {
                                                            this.set_font_family(&family, cx);
                                                        }
                                                    }))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .text_size(px(11.))
                                                            .text_color(rgba(theme.fg0))
                                                            .font_family(family.clone())
                                                            .child(family),
                                                    )
                                                    .child(if selected { "✓" } else { "" })
                                            })),
                                    )
                                    .with_priority(1),
                                )
                            })
                    }))
                    .child(
                        div()
                            .px_4()
                            .pb_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgba(theme.fg0))
                                    .child(i18n::text(self.language, "语言", "Language")),
                            )
                            .child({
                                let current_language = self.language;
                                div()
                                    .relative()
                                    .child(
                                        div()
                                            .id("settings-language-select")
                                            .w(px(180.))
                                            .h(px(32.))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .border_1()
                                            .border_color(rgba(theme.line))
                                            .bg(rgba(theme.bg0))
                                            .hover(|s| s.bg(rgba(theme.bg2)))
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                let open = !this.settings_language_menu;
                                                this.dismiss_settings_menus();
                                                this.settings_language_menu = open;
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(11.))
                                                    .text_color(rgba(theme.fg0))
                                                    .child(self.language.label()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(rgba(theme.fg1))
                                                    .child(if self.settings_language_menu {
                                                        "⌃"
                                                    } else {
                                                        "⌄"
                                                    }),
                                            ),
                                    )
                                    .when(self.settings_language_menu, |anchor| {
                                        anchor.child(
                                            deferred(
                                                div()
                                                    .id("settings-language-menu")
                                                    .absolute()
                                                    .top_full()
                                                    .left_0()
                                                    .w(px(180.))
                                                    .bg(rgba(theme.bg1))
                                                    .border_1()
                                                    .border_color(rgba(theme.line))
                                                    .shadow_lg()
                                                    .occlude()
                                                    .children(Language::ALL.into_iter().map(
                                                        |language| {
                                                            let selected =
                                                                language == current_language;
                                                            div()
                                                            .id(gpui::ElementId::Name(
                                                                format!(
                                                                    "settings-language-option-{}",
                                                                    language.id()
                                                                )
                                                                .into(),
                                                            ))
                                                            .h(px(30.))
                                                            .px_2()
                                                            .flex()
                                                            .items_center()
                                                            .when(selected, |el| {
                                                                el.bg(rgba(theme.bg2))
                                                            })
                                                            .when(!selected, |el| {
                                                                el.hover(|s| s.bg(rgba(theme.bg2)))
                                                            })
                                                            .on_click(cx.listener(
                                                                move |this, _event, _window, cx| {
                                                                    this.set_language(language, cx);
                                                                },
                                                            ))
                                                            .child(language.label())
                                                        },
                                                    )),
                                            )
                                            .with_priority(1),
                                        )
                                    })
                            }),
                    )
                    .child(
                        div()
                            .px_4()
                            .pb_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div().text_size(px(12.)).text_color(rgba(theme.fg0)).child(
                                    i18n::text(self.language, "通知声音", "Notification sound"),
                                ),
                            )
                            .child(
                                div()
                                    .id("settings-sound-toggle")
                                    .h(px(30.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_1()
                                    .border_color(rgba(if self.sound_enabled {
                                        theme.accent
                                    } else {
                                        theme.line
                                    }))
                                    .text_size(px(11.))
                                    .text_color(rgba(if self.sound_enabled {
                                        theme.accent
                                    } else {
                                        theme.fg2
                                    }))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.sound_enabled = !this.sound_enabled;
                                        this.persist();
                                        cx.notify();
                                    }))
                                    .child(if self.sound_enabled {
                                        i18n::text(self.language, "已开启", "Enabled")
                                    } else {
                                        i18n::text(self.language, "已关闭", "Disabled")
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_connect_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let input = self.connect_input.clone();
        let username = self.connect_username.clone();
        let password = self.connect_password.clone();
        let key_path = self.connect_key_path.clone();
        let auth_mode = self.connect_auth_mode;
        let error = self.dialog_error.clone();
        div()
            .id("connect-dialog-backdrop")
            .absolute()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(90.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, cx| {
                    this.connect_dialog = false;
                    this.dialog_error = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _ev, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
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
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child("连接远程机器"),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child("输入 SSH Host 或 ~/.ssh/config 别名；socket 自动发现"),
                    )
                    .child(div().mx_4().mt_3().child(input))
                    .child(
                        div()
                            .mx_4()
                            .mt_2()
                            .flex()
                            .border_1()
                            .border_color(rgba(theme.line))
                            .rounded_sm()
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("auth-config")
                                    .flex_1()
                                    .px_2()
                                    .py_1()
                                    .text_size(px(11.))
                                    .text_color(rgba(theme.fg1))
                                    .when(auth_mode == ConnectAuthMode::SshConfig, |item| {
                                        item.bg(rgba(theme.accent))
                                            .text_color(rgba(theme.on_accent))
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
                                    .text_color(rgba(theme.fg1))
                                    .when(auth_mode == ConnectAuthMode::PublicKey, |item| {
                                        item.bg(rgba(theme.accent))
                                            .text_color(rgba(theme.on_accent))
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
                                    .text_color(rgba(theme.fg1))
                                    .when(auth_mode == ConnectAuthMode::Password, |item| {
                                        item.bg(rgba(theme.accent))
                                            .text_color(rgba(theme.on_accent))
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
                                .text_color(rgba(theme.red))
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
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
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
                                    .rounded_sm()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        let target = this.connect_input.read(cx).text();
                                        this.add_remote_target(target, cx);
                                    }))
                                    .child("连接"),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn spawn_remote_agent(
        &mut self,
        host: String,
        project: String,
        preset: Option<muxlane_core::AgentPreset>,
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
        let target_project = project.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    muxlane_client::spawn_agent(&endpoint, &project, preset.as_ref()).await
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(agent) => {
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
                    let remote_project_key = format!("remote:{}:{}", host, target_project);
                    this.collapsed_projects.remove(&remote_project_key);
                    let remote_machine_key = format!("machine:remote:{}", host);
                    this.collapsed_machines.remove(&remote_machine_key);
                    this.open_remote_agent(&agent_id, cx);
                    this.focus_agent(&agent_id, window, cx);
                    this.persist();
                    cx.notify();
                }
                Err(error) => {
                    let text = error.to_string();
                    this.error_toast = Some((
                        format!("远程创建会话失败：{text}"),
                        std::time::Instant::now(),
                    ));
                    // 类型不匹配通常意味着远端仍在运行旧版 Muxlane，
                    // 直接切换到已有的更新引导状态。
                    if text.contains("远端 Muxlane 版本过旧") {
                        if let Some(remote) = this
                            .remotes
                            .iter()
                            .find(|remote| remote.cfg.name == host)
                            .cloned()
                        {
                            cx.spawn(async move |_, _| {
                                remote.mark_needs_upgrade().await;
                            })
                            .detach();
                        }
                    }
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
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    muxlane_client::add_project(&endpoint, path.trim()).await
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(project) => {
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
                Err(error) => {
                    let text = error.to_string();
                    if text.contains("unknown_method")
                        && text.contains(muxlane_core::protocol::features::PROJECT_ADD)
                    {
                        // 旧版远端没有 project.add：转入升级引导，而不是弹原始错误
                        this.remote_project_dialog = None;
                        this.dialog_error = None;
                        if let Some(remote) = this
                            .remotes
                            .iter()
                            .find(|remote| remote.cfg.name == host)
                            .cloned()
                        {
                            cx.spawn(async move |this, cx| {
                                remote.mark_needs_upgrade().await;
                                let _ = this.update(cx, |_, _| {});
                            })
                            .detach();
                        }
                    } else {
                        this.dialog_error = Some(text);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn render_remote_project_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(host) = self.remote_project_dialog.clone() else {
            return div().into_any_element();
        };
        let input = self.remote_project_input.clone();
        div()
            .id("remote-project-backdrop")
            .absolute()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(90.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.remote_project_dialog = None;
                    this.dialog_error = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _ev, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
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
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child(format!("在 {host} 添加项目")),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child("输入远端已存在的目录；不会上传或删除项目文件"),
                    )
                    .child(div().mx_4().mt_3().child(input))
                    .when_some(self.dialog_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .mx_4()
                                .mt_2()
                                .text_size(px(11.))
                                .text_color(rgba(theme.red))
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
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
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
                                    .rounded_sm()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        if let Some(host) = this.remote_project_dialog.clone() {
                                            let path = this.remote_project_input.read(cx).text();
                                            this.submit_remote_project(host, path, cx);
                                        }
                                    }))
                                    .child("添加"),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_project_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let input = self.project_input.clone();
        let error = self.dialog_error.clone();
        div()
            .id("project-dialog-backdrop")
            .absolute()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(90.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, cx| {
                    this.project_dialog = false;
                    this.dialog_error = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _ev, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
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
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child("添加本地项目"),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child("输入已有项目目录；远程项目由连接机器后自动发现"),
                    )
                    .child(div().mx_4().mt_3().child(input))
                    .when_some(error, |dialog, error| {
                        dialog.child(
                            div()
                                .mx_4()
                                .mt_2()
                                .text_size(px(11.))
                                .text_color(rgba(theme.red))
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
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
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
                                    .rounded_sm()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        let path = this.project_input.read(cx).text();
                                        this.add_local_project(path, cx);
                                    }))
                                    .child("添加"),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn palette_project_path(&self) -> Option<std::path::PathBuf> {
        match &self.new_session_target {
            Some(NewSessionTarget::Local(id)) => {
                self.last_snapshot.project(id).map(|p| p.path.clone())
            }
            Some(NewSessionTarget::Remote { host, project }) => self
                .remote_snaps
                .get(host)
                .and_then(|s| s.project(project))
                .map(|p| p.path.clone()),
            None => self
                .active
                .as_ref()
                .and_then(|id| self.find_agent(id))
                .and_then(|a| {
                    self.last_snapshot
                        .project(&a.project)
                        .map(|p| p.path.clone())
                        .or_else(|| {
                            for snap in self.remote_snaps.values() {
                                if let Some(p) = snap.project(&a.project) {
                                    return Some(p.path.clone());
                                }
                            }
                            None
                        })
                })
                .or_else(|| {
                    self.last_snapshot
                        .projects
                        .first()
                        .map(|project| project.path.clone())
                }),
        }
    }

    fn compute_palette_items(&self, cx: &Context<Self>) -> Vec<PaletteItem> {
        let query = self.palette_input.read(cx).text().trim().to_lowercase();
        let mut items = Vec::new();

        if let Some(target) = &self.new_session_target {
            // 新增 Agent 会话模式：仅列出预设 Agent，不混入已有会话跳转与全局操作指令
            match target {
                NewSessionTarget::Local(_) => {
                    let project_path = self.palette_project_path();
                    for preset in self.presets.clone().into_iter().filter(|p| {
                        project_path
                            .as_deref()
                            .map_or_else(|| p.installed(), |path| p.installed_in(path))
                    }) {
                        items.push(PaletteItem::Preset { preset });
                    }
                }
                NewSessionTarget::Remote { .. } => {
                    // 远端预设不做本机 PATH 过滤：program 绝对路径跨机无意义，
                    // 远端是否可用由远端 spawn 结果反馈（spawn_failed）。
                    for preset in self.presets.clone() {
                        items.push(PaletteItem::Preset { preset });
                    }
                }
            }
        } else {
            // 全局命令面板 (Ctrl+K)：预设 + 操作，不含会话列表。
            let project_path = self.palette_project_path();
            for preset in self.presets.clone().into_iter().filter(|p| {
                project_path
                    .as_deref()
                    .map_or_else(|| p.installed(), |path| p.installed_in(path))
            }) {
                items.push(PaletteItem::Preset { preset });
            }

            // 3. 操作指令
            items.push(PaletteItem::Action {
                id: "cmd-split-h",
                label: "水平分屏",
                shortcut: Some("h"),
                icon: SPLIT_HORIZONTAL_ICON,
            });
            items.push(PaletteItem::Action {
                id: "cmd-split-v",
                label: "垂直分屏",
                shortcut: Some("v"),
                icon: SPLIT_VERTICAL_ICON,
            });
            items.push(PaletteItem::Action {
                id: "cmd-max",
                label: "最大化 / 还原当前面板",
                shortcut: Some("m"),
                icon: MAXIMIZE_ICON,
            });
            if self.pane_tree.leaf_count() > 1 {
                items.push(PaletteItem::Action {
                    id: "cmd-close-pane",
                    label: "关闭当前分屏",
                    shortcut: Some("x"),
                    icon: CLOSE_ICON,
                });
            }
            items.push(PaletteItem::Action {
                id: "cmd-connect",
                label: "连接远程开发机…",
                shortcut: None,
                icon: CONNECT_ICON,
            });
            items.push(PaletteItem::Action {
                id: "cmd-toggle-theme",
                label: if self.theme_mode.is_dark() {
                    "切换为浅色模式"
                } else {
                    "切换为深色模式"
                },
                shortcut: None,
                icon: THEME_ICON,
            });
            items.push(PaletteItem::Action {
                id: "cmd-clear-notifs",
                label: "清空所有通知",
                shortcut: None,
                icon: NOTIFICATION_ICON,
            });
        }

        if query.is_empty() {
            items
        } else {
            items
                .into_iter()
                .filter(|item| match item {
                    PaletteItem::Preset { preset } => {
                        let text =
                            format!("新建 {} {}", preset.label, preset.program).to_lowercase();
                        text.contains(&query)
                    }
                    PaletteItem::Action { label, .. } => label.to_lowercase().contains(&query),
                })
                .collect()
        }
    }

    fn execute_palette_item(
        &mut self,
        item: PaletteItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        // 注意：new_session_target 由消费方（spawn_preset 的远程/本地分支）
        // 自行读取并清除，此处不能提前清空，否则远程会话会回退到本地项目。
        match item {
            PaletteItem::Preset { preset } => {
                self.spawn_preset(&preset, window, cx);
            }
            PaletteItem::Action { id, .. } => {
                self.new_session_target = None;
                match id {
                    "cmd-split-h" => {
                        let pane = self.active_pane.clone();
                        self.split_pane(&pane, SplitAxis::Horizontal, window, cx);
                    }
                    "cmd-split-v" => {
                        let pane = self.active_pane.clone();
                        self.split_pane(&pane, SplitAxis::Vertical, window, cx);
                    }
                    "cmd-max" => {
                        let pane = self.active_pane.clone();
                        self.toggle_maximize(&pane, cx);
                    }
                    "cmd-close-pane" => {
                        let pane = self.active_pane.clone();
                        self.close_split_pane(&pane, window, cx);
                    }
                    "cmd-connect" => self.open_connect_dialog(window, cx),
                    "cmd-toggle-theme" => {
                        self.toggle_theme(cx);
                    }
                    "cmd-clear-notifs" => {
                        self.notifications.clear();
                    }
                    _ => {}
                }
            }
        }
        cx.notify();
    }

    /// 返回是否消费了该按键（消费才 stop_propagation）
    fn handle_palette_key(
        &mut self,
        ks: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // 有输入框时不抢编辑键：输入框的 bubble listener 先处理编辑/自插字符；
        // 导航/确认键仍由 palette 统一处理（Enter/上下/Escape 在 TextField 中
        // 本就不消费，会冒泡到这里）。
        let items = self.compute_palette_items(cx);
        // 无查询时支持 Action 快捷键（与列表里展示的 [h]/[v]/[x]/[m] 一致）
        let query = self.palette_input.read(cx).text().trim().to_lowercase();
        if query.is_empty() {
            let pane = self.active_pane.clone();
            match ks.key.as_str() {
                "h" => {
                    self.palette_open = false;
                    self.split_pane(&pane, SplitAxis::Horizontal, window, cx);
                    return true;
                }
                "v" => {
                    self.palette_open = false;
                    self.split_pane(&pane, SplitAxis::Vertical, window, cx);
                    return true;
                }
                "x" => {
                    self.palette_open = false;
                    self.close_split_pane(&pane, window, cx);
                    return true;
                }
                "m" => {
                    self.palette_open = false;
                    self.toggle_maximize(&pane, cx);
                    return true;
                }
                _ => {}
            }
        }
        match ks.key.as_str() {
            "up" => {
                self.palette_index = self.palette_index.saturating_sub(1);
                self.palette_scroll.scroll_to_item(self.palette_index);
                cx.notify();
                true
            }
            "down" => {
                if !items.is_empty() {
                    self.palette_index = (self.palette_index + 1).min(items.len() - 1);
                    self.palette_scroll.scroll_to_item(self.palette_index);
                    cx.notify();
                }
                true
            }
            "enter" => {
                if let Some(item) = items.get(self.palette_index).cloned() {
                    self.execute_palette_item(item, window, cx);
                    return true;
                }
                false
            }
            "escape" => {
                self.palette_open = false;
                self.new_session_target = None;
                if let Some(active) = self.active.clone() {
                    self.focus_agent(&active, window, cx);
                }
                cx.notify();
                true
            }
            _ => false,
        }
    }

    fn render_palette(&mut self, theme: Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let items = self.compute_palette_items(cx);
        let current_index = self.palette_index;
        let mut list_container = div()
            .id("palette-items-scroll")
            .flex()
            .flex_col()
            .max_h(px(324.))
            .overflow_y_scroll()
            .track_scroll(&self.palette_scroll);

        if items.is_empty() {
            list_container = list_container.child(
                div()
                    .px_4()
                    .py_6()
                    .text_size(px(12.))
                    .text_color(rgba(theme.fg2))
                    .child("无匹配结果"),
            );
        } else {
            for (index, item) in items.into_iter().enumerate() {
                let is_selected = index == current_index;
                let item_for_click = item.clone();
                let row = match item {
                    PaletteItem::Preset { preset } => div()
                        .id(gpui::ElementId::Name(
                            format!("pal-preset-{}", preset.id).into(),
                        ))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(rgba(theme.fg0))
                        .when(is_selected, |el| el.bg(rgba(theme.bg2)))
                        .hover(|s| s.bg(rgba(theme.bg2)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            this.execute_palette_item(item_for_click.clone(), window, cx);
                        }))
                        .child(panel_icon(PLUS_ICON, theme.accent))
                        .child(format!("新建 {}", preset.label))
                        .child(
                            div()
                                .ml_auto()
                                .text_size(px(10.))
                                .text_color(rgba(theme.fg2))
                                .child(preset.program),
                        ),
                    PaletteItem::Action {
                        label,
                        shortcut,
                        icon,
                        ..
                    } => div()
                        .id(gpui::ElementId::Name(format!("pal-action-{index}").into()))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(rgba(theme.fg0))
                        .when(is_selected, |el| el.bg(rgba(theme.bg2)))
                        .hover(|s| s.bg(rgba(theme.bg2)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            this.execute_palette_item(item_for_click.clone(), window, cx);
                        }))
                        .child(panel_icon(icon, theme.accent))
                        .child(label)
                        .when_some(shortcut, |row, sc| {
                            row.child(
                                div()
                                    .ml_auto()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_xs()
                                    .border_1()
                                    .border_color(rgba(theme.line))
                                    .text_size(px(9.5))
                                    .text_color(rgba(theme.fg2))
                                    .child(format!("[{sc}]")),
                            )
                        }),
                };
                list_container = list_container.child(row);
            }
        }

        let panel = div()
            .occlude()
            .w(px(560.))
            .max_w(relative(0.92))
            .max_h(relative(0.75))
            .overflow_hidden()
            .bg(rgba(theme.bg1))
            .border_1()
            .border_color(rgba(theme.line))
            .shadow_xl()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _ev, _window, cx| {
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .p_3()
                    .border_b_1()
                    .border_color(rgba(theme.line))
                    .child(self.palette_input.clone()),
            )
            .child(list_container);

        div()
            .id("palette-backdrop")
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, window, cx| {
                    this.palette_open = false;
                    this.new_session_target = None;
                    if let Some(active) = this.active.clone() {
                        this.focus_agent(&active, window, cx);
                    }
                    cx.notify();
                }),
            )
            .child(panel)
            .into_any_element()
    }

    fn close_split_pane(&mut self, pane: &PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if self.pane_tree.leaf_count() <= 1 {
            return;
        }
        let agents = self
            .pane_tree
            .group(pane)
            .map(|group| group.tabs.clone())
            .unwrap_or_default();
        if let Some(next) = self.pane_tree.without_pane(pane) {
            self.pane_tree = next;
            for agent in agents {
                let remote = self.remote_snaps.values().any(|snapshot| {
                    snapshot
                        .agents
                        .iter()
                        .any(|candidate| candidate.id == agent)
                });
                self.delete_session(&agent, remote, window, cx);
            }
            if let Ok(mut metrics) = self.split_metrics.lock() {
                metrics.clear();
            }
            self.maximized_pane = None;
            self.active_pane = self.pane_tree.first_pane_id();
            self.active = self
                .pane_tree
                .group(&self.active_pane)
                .and_then(|g| g.active.clone());
            if let Some(active) = self.active.clone() {
                self.focus_agent(&active, window, cx);
            }
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

    fn render_pane_node(&mut self, node: PaneNode, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        match node {
            PaneNode::Split {
                id,
                axis,
                children,
                sizes,
            } => {
                let metrics = Arc::clone(&self.split_metrics);
                let metric_id = id.clone();
                let num_children = children.len();
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
                                    el.w(px(2.))
                                        .h_full()
                                        .ml(px(-1.))
                                        .mr(px(-1.))
                                        .cursor_col_resize()
                                })
                                .when(axis == SplitAxis::Vertical, |el| {
                                    el.h(px(2.))
                                        .w_full()
                                        .mt(px(-1.))
                                        .mb(px(-1.))
                                        .cursor_row_resize()
                                })
                                .on_click(cx.listener({
                                    let split_id = split_id.clone();
                                    move |this, ev: &gpui::ClickEvent, _window, cx| {
                                        if ev.click_count() >= 2 {
                                            let equal_size = 1.0 / num_children.max(1) as f32;
                                            let next_sizes = vec![equal_size; num_children];
                                            if this
                                                .pane_tree
                                                .update_split_sizes(&split_id, next_sizes)
                                            {
                                                this.persist();
                                                cx.notify();
                                            }
                                        }
                                    }
                                }))
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
                    let rendered = self.render_pane_node(child, cx);
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
                let is_focused_pane = group.id == self.active_pane;
                let mut tabs = div()
                    .flex()
                    .items_center()
                    .h(px(34.))
                    .bg(rgba(theme.bg1))
                    .border_b_1()
                    .border_color(rgba(theme.line));
                for tab_id in group.tabs.clone() {
                    let is_active = active_id.as_ref() == Some(&tab_id);
                    let pane_for_tab = pane_id.clone();
                    let agent_opt = self.find_agent(&tab_id);
                    let status = agent_opt
                        .as_ref()
                        .map(|a| a.status)
                        .unwrap_or(muxlane_core::model::AgentStatus::Idle);
                    let seen = agent_opt.as_ref().map(|a| a.seen).unwrap_or(true)
                        || self.active.as_ref() == Some(&tab_id);
                    let is_error = agent_opt
                        .as_ref()
                        .map(|a| a.title.contains("异常") || a.title.contains("错误"))
                        .unwrap_or(false);
                    let att =
                        compute_attention_style(status, seen, is_error, self.pulse_phase, theme);
                    let tab_title = agent_opt
                        .as_ref()
                        .map(|a| {
                            let title = a.title.trim();
                            if title.is_empty() {
                                a.agent_type.as_str().to_string()
                            } else {
                                title.to_string()
                            }
                        })
                        .unwrap_or_else(|| "session".into());
                    let drag_label: SharedString = agent_opt
                        .as_ref()
                        .map(|a| format!("{} · {}", a.agent_type.as_str(), a.status.as_str()))
                        .unwrap_or_default()
                        .into();
                    let tab = div()
                        .id(gpui::ElementId::Name(format!("tab-{tab_id}").into()))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .h_full()
                        .px_2()
                        .text_size(px(11.5))
                        .when(att.is_alerting && att.text_color.is_some(), |el| {
                            el.text_color(rgba(att.text_color.unwrap()))
                        })
                        .when(!att.is_alerting, |el| {
                            el.text_color(rgba(if is_active { theme.fg0 } else { theme.fg1 }))
                        })
                        .when(att.is_alerting && att.bg_color.is_some(), |el| {
                            el.bg(rgba(att.bg_color.unwrap()))
                        })
                        .when(is_active, |el| {
                            el.bg(rgba(theme.bg0))
                                .border_t_2()
                                .border_color(rgba(theme.accent))
                        })
                        .border_r_1()
                        .border_color(rgba(theme.line))
                        .when(!is_active, |el| el.hover(|s| s.bg(rgba(theme.bg2))))
                        .on_click(cx.listener({
                            let id = tab_id.clone();
                            let pane = pane_for_tab.clone();
                            move |this, _ev, window, cx| {
                                this.activate_tab(&pane, &id);
                                this.focus_agent(&id, window, cx);
                                cx.notify();
                            }
                        }))
                        // 鼠标中键直接关闭 Tab
                        .on_mouse_down(
                            MouseButton::Middle,
                            cx.listener({
                                let id = tab_id.clone();
                                let pane = pane_for_tab.clone();
                                move |this, _ev, window, cx| {
                                    cx.stop_propagation();
                                    this.close_tab(&pane, &id, window, cx);
                                }
                            }),
                        )
                        .on_drag(
                            DragTab {
                                agent: tab_id.clone(),
                                from_pane: pane_for_tab.clone(),
                            },
                            {
                                let label = drag_label;
                                move |_, offset, _, cx| {
                                    let label = label.clone();
                                    cx.new(move |_| DragGhost {
                                        label,
                                        offset,
                                        theme,
                                    })
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
                        .child(render_status_indicator(
                            status,
                            is_error,
                            self.spinner_frame,
                            theme,
                        ))
                        .child(div().line_height(px(14.)).child(tab_title))
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("tab-close-{tab_id}").into()))
                                .text_color(rgba(theme.fg2))
                                .rounded_sm()
                                .px_1()
                                .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.fg0)))
                                .on_click(cx.listener({
                                    let id = tab_id.clone();
                                    let pane = pane_id.clone();
                                    move |this, _ev, window, cx| {
                                        cx.stop_propagation();
                                        this.close_tab(&pane, &id, window, cx);
                                    }
                                }))
                                .child("×"),
                        );
                    tabs = tabs.child(tab);
                }
                tabs = tabs.child(
                    div()
                        .id(gpui::ElementId::Name(format!("new-tab-{pane_id}").into()))
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .text_color(rgba(theme.fg1))
                        .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.fg0)))
                        .on_click(cx.listener({
                            let pane = pane_id.clone();
                            move |this, _ev, window, cx| this.new_shell_tab(&pane, window, cx)
                        }))
                        .child(panel_icon(PLUS_ICON, theme.fg1)),
                );
                // 显式分屏/最大化 controls：没有隐式 split。
                tabs = tabs.child(
                    div()
                        .ml_auto()
                        .flex()
                        .items_center()
                        .h_full()
                        .text_color(rgba(theme.fg1))
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("split-h-{pane_id}").into()))
                                .w(px(28.))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.fg0)))
                                .on_click(cx.listener({
                                    let pane = pane_id.clone();
                                    move |this, _ev, window, cx| {
                                        this.split_pane(&pane, SplitAxis::Horizontal, window, cx)
                                    }
                                }))
                                .child(panel_icon(SPLIT_HORIZONTAL_ICON, theme.fg1)),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("split-v-{pane_id}").into()))
                                .w(px(28.))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.fg0)))
                                .on_click(cx.listener({
                                    let pane = pane_id.clone();
                                    move |this, _ev, window, cx| {
                                        this.split_pane(&pane, SplitAxis::Vertical, window, cx)
                                    }
                                }))
                                .child(panel_icon(SPLIT_VERTICAL_ICON, theme.fg1)),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("maximize-{pane_id}").into()))
                                .w(px(28.))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.fg0)))
                                .on_click(cx.listener({
                                    let pane = pane_id.clone();
                                    move |this, _ev, _window, cx| this.toggle_maximize(&pane, cx)
                                }))
                                .child(panel_icon(
                                    if self.maximized_pane.as_ref() == Some(&pane_id) {
                                        RESTORE_ICON
                                    } else {
                                        MAXIMIZE_ICON
                                    },
                                    theme.fg1,
                                )),
                        )
                        .when(self.pane_tree.leaf_count() > 1, |controls| {
                            controls.child(
                                div()
                                    .id(gpui::ElementId::Name(
                                        format!("close-pane-{pane_id}").into(),
                                    ))
                                    .w(px(28.))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(rgba(theme.fg1))
                                    .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.red)))
                                    .on_click(cx.listener({
                                        let pane = pane_id.clone();
                                        move |this, _ev, window, cx| {
                                            this.close_split_pane(&pane, window, cx)
                                        }
                                    }))
                                    .child(panel_icon(CLOSE_ICON, theme.red)),
                            )
                        }),
                );

                let active_agent_opt = active_id.as_ref().and_then(|id| self.find_agent(id));
                let active_status = active_agent_opt
                    .as_ref()
                    .map(|a| a.status)
                    .unwrap_or(muxlane_core::model::AgentStatus::Idle);
                let active_seen = active_agent_opt.as_ref().map(|a| a.seen).unwrap_or(true)
                    || self.active.as_ref() == active_id.as_ref();
                let active_is_error = active_agent_opt
                    .as_ref()
                    .map(|a| a.title.contains("异常") || a.title.contains("错误"))
                    .unwrap_or(false);
                let pane_att = compute_attention_style(
                    active_status,
                    active_seen,
                    active_is_error,
                    self.pulse_phase,
                    theme,
                );

                let content = active_id
                    .as_ref()
                    .and_then(|id| self.terms.get(id).cloned());
                let tab_count = group.tabs.len();
                let target_pane = pane_id.clone();
                let pane_click_id = pane_id.clone();
                let pane_click_active = active_id.clone();
                let pane_drop_bg = Theme::with_alpha(theme.accent, 0x1a);
                let mut pane = div()
                    .id(gpui::ElementId::Name(
                        format!("pane-container-{}", pane_id).into(),
                    ))
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    // 所有 pane 画全边框（交界处双线叠加，简单可靠）
                    .border_1()
                    .border_color(rgba(
                        if let Some(alert_color) =
                            pane_att.border_color.filter(|_| pane_att.is_alerting)
                        {
                            alert_color
                        } else if is_focused_pane {
                            theme.accent
                        } else {
                            theme.line
                        },
                    ))
                    .when(is_focused_pane || pane_att.is_alerting, |el| el.shadow_md())
                    .on_hover(cx.listener({
                        let pane_id = pane_click_id.clone();
                        let active_id = pane_click_active.clone();
                        move |this, hovered: &bool, window, cx| {
                            if !*hovered
                                || this.palette_open
                                || this.connect_dialog
                                || this.project_dialog
                                || this.remote_project_dialog.is_some()
                                || this.session_menu.is_some()
                                || this.tree_menu.is_some()
                                || this.split_drag.is_some()
                            {
                                return;
                            }
                            if this.active_pane != pane_id
                                || this.active.as_ref() != active_id.as_ref()
                            {
                                if let Some(agent_id) = &active_id {
                                    this.activate_tab(&pane_id, agent_id);
                                    this.focus_agent(agent_id, window, cx);
                                } else {
                                    this.active_pane = pane_id.clone();
                                }
                                cx.notify();
                            }
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener({
                            let pane_id = pane_click_id.clone();
                            let active_id = pane_click_active.clone();
                            move |this, _ev: &gpui::MouseDownEvent, window, cx| {
                                if let Some(agent_id) = &active_id {
                                    this.activate_tab(&pane_id, agent_id);
                                    this.focus_agent(agent_id, window, cx);
                                } else {
                                    this.active_pane = pane_id.clone();
                                }
                                cx.notify();
                            }
                        }),
                    )
                    .on_drop::<DragTab>(cx.listener(move |this, drag: &DragTab, _window, cx| {
                        this.move_dragged_tab(drag, &target_pane, tab_count, cx)
                    }))
                    .drag_over::<DragTab>(move |s, _, _, _| s.bg(rgba(pane_drop_bg)))
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
                            .text_color(rgba(theme.fg2))
                            .child("从左侧选择 agent 打开 tab"),
                    );
                }
                pane.into_any_element()
            }
        }
    }
}
fn format_relative_time(then: u64, lang: Language) -> String {
    let now = muxlane_core::model::now_secs();
    let diff = now.saturating_sub(then);
    if diff < 10 {
        i18n::text(lang, "刚刚", "just now").to_string()
    } else if diff < 60 {
        if lang == Language::English {
            format!("{diff}s ago")
        } else {
            format!("{diff}秒前")
        }
    } else if diff < 3600 {
        let m = diff / 60;
        if lang == Language::English {
            format!("{m}m ago")
        } else {
            format!("{m}分钟前")
        }
    } else if diff < 86400 {
        let h = diff / 3600;
        if lang == Language::English {
            format!("{h}h ago")
        } else {
            format!("{h}小时前")
        }
    } else {
        let d = diff / 86400;
        if lang == Language::English {
            format!("{d}d ago")
        } else {
            format!("{d}天前")
        }
    }
}

impl Render for MuxlaneApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_mode(self.theme_mode);
        let snap = self.last_snapshot.clone();
        let machine_name = snap
            .machine
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "local".into());

        // ── 侧栏：机器树（统一 Machines 树：Local Machine + Projects + Sessions）
        let local_machine_key = "local".to_string();
        let local_collapsed = self.collapsed_machines.contains(&local_machine_key);
        let mut tree = div().flex().flex_col().py_1();
        tree = tree.child(
            div()
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgba(theme.fg1))
                .child("MACHINES")
                .child(
                    div()
                        .id("connect-machine")
                        .ml_auto()
                        .w(px(20.))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(rgba(theme.fg1))
                        .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.accent)))
                        .active(|s| s.bg(rgba(theme.bg3)))
                        .tooltip(hover_tip(i18n::text(
                            self.language,
                            "连接远程机器",
                            "Connect remote machine",
                        )))
                        .on_click(cx.listener(|this, _ev, window, cx| {
                            this.open_connect_dialog(window, cx);
                        }))
                        .child(panel_icon(PLUS_ICON, theme.fg1)),
                ),
        );
        tree = tree.child(
            div()
                .id("machine-local")
                .flex()
                .items_center()
                .gap_1()
                .h(px(32.))
                .pl_4()
                .pr_2()
                .text_size(px(12.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgba(theme.fg0))
                .group("local-machine")
                .hover(|style| style.bg(rgba(theme.bg2)))
                .on_click(cx.listener({
                    let key = local_machine_key.clone();
                    move |this, _event, _window, cx| {
                        if !this.collapsed_machines.remove(&key) {
                            this.collapsed_machines.insert(key.clone());
                        }
                        cx.notify();
                    }
                }))
                .child(machine_name.clone())
                .child(
                    div()
                        .ml_auto()
                        .px_1()
                        .bg(rgba(theme.bg2))
                        .text_size(px(9.))
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(rgba(theme.fg1))
                        .child("local"),
                )
                .child(
                    div()
                        .id("add-local-project")
                        .w(px(20.))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(rgba(theme.fg1))
                        .invisible()
                        .group_hover("local-machine", |style| style.visible())
                        .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.accent)))
                        .on_click(cx.listener(|this, _ev, window, cx| {
                            cx.stop_propagation();
                            this.project_dialog = true;
                            this.connect_dialog = false;
                            this.dialog_error = None;
                            this.project_input.update(cx, |input, cx| input.reset(cx));
                            this.project_input.focus_handle(cx).focus(window, cx);
                            cx.notify();
                        }))
                        .child(panel_icon(PLUS_ICON, theme.fg1)),
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
                let project_group = format!("project-hover-{}", project.id);
                let mut pnode = div().flex().flex_col().ml(px(18.));
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
                        .h(px(28.))
                        .pl_4()
                        .pr_2()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgba(theme.fg0))
                        .group(project_group.clone())
                        .hover(|style| style.bg(rgba(theme.bg2)))
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
                                move |this, event: &gpui::MouseDownEvent, window, cx| {
                                    this.focus.focus(window, cx);
                                    this.palette_open = false;
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
                                            .bg(rgba(theme.bg2))
                                            .text_size(px(9.))
                                            .font_weight(gpui::FontWeight::NORMAL)
                                            .text_color(rgba(theme.fg1))
                                            .child(branch),
                                    )
                                })
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(
                                            format!("project-add-{}", project.id).into(),
                                        ))
                                        .w(px(20.))
                                        .h(px(20.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(13.))
                                        .font_weight(gpui::FontWeight::NORMAL)
                                        .text_color(rgba(theme.fg1))
                                        .invisible()
                                        .group_hover(project_group.clone(), |style| style.visible())
                                        .hover(|s| {
                                            s.bg(rgba(theme.bg2)).text_color(rgba(theme.accent))
                                        })
                                        .on_click(cx.listener(move |this, _ev, _window, cx| {
                                            cx.stop_propagation();
                                            this.new_session_target = Some(
                                                NewSessionTarget::Local(project_id_for_add.clone()),
                                            );
                                            this.palette_open = true;
                                            this.palette_index = 0;
                                            this.palette_scroll.scroll_to_item(0);
                                            this.palette_input
                                                .update(cx, |input, cx| input.reset(cx));
                                            this.palette_input.focus_handle(cx).focus(_window, cx);
                                            dismiss_context_menus(
                                                &mut this.session_menu,
                                                &mut this.tree_menu,
                                            );
                                            cx.notify();
                                        }))
                                        .child(panel_icon(PLUS_ICON, theme.fg1)),
                                ),
                        ),
                );
                if !project_collapsed {
                    for agent in snap.agents_of(&project.id) {
                        let id = agent.id.clone();
                        let active = self.active.as_deref() == Some(&id);
                        let status = agent.status;
                        let is_error = agent.title.contains("异常") || agent.title.contains("错误");
                        let att = compute_attention_style(
                            status,
                            agent.seen || active,
                            is_error,
                            self.pulse_phase,
                            theme,
                        );
                        let row = div()
                            .id(gpui::ElementId::Name(id.clone().into()))
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(26.))
                            .pl(px(24.))
                            .pr_2()
                            .text_size(px(11.5))
                            .when(att.is_alerting && att.text_color.is_some(), |el| {
                                el.text_color(rgba(att.text_color.unwrap()))
                            })
                            .when(!att.is_alerting, |el| {
                                el.text_color(rgba(if active { theme.fg0 } else { theme.fg1 }))
                            })
                            .hover(|s| s.bg(rgba(theme.bg2)))
                            .when(att.is_alerting && att.bg_color.is_some(), |el| {
                                el.bg(rgba(att.bg_color.unwrap()))
                            })
                            .when(!att.is_alerting && active, |el| el.bg(rgba(theme.bg2)))
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
                                    move |this, ev: &gpui::MouseDownEvent, window, cx| {
                                        this.focus.focus(window, cx);
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
                            )
                            .child(render_status_indicator(
                                status,
                                is_error,
                                self.spinner_frame,
                                theme,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(truncate(&agent.title, 20)),
                            );
                        pnode = pnode.child(row);
                    }
                }
                tree = tree.child(pnode);
            }
        }

        // ── 远程机器分组
        for host in &self.remotes {
            let name = host.cfg.name.clone();
            let machine_target = DeleteTarget::RemoteMachine { host: name.clone() };
            let (dot_color, status_text, remediation) = match self.remote_states.get(&name) {
                Some(muxlane_client::RemoteState::Online(_)) => {
                    if self.remote_snaps.contains_key(&name) {
                        (theme.green, "已连接", None)
                    } else {
                        (theme.fg1, "连接中", None)
                    }
                }
                Some(muxlane_client::RemoteState::NeedsInstall { .. }) => {
                    (theme.yellow, "未安装", Some((true, false, None)))
                }
                Some(muxlane_client::RemoteState::NeedsStart { binary, .. }) => (
                    theme.yellow,
                    "未启动",
                    Some((false, false, Some(binary.clone()))),
                ),
                Some(muxlane_client::RemoteState::NeedsUpgrade { .. }) => {
                    (theme.yellow, "需要更新", Some((false, true, None)))
                }
                Some(muxlane_client::RemoteState::AuthenticationFailed(_)) => {
                    (theme.red, "认证失败", None)
                }
                Some(muxlane_client::RemoteState::Connecting(stage)) => {
                    (theme.yellow, stage.label(), None)
                }
                Some(muxlane_client::RemoteState::Offline(_)) => (theme.fg1, "离线", None),
                _ => (theme.fg1, "连接中", None),
            };
            let reconnectable = !matches!(
                self.remote_states.get(&name),
                Some(muxlane_client::RemoteState::Online(_))
            );
            let status_text = if matches!(
                self.remote_states.get(&name),
                Some(muxlane_client::RemoteState::Online(_))
            ) {
                host.latency_ms()
                    .map(|latency| format!("{latency} ms"))
                    .unwrap_or_else(|| "已连接".into())
            } else if matches!(
                self.remote_states.get(&name),
                Some(muxlane_client::RemoteState::Offline(_))
            ) {
                "已断开".into()
            } else {
                status_text.to_string()
            };
            let machine_key = format!("remote:{name}");
            let machine_collapsed = self.collapsed_machines.contains(&machine_key);
            let snap_ref = self.remote_snaps.get(&name);
            let machine_attention = snap_ref
                .map(|snapshot| {
                    if snapshot
                        .agents
                        .iter()
                        .any(|agent| agent.status == muxlane_core::model::AgentStatus::Blocked)
                    {
                        compute_attention_style(
                            muxlane_core::model::AgentStatus::Blocked,
                            false,
                            false,
                            self.pulse_phase,
                            theme,
                        )
                    } else if snapshot.agents.iter().any(|agent| {
                        agent.status == muxlane_core::model::AgentStatus::Done
                            && !agent.seen
                            && self.active.as_ref() != Some(&agent.id)
                    }) {
                        compute_attention_style(
                            muxlane_core::model::AgentStatus::Done,
                            false,
                            false,
                            self.pulse_phase,
                            theme,
                        )
                    } else {
                        AttentionStyle::default()
                    }
                })
                .unwrap_or_default();
            let remediation_host = name.clone();
            let remote_project_host = name.clone();
            let mut rnode = div().flex().flex_col().mt_1().child(
                div()
                    .id(gpui::ElementId::Name(format!("machine-row-{name}").into()))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(32.))
                    .pl_4()
                    .pr_2()
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgba(theme.fg0))
                    .when(machine_attention.is_alerting, |row| {
                        row.bg(rgba(machine_attention.bg_color.unwrap_or(theme.bg2)))
                    })
                    .group(gpui::SharedString::from(format!("machine-hover-{name}")))
                    .hover(|style| style.bg(rgba(theme.bg2)))
                    .on_click(cx.listener({
                        let key = machine_key.clone();
                        move |this, _event, _window, cx| {
                            if !this.collapsed_machines.remove(&key) {
                                this.collapsed_machines.insert(key.clone());
                            }
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener({
                            let target = machine_target.clone();
                            move |this, event: &gpui::MouseDownEvent, window, cx| {
                                this.focus.focus(window, cx);
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
                    .child(name.clone())
                    .child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("remote-reconnect-{name}").into(),
                            ))
                            .ml_auto()
                            .px_1()
                            .bg(rgba(theme.bg2))
                            .text_size(px(9.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgba(dot_color))
                            .on_click(cx.listener({
                                let host = Arc::clone(host);
                                let host_cfg = host.cfg.clone();
                                let is_auth_failed = matches!(
                                    self.remote_states.get(&name),
                                    Some(muxlane_client::RemoteState::AuthenticationFailed(_))
                                );
                                move |this, _event, window, cx| {
                                    if is_auth_failed {
                                        this.connect_dialog = true;
                                        this.dialog_error = None;
                                        let target_str = match &host_cfg.target {
                                            muxlane_client::Target::Socket(path) => path.clone(),
                                            muxlane_client::Target::Ssh { host, socket }
                                                if socket.is_empty() =>
                                            {
                                                host.clone()
                                            }
                                            muxlane_client::Target::Ssh { host, socket } => {
                                                format!("{host}:{socket}")
                                            }
                                        };
                                        this.connect_input.update(cx, |input, cx| {
                                            input.set_text(&target_str, cx);
                                        });
                                        match &host_cfg.auth {
                                            muxlane_client::SshAuth::Password {
                                                username, ..
                                            } => {
                                                this.connect_auth_mode = ConnectAuthMode::Password;
                                                this.connect_username.update(cx, |input, cx| {
                                                    input.set_text(username, cx);
                                                });
                                                this.connect_password
                                                    .update(cx, |input, cx| input.reset(cx));
                                                this.connect_password
                                                    .focus_handle(cx)
                                                    .focus(window, cx);
                                            }
                                            muxlane_client::SshAuth::PublicKey {
                                                username,
                                                identity_file,
                                            } => {
                                                this.connect_auth_mode = ConnectAuthMode::PublicKey;
                                                if let Some(u) = username {
                                                    this.connect_username.update(
                                                        cx,
                                                        |input, cx| {
                                                            input.set_text(u, cx);
                                                        },
                                                    );
                                                }
                                                if let Some(k) = identity_file {
                                                    this.connect_key_path.update(
                                                        cx,
                                                        |input, cx| {
                                                            input.set_text(k, cx);
                                                        },
                                                    );
                                                }
                                                this.connect_key_path
                                                    .focus_handle(cx)
                                                    .focus(window, cx);
                                            }
                                            muxlane_client::SshAuth::SshConfig => {
                                                this.connect_auth_mode = ConnectAuthMode::SshConfig;
                                                this.connect_input
                                                    .focus_handle(cx)
                                                    .focus(window, cx);
                                            }
                                        }
                                        cx.notify();
                                    } else if reconnectable {
                                        host.reconnect();
                                        this.focus.focus(window, cx);
                                        cx.notify();
                                    }
                                    cx.stop_propagation();
                                }
                            }))
                            .child(status_text),
                    )
                    .child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("remote-project-add-{remote_project_host}").into(),
                            ))
                            .w(px(20.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgba(theme.fg1))
                            .invisible()
                            .group_hover(format!("machine-hover-{name}"), |style| style.visible())
                            .hover(|style| style.bg(rgba(theme.bg2)).text_color(rgba(theme.accent)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                cx.stop_propagation();
                                this.remote_project_dialog = Some(remote_project_host.clone());
                                this.dialog_error = None;
                                this.remote_project_input
                                    .update(cx, |input, cx| input.reset(cx));
                                this.remote_project_input.focus_handle(cx).focus(window, cx);
                                cx.notify();
                            }))
                            .child(panel_icon(PLUS_ICON, theme.fg1)),
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
                                .text_color(rgba(theme.accent))
                                .hover(|style| style.bg(rgba(theme.bg2)))
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
                                .child(if install {
                                    "安装…"
                                } else if upgrade {
                                    "更新…"
                                } else {
                                    "启动…"
                                }),
                        )
                    }),
            );
            // 进度条
            if let Some(progress) = self.bootstrap_progress.get(&name) {
                let overall = progress.phase.overall(progress.percent);
                let phase_text = format_upload_phase(progress);
                let host_to_cancel = name.clone();
                rnode = rnode.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pl_4()
                        .pr_2()
                        .h(px(22.))
                        .text_size(px(10.))
                        .text_color(rgba(theme.accent))
                        .child(phase_text)
                        .child(
                            div().flex_1().h(px(3.)).bg(rgba(theme.bg2)).child(
                                div()
                                    .w(relative(overall as f32 / 100.0))
                                    .h_full()
                                    .bg(rgba(theme.accent)),
                            ),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("sidebar-cancel-bootstrap-{host_to_cancel}").into(),
                                ))
                                .px_1()
                                .cursor_pointer()
                                .text_color(rgba(theme.fg2))
                                .hover(|s| s.text_color(rgba(theme.red)))
                                .on_click(cx.listener(move |this, _ev, _window, cx| {
                                    this.cancel_bootstrap_for_host(&host_to_cancel, cx);
                                }))
                                .child("×"),
                        ),
                );
            }
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
                        let project_branch = project.branch.clone();
                        let spawn_host = name.clone();
                        let spawn_project = project.id.clone();
                        let project_group = format!("remote-project-hover-{name}-{}", project.id);
                        let mut pnode = div().flex().flex_col().ml(px(18.));
                        pnode = pnode.child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("remote-project-row-{name}-{}", project.id).into(),
                                ))
                                .flex()
                                .items_center()
                                .gap_1()
                                .h(px(28.))
                                .pl_4()
                                .pr_2()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgba(theme.fg0))
                                .group(project_group.clone())
                                .hover(|style| style.bg(rgba(theme.bg2)))
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
                                        move |this, event: &gpui::MouseDownEvent, window, cx| {
                                            this.focus.focus(window, cx);
                                            this.palette_open = false;
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
                                                    .bg(rgba(theme.bg2))
                                                    .text_size(px(9.))
                                                    .font_weight(gpui::FontWeight::NORMAL)
                                                    .text_color(rgba(theme.fg1))
                                                    .child(branch),
                                            )
                                        })
                                        .child(
                                            div()
                                                .id(gpui::ElementId::Name(
                                                    format!(
                                                        "remote-session-add-{name}-{}",
                                                        project.id
                                                    )
                                                    .into(),
                                                ))
                                                .w(px(20.))
                                                .h(px(20.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(13.))
                                                .font_weight(gpui::FontWeight::NORMAL)
                                                .text_color(rgba(theme.fg1))
                                                .invisible()
                                                .group_hover(project_group.clone(), |style| {
                                                    style.visible()
                                                })
                                                .hover(|style| {
                                                    style
                                                        .bg(rgba(theme.bg2))
                                                        .text_color(rgba(theme.accent))
                                                })
                                                .on_click(cx.listener({
                                                    let host_name = spawn_host.clone();
                                                    let proj_id = spawn_project.clone();
                                                    move |this, _event, window, cx| {
                                                        cx.stop_propagation();
                                                        this.new_session_target =
                                                            Some(NewSessionTarget::Remote {
                                                                host: host_name.clone(),
                                                                project: proj_id.clone(),
                                                            });
                                                        this.palette_open = true;
                                                        this.palette_index = 0;
                                                        this.palette_scroll.scroll_to_item(0);
                                                        this.palette_input
                                                            .update(cx, |input, cx| {
                                                                input.reset(cx)
                                                            });
                                                        this.palette_input
                                                            .focus_handle(cx)
                                                            .focus(window, cx);
                                                        dismiss_context_menus(
                                                            &mut this.session_menu,
                                                            &mut this.tree_menu,
                                                        );
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(panel_icon(PLUS_ICON, theme.fg1)),
                                        ),
                                ),
                        );
                        if !project_collapsed {
                            for agent in rsnap.agents_of(&project.id) {
                                let id = agent.id.clone();
                                let active = self.active.as_deref() == Some(&id);
                                let status = agent.status;
                                let is_error =
                                    agent.title.contains("异常") || agent.title.contains("错误");
                                let att = compute_attention_style(
                                    status,
                                    agent.seen || active,
                                    is_error,
                                    self.pulse_phase,
                                    theme,
                                );
                                let row = div()
                                    .id(gpui::ElementId::Name(id.clone().into()))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .h(px(26.))
                                    .pl(px(24.))
                                    .pr_2()
                                    .text_size(px(11.5))
                                    .when(att.is_alerting && att.text_color.is_some(), |el| {
                                        el.text_color(rgba(att.text_color.unwrap()))
                                    })
                                    .when(!att.is_alerting, |el| {
                                        el.text_color(rgba(if active {
                                            theme.fg0
                                        } else {
                                            theme.fg1
                                        }))
                                    })
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .when(att.is_alerting && att.bg_color.is_some(), |el| {
                                        el.bg(rgba(att.bg_color.unwrap()))
                                    })
                                    .when(!att.is_alerting && active, |el| el.bg(rgba(theme.bg2)))
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
                                            move |this, ev: &gpui::MouseDownEvent, window, cx| {
                                                this.focus.focus(window, cx);
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
                                    )
                                    .child(render_status_indicator(
                                        status,
                                        is_error,
                                        self.spinner_frame,
                                        theme,
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .child(truncate(&agent.title, 20)),
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

        let unread_count = self.notifications.iter().filter(|n| n.unread).count();

        // ── 终端网格：PaneTree 递归渲染。
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
            .bg(rgba(theme.bg0))
            .child(self.render_pane_node(render_tree, cx));

        // ── 根布局：侧栏 + 网格
        let mut root = div()
            .id("muxlane-root")
            .relative()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| {
                this.palette_open = !this.palette_open;
                this.new_session_target = None;
                if this.palette_open {
                    this.palette_index = 0;
                    this.palette_scroll.scroll_to_item(0);
                    dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu);
                    this.palette_input.update(cx, |input, cx| input.reset(cx));
                    this.palette_input.focus_handle(cx).focus(window, cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                let pane = this.active_pane.clone();
                if let Some(agent) = this
                    .pane_tree
                    .group(&pane)
                    .and_then(|group| group.active.clone())
                {
                    this.close_tab(&pane, &agent, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NewShellTab, window, cx| {
                let pane = this.active_pane.clone();
                this.new_shell_tab(&pane, window, cx);
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                this.next_tab(window, cx);
            }))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| {
                this.prev_tab(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab1, window, cx| {
                this.select_tab_n(0, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab2, window, cx| {
                this.select_tab_n(1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab3, window, cx| {
                this.select_tab_n(2, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab4, window, cx| {
                this.select_tab_n(3, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab5, window, cx| {
                this.select_tab_n(4, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab6, window, cx| {
                this.select_tab_n(5, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab7, window, cx| {
                this.select_tab_n(6, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab8, window, cx| {
                this.select_tab_n(7, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectTab9, window, cx| {
                this.select_tab_n(8, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleTheme, _window, cx| {
                this.toggle_theme(cx);
            }))
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, window, cx| {
                if this.notifications_open {
                    if ev.keystroke.key.as_str() == "escape" {
                        this.notifications_open = false;
                        this.focus.focus(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                } else if this.settings_open {
                    if ev.keystroke.key.as_str() == "escape" {
                        this.settings_open = false;
                        this.focus.focus(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                } else if this.palette_open {
                    // 只拦截 palette 真正消费的导航/确认键；普通字符必须放行，
                    // 否则平台层 key_char 插入路径被切断，输入框无法输入。
                    let handled = this.handle_palette_key(&ev.keystroke, window, cx);
                    if handled {
                        cx.stop_propagation();
                    }
                } else if ev.keystroke.key.as_str() == "escape"
                    && (this.session_menu.is_some() || this.tree_menu.is_some())
                {
                    dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu);
                    cx.stop_propagation();
                    cx.notify();
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
            .flex()
            .size_full()
            .bg(rgba(theme.bg0))
            .text_color(rgba(theme.fg0))
            .font_family("Noto Sans")
            .child(
                div()
                    .w(px(230.))
                    .flex()
                    .flex_col()
                    .bg(rgba(theme.bg1))
                    .border_r_1()
                    .border_color(rgba(theme.line))
                    .child(
                        div()
                            .id("sidebar-tree-scroll")
                            .flex_1()
                            .overflow_y_scroll()
                            .child(tree),
                    )
                    .child(
                        div()
                            .h(px(40.))
                            .px_2()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .border_t_1()
                            .border_color(rgba(theme.line))
                            .child({
                                let has_blocked = self.notifications.iter().any(|n| {
                                    n.unread && n.to == muxlane_core::model::AgentStatus::Blocked
                                });
                                let pulse = (1.0
                                    - (self.pulse_phase as f32 * std::f32::consts::TAU / 36.0)
                                        .cos())
                                    * 0.5;
                                let badge_color = if has_blocked {
                                    theme.yellow
                                } else if unread_count > 0 {
                                    theme.accent
                                } else {
                                    theme.fg2
                                };
                                let badge_glow = if unread_count > 0 {
                                    let glow_alpha = (0x30 as f32 + pulse * 0x60 as f32) as u32;
                                    Some(Theme::with_alpha(badge_color, glow_alpha as u8))
                                } else {
                                    None
                                };

                                div()
                                    .id("sidebar-notification-button")
                                    .relative()
                                    .w(px(32.))
                                    .h(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .active(|s| s.bg(rgba(theme.bg3)))
                                    .tooltip(hover_tip(i18n::text(
                                        self.language,
                                        "通知",
                                        "Notifications",
                                    )))
                                    .when(unread_count > 0, |el| {
                                        el.bg(rgba(Theme::with_alpha(badge_color, 0x18)))
                                    })
                                    .when(self.notifications_open, |el| el.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        this.notifications_open = !this.notifications_open;
                                        cx.notify();
                                    }))
                                    .child(panel_icon(
                                        NOTIFICATION_ICON,
                                        if self.notifications_open || unread_count > 0 {
                                            badge_color
                                        } else {
                                            theme.fg1
                                        },
                                    ))
                                    .when(unread_count > 0, |el| {
                                        el.child(
                                            div()
                                                .absolute()
                                                .top(px(2.))
                                                .right(px(2.))
                                                .min_w(px(14.))
                                                .h(px(14.))
                                                .px(px(3.))
                                                .rounded_full()
                                                .bg(rgba(badge_color))
                                                .when_some(badge_glow, |b, glow| {
                                                    b.border_1().border_color(rgba(glow))
                                                })
                                                .text_size(px(9.))
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgba(theme.on_accent))
                                                .child(if unread_count > 99 {
                                                    "99+".to_string()
                                                } else {
                                                    format!("{unread_count}")
                                                }),
                                        )
                                    })
                            })
                            .child(
                                div()
                                    .id("open-settings")
                                    .w(px(32.))
                                    .h(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .active(|s| s.bg(rgba(theme.bg3)))
                                    .when(self.settings_open, |el| el.bg(rgba(theme.bg2)))
                                    .tooltip(hover_tip(i18n::text(
                                        self.language,
                                        "设置",
                                        "Settings",
                                    )))
                                    .on_click(cx.listener(|this, _ev, window, cx| {
                                        this.settings_open = true;
                                        this.palette_open = false;
                                        this.focus.focus(window, cx);
                                        cx.notify();
                                    }))
                                    .child(panel_icon(
                                        SETTINGS_ICON,
                                        if self.settings_open {
                                            theme.fg0
                                        } else {
                                            theme.fg1
                                        },
                                    )),
                            ),
                    ),
            )
            .child(grid);

        // 浮动右下角 Toast 通知
        if !self.toasts.is_empty() {
            let toast_container = div()
                .id("toast-overlay")
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .flex()
                .flex_col()
                .gap_2()
                .w(px(320.))
                .children(self.toasts.iter().map(|t| {
                    let dot_color = match t.status {
                        muxlane_core::model::AgentStatus::Blocked => theme.yellow,
                        muxlane_core::model::AgentStatus::Done => theme.green,
                        muxlane_core::model::AgentStatus::Working => theme.accent,
                        muxlane_core::model::AgentStatus::Idle => theme.fg2,
                    };
                    let agent_id = t.agent.clone();
                    let toast_id = t.id;
                    div()
                        .id(gpui::ElementId::Name(format!("toast-{}", t.id).into()))
                        .relative()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .p_3()
                        .pl_4()
                        .bg(rgba(theme.bg2))
                        .border_1()
                        .border_color(rgba(theme.line))
                        .rounded_md()
                        .shadow_lg()
                        .cursor_pointer()
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(4.))
                                .bg(rgba(dot_color)),
                        )
                        .on_click(cx.listener({
                            let agent_id = agent_id.clone();
                            move |this, _ev, window, cx| {
                                this.jump_to_agent(&agent_id, window, cx);
                            }
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .w(px(7.))
                                                .h(px(7.))
                                                .rounded_full()
                                                .bg(rgba(dot_color)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(rgba(theme.fg0))
                                                .child(t.title.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(
                                            format!("toast-close-{toast_id}").into(),
                                        ))
                                        .text_size(px(11.))
                                        .text_color(rgba(theme.fg2))
                                        .hover(|s| s.text_color(rgba(theme.fg0)))
                                        .on_click(cx.listener(move |this, _ev, _window, cx| {
                                            cx.stop_propagation();
                                            this.toasts.retain(|item| item.id != toast_id);
                                            cx.notify();
                                        }))
                                        .child("×"),
                                ),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_size(px(11.5))
                                .text_color(rgba(theme.fg1))
                                .child(truncate(&t.message, 120)),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_size(px(9.5))
                                .text_color(rgba(theme.fg2))
                                .child("点击直达终端"),
                        )
                }));
            root = root.child(toast_container);
        }

        if let Some((message, _)) = self.error_toast.clone() {
            root = root.child(
                div()
                    .id("error-toast")
                    .absolute()
                    .top(px(16.))
                    .right(px(16.))
                    .w(px(420.))
                    .p_3()
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.red))
                    .text_size(px(11.5))
                    .text_color(rgba(theme.red))
                    .child(message),
            );
        }

        if self.split_drag.is_some() {
            let mut overlay = div()
                .id("split-drag-overlay")
                .absolute()
                .size_full()
                .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _window, cx| {
                    if this.split_drag.is_some() {
                        this.update_split_drag(ev.position, cx);
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _ev, _window, _cx| this.end_split_drag()),
                );
            if self.split_drag.as_ref().map(|d| d.axis) == Some(SplitAxis::Horizontal) {
                overlay = overlay.cursor_col_resize();
            } else {
                overlay = overlay.cursor_row_resize();
            }
            root = root.child(overlay);
        }

        if self.palette_open {
            root = root.child(self.render_palette(theme, cx));
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
        if self.session_menu.is_some() || self.tree_menu.is_some() {
            root = root.child(
                div()
                    .id("context-menu-backdrop")
                    .absolute()
                    .size_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, window, cx| {
                            if dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu) {
                                if let Some(active) = this.active.clone() {
                                    this.focus_agent(&active, window, cx);
                                }
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }),
                    ),
            );
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
        if self.notifications_open {
            root = root.child(self.render_notifications_popover(theme, cx));
        }
        if self.settings_open {
            root = root.child(self.render_settings(cx));
        }
        root
    }
}

fn render_pi_loading_spinner(frame: usize, theme: Theme) -> gpui::Div {
    let empty_index = frame % 8;
    // 顺时针 8 点阵索引：左列 [0, 7, 6, 5]，右列 [1, 2, 3, 4]
    let left_indices = [0, 7, 6, 5];
    let right_indices = [1, 2, 3, 4];

    let render_col = |indices: [usize; 4]| {
        let mut col = div().flex().flex_col().gap(px(1.5));
        for idx in indices {
            let is_filled = idx != empty_index;
            col = col.child(
                div()
                    .w(px(2.5))
                    .h(px(2.5))
                    .rounded_full()
                    .when(is_filled, |el| el.bg(rgba(theme.accent)))
                    .when(!is_filled, |el| {
                        el.bg(rgba(Theme::with_alpha(theme.accent, 0x25)))
                    }),
            );
        }
        col
    };

    div()
        .w(px(14.))
        .h(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(2.5))
                .items_center()
                .justify_center()
                .child(render_col(left_indices))
                .child(render_col(right_indices)),
        )
}

#[derive(Clone, Copy, Debug, Default)]
struct AttentionStyle {
    bg_color: Option<u32>,
    text_color: Option<u32>,
    border_color: Option<u32>,
    is_alerting: bool,
}

fn compute_attention_style(
    status: muxlane_core::model::AgentStatus,
    seen: bool,
    is_error: bool,
    pulse_phase: usize,
    theme: Theme,
) -> AttentionStyle {
    // 36 步采样的平滑余弦缓动（10 FPS，3.6 秒一周期）。
    let pulse = (1.0 - (pulse_phase as f32 * std::f32::consts::TAU / 36.0).cos()) * 0.5;
    match status {
        muxlane_core::model::AgentStatus::Blocked => {
            let base_color = theme.yellow;
            let alpha = (0x0e as f32 + pulse * 0x28 as f32) as u32;
            let border_alpha = (0x40 as f32 + pulse * 0x80 as f32) as u32;
            AttentionStyle {
                bg_color: Some(Theme::with_alpha(base_color, alpha as u8)),
                text_color: Some(base_color),
                border_color: Some(Theme::with_alpha(base_color, border_alpha as u8)),
                is_alerting: true,
            }
        }
        muxlane_core::model::AgentStatus::Done if !seen => {
            let base_color = if is_error { theme.red } else { theme.green };
            let alpha = (0x0c as f32 + pulse * 0x24 as f32) as u32;
            let border_alpha = (0x35 as f32 + pulse * 0x75 as f32) as u32;
            AttentionStyle {
                bg_color: Some(Theme::with_alpha(base_color, alpha as u8)),
                text_color: Some(base_color),
                border_color: Some(Theme::with_alpha(base_color, border_alpha as u8)),
                is_alerting: true,
            }
        }
        _ => AttentionStyle::default(),
    }
}

fn render_status_indicator(
    status: muxlane_core::model::AgentStatus,
    is_error: bool,
    spinner_frame: usize,
    theme: Theme,
) -> gpui::Div {
    let container = div()
        .w(px(14.))
        .h(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();

    match status {
        muxlane_core::model::AgentStatus::Working => {
            render_pi_loading_spinner(spinner_frame, theme)
        }
        muxlane_core::model::AgentStatus::Blocked => container.child(
            div()
                .w(px(6.))
                .h(px(6.))
                .rounded_full()
                .bg(rgba(theme.yellow)),
        ),
        muxlane_core::model::AgentStatus::Done if is_error => {
            container.child(div().w(px(6.)).h(px(6.)).rounded_full().bg(rgba(theme.red)))
        }
        muxlane_core::model::AgentStatus::Done => container.child(
            div()
                .w(px(6.))
                .h(px(6.))
                .rounded_full()
                .bg(rgba(theme.green)),
        ),
        muxlane_core::model::AgentStatus::Idle => {
            container.child(div().w(px(5.)).h(px(5.)).rounded_full().bg(rgba(theme.fg2)))
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        let t: String = s.chars().take(n).collect();
        format!("{t}…")
    } else {
        s.to_string()
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 上传阶段进度文本：如「上传二进制 12.3 MB / 45.6 MB (27%)」；
/// 无字节数时退化为「上传二进制 27%」/「上传二进制…」
fn format_upload_phase(progress: &muxlane_client::BootstrapProgress) -> String {
    let label = progress.phase.label();
    match (progress.done_bytes, progress.total_bytes, progress.percent) {
        (Some(done), Some(total), Some(percent)) if total > 0 => {
            format!(
                "{label} {} / {} ({percent}%)",
                format_bytes(done),
                format_bytes(total)
            )
        }
        (_, _, Some(percent)) => format!("{label} {percent}%"),
        _ => format!("{label}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_body_preserves_content_and_has_status_fallbacks() {
        assert_eq!(
            effective_notification_body(
                muxlane_core::model::AgentStatus::Done,
                Some("  完成了修复\n并通过测试  ".into()),
            ),
            "完成了修复 并通过测试"
        );
        assert_eq!(
            effective_notification_body(muxlane_core::model::AgentStatus::Done, Some("  ".into())),
            "任务已完成"
        );
        assert_eq!(
            effective_notification_body(muxlane_core::model::AgentStatus::Blocked, None),
            "等待输入"
        );
    }

    #[test]
    fn upload_progress_text_shows_bytes_and_percent() {
        use muxlane_client::BootstrapPhase;
        let progress = muxlane_client::BootstrapProgress {
            phase: BootstrapPhase::Upload,
            percent: Some(27),
            done_bytes: Some(12 * 1024 * 1024 + 300 * 1024),
            total_bytes: Some(45 * 1024 * 1024),
        };
        let text = format_upload_phase(&progress);
        assert!(text.contains("12.3 MB"), "{text}");
        assert!(text.contains("45.0 MB"), "{text}");
        assert!(text.contains("27%"), "{text}");
        // 无字节数时退化为纯百分比
        let text = format_upload_phase(&muxlane_client::BootstrapProgress {
            phase: BootstrapPhase::Install,
            percent: Some(50),
            done_bytes: None,
            total_bytes: None,
        });
        assert_eq!(text, "安装 50%");
        // 无细分进度
        let text = format_upload_phase(&muxlane_client::BootstrapProgress {
            phase: BootstrapPhase::Restart,
            percent: None,
            done_bytes: None,
            total_bytes: None,
        });
        assert_eq!(text, "重启服务…");
    }

    #[test]
    fn local_project_path_requires_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_local_project_path(dir.path().to_str().unwrap()),
            Some(dir.path().canonicalize().unwrap())
        );
        assert!(resolve_local_project_path("/definitely/missing/muxlane-project").is_none());
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
}
