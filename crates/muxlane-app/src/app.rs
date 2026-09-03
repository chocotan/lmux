//! MuxlaneApp：根组件。侧栏（机器树+通知）+ 贴边终端网格。
#[path = "palette.rs"]
pub(crate) mod palette;
#[path = "panes.rs"]
mod panes;
#[path = "sidebar.rs"]
mod sidebar;
use crate::dialogs::ConnectAuthMode;
use crate::i18n::Language;
use crate::icons::svg_asset;
use crate::menus::{dismiss_context_menus, BootstrapConfirm, DeleteConfirm, SessionMenu, TreeMenu};
use crate::notifications::{NotificationCenter, NotificationCenterEvent, NotificationDraft};
use crate::settings::{DEFAULT_FONT_FAMILY, FONT_FAMILIES};
use crate::term_view::TermView;
use crate::text_field::TextField;
use crate::theme::{Theme, ThemeMode};
use gpui::{
    div, prelude::*, px, rgba, size, App, AssetSource, Bounds, Context, Entity, FocusHandle,
    Focusable, KeyBinding, MouseButton, ParentElement, Render, ScrollHandle, SharedString, Styled,
    Window, WindowBounds, WindowOptions,
};
use muxlane_core::model::{AgentId, Snapshot};
use muxlane_core::{PaneId, PaneNode, SplitAxis};
use muxlane_server::MuxlaneServer;
use palette::NewSessionTarget;
use panes::{DividerDrag, SplitDrag};
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
                            window,
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

pub struct MuxlaneApp {
    // 环境/基础设施
    pub(crate) focus: FocusHandle,
    pub(crate) server: Arc<MuxlaneServer>,
    store_path: std::path::PathBuf,
    pub(crate) remote_event_tx: tokio::sync::mpsc::Sender<muxlane_client::ClientEvent>,

    // Pane/Tab 布局
    /// 递归 pane tree；每个 Leaf 内是 TabGroup（参考 muxel）
    pub(crate) pane_tree: PaneNode,
    pub(crate) active_pane: PaneId,
    pub(crate) maximized_pane: Option<PaneId>,
    pub(crate) active: Option<AgentId>,
    split_drag: Option<SplitDrag>,
    split_metrics: Arc<std::sync::Mutex<HashMap<String, f32>>>,

    // 终端缓存
    pub(crate) terms: HashMap<AgentId, Entity<TermView>>,
    pub(crate) mirror_cancel: HashMap<AgentId, Arc<std::sync::atomic::AtomicBool>>,

    // 快照/远程镜像
    pub(crate) last_snapshot: Snapshot,
    /// 远程机器（本地快照缓存：host name → snapshot）
    pub(crate) remotes: Vec<Arc<muxlane_client::RemoteHost>>,
    pub(crate) remote_snaps: HashMap<String, Snapshot>,
    pub(crate) remote_states: HashMap<String, muxlane_client::RemoteState>,
    /// 远端安装/升级进度（host → 进度）
    pub(crate) bootstrap_progress: HashMap<String, muxlane_client::BootstrapProgress>,

    // 通知 Entity
    pub(crate) notifications: Entity<NotificationCenter>,

    // 外观设置
    pub(crate) theme_mode: ThemeMode,
    pub(crate) font_family: String,
    pub(crate) sound_enabled: bool,
    pub(crate) osc52_clipboard_enabled: bool,
    pub(crate) language: Language,

    // 设置面板
    pub(crate) settings_open: bool,
    pub(crate) settings_theme_menu: bool,
    pub(crate) settings_font_menu: bool,
    pub(crate) settings_language_menu: bool,

    // 命令面板
    pub(crate) palette_open: bool,
    palette_index: usize,
    palette_scroll: ScrollHandle,
    pub(crate) palette_input: Entity<TextField>,
    presets: Vec<muxlane_core::AgentPreset>,
    pub(crate) new_session_target: Option<NewSessionTarget>,

    // 对话框
    pub(crate) connect_dialog: bool,
    pub(crate) connect_input: Entity<TextField>,
    pub(crate) connect_auth_mode: ConnectAuthMode,
    pub(crate) connect_focus_index: usize,
    pub(crate) connect_username: Entity<TextField>,
    pub(crate) connect_password: Entity<TextField>,
    pub(crate) connect_key_path: Entity<TextField>,
    pub(crate) project_dialog: bool,
    pub(crate) remote_project_dialog: Option<String>,
    pub(crate) remote_project_input: Entity<TextField>,
    pub(crate) project_input: Entity<TextField>,
    pub(crate) dialog_error: Option<String>,

    // 菜单/确认框
    pub(crate) session_menu: Option<SessionMenu>,
    pub(crate) tree_menu: Option<TreeMenu>,
    pub(crate) delete_confirm: Option<DeleteConfirm>,
    pub(crate) delete_error: Option<String>,
    pub(crate) delete_busy: bool,
    pub(crate) bootstrap_confirm: Option<BootstrapConfirm>,
    pub(crate) bootstrap_error: Option<String>,

    // 动画/侧栏
    spinner_frame: usize,
    pulse_phase: usize,
    pub(crate) collapsed_machines: std::collections::HashSet<String>,
    pub(crate) collapsed_projects: std::collections::HashSet<String>,
}

impl Focusable for MuxlaneApp {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MuxlaneApp {
    pub fn new(
        window: &Window,
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
                                let draft =
                                    this.notification_draft(p.agent, p.from, p.to, p.message);
                                this.notifications
                                    .update(cx, |center, cx| center.push_notification(draft, cx));
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

        // working spinner / attention pulse；通知浮层动画由 NotificationCenter 独立驱动。
        cx.spawn(async move |this, cx| {
            let mut anim_tick: usize = 0;
            loop {
                let should_animate = match this.update(cx, |this, _cx| this.has_attention()) {
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
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let remotes = Self::restore_remotes(&server, &persisted, &connect_to, &tx);

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
                                let draft = this.notification_draft(agent, from, to, message);
                                this.notifications
                                    .update(cx, |center, cx| center.push_notification(draft, cx));
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
        let notifications = cx.new(|cx| NotificationCenter::new(theme_mode, language, cx));
        cx.subscribe_in(
            &notifications,
            window,
            |this, _center, event: &NotificationCenterEvent, window, cx| match event {
                NotificationCenterEvent::JumpToAgent(agent) => {
                    this.jump_to_agent(agent, window, cx)
                }
            },
        )
        .detach();
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
            notifications,
            theme_mode,
            font_family,
            settings_open: false,
            settings_theme_menu: false,
            settings_font_menu: false,
            settings_language_menu: false,
            sound_enabled: persisted.sound_enabled.unwrap_or(true),
            osc52_clipboard_enabled: persisted.osc52_clipboard_enabled.unwrap_or(false),
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
                            this.osc52_clipboard_enabled,
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

    pub(crate) fn persist(&self) {
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
        app.osc52_clipboard_enabled = Some(self.osc52_clipboard_enabled);
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

    fn has_attention(&self) -> bool {
        let attention = |snapshot: &Snapshot| {
            snapshot.agents.iter().any(|agent| {
                matches!(
                    agent.status,
                    muxlane_core::model::AgentStatus::Working
                        | muxlane_core::model::AgentStatus::Blocked
                ) || (agent.status == muxlane_core::model::AgentStatus::Done && !agent.seen)
            })
        };

        attention(&self.last_snapshot) || self.remote_snaps.values().any(attention)
    }

    fn notification_draft(
        &self,
        agent: AgentId,
        from: muxlane_core::model::AgentStatus,
        to: muxlane_core::model::AgentStatus,
        message: Option<String>,
    ) -> NotificationDraft {
        let details = self
            .last_snapshot
            .agent(&agent)
            .map(|instance| {
                let project_name = self
                    .last_snapshot
                    .project(&instance.project)
                    .map(|project| project.name.clone())
                    .unwrap_or_else(|| "project".into());
                ("local".to_string(), project_name, instance.agent_type)
            })
            .or_else(|| {
                self.remote_snaps.iter().find_map(|(host, snapshot)| {
                    let instance = snapshot.agent(&agent)?;
                    let project_name = snapshot
                        .project(&instance.project)
                        .map(|project| project.name.clone())
                        .unwrap_or_else(|| "project".into());
                    Some((host.clone(), project_name, instance.agent_type))
                })
            })
            .unwrap_or_else(|| {
                (
                    "remote".into(),
                    "project".into(),
                    muxlane_core::model::AgentType::Unknown,
                )
            });

        NotificationDraft {
            focused: self.active.as_ref() == Some(&agent),
            agent,
            machine_name: details.0,
            project_name: details.1,
            agent_type: details.2,
            from,
            to,
            message,
            sound_enabled: self.sound_enabled,
        }
    }
}

impl Render for MuxlaneApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_mode(self.theme_mode);
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
                if this.notifications.read(cx).summary().2 {
                    if ev.keystroke.key.as_str() == "escape" {
                        this.notifications.update(cx, |center, cx| center.close(cx));
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
                            .child(self.render_machine_tree(theme, cx)),
                    )
                    .child(self.render_sidebar_footer(theme, cx)),
            )
            .child(grid);
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
        root = root.child(self.notifications.clone());
        if self.settings_open {
            root = root.child(self.render_settings(cx));
        }
        root
    }
}
