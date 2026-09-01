//! lmux GPUI 主程序：三区极简 UI（侧栏机器树 / 贴边终端网格 / 浮层）
mod app;
mod term_view;
mod text_field;

use gpui::{px, size, App, AppContext as _, Bounds, KeyBinding, WindowBounds, WindowOptions};
use lmux_core::model::MachineInfo;
use lmux_server::{DirtyFlag, LmuxServer, ServerState};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".local/share")
        });
    base.join("lmux")
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "local".into())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("lmux {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "lmux {version}\n\nUSAGE:\n  lmux [--headless] [--connect TARGET[,TARGET...]]\n\nOPTIONS:\n  --headless          Run Unix socket server without a window\n  --connect TARGET    Connect to /path/lmux.sock or user@host:/path/lmux.sock\n  -V, --version       Print version\n  -h, --help          Print help\n\nENV:\n  LMUX_SHELL=/path    Override default shell\n  LMUX_HOOKS=off      Disable agent hook injection",
            version = env!("CARGO_PKG_VERSION")
        );
        return;
    }
    let headless = args.iter().any(|a| a == "--headless");
    let mut connect_to: Vec<String> = args
        .iter()
        .position(|a| a == "--connect")
        .and_then(|i| args.get(i + 1).cloned())
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let dir = data_dir();
    std::fs::create_dir_all(&dir).ok();
    let store_path = lmux_store::default_path(&dir);
    let mut persisted = match lmux_store::load(&store_path) {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "lmux: 无法读取状态文件 {}: {error}\n为避免覆盖原数据，启动已中止。",
                store_path.display()
            );
            std::process::exit(2);
        }
    };
    for remote in &persisted.remotes {
        if !connect_to.contains(remote) {
            connect_to.push(remote.clone());
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .try_init()
        .ok();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let machine_id = std::fs::read_to_string(dir.join("machine_id"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            let id = lmux_core::model::new_id("machine");
            std::fs::write(dir.join("machine_id"), &id).ok();
            id
        });

    let mut initial_state = ServerState::new(MachineInfo {
        machine_id,
        name: hostname(),
        os: std::env::consts::OS.into(),
        version: "0.1.0".into(),
    });
    initial_state.projects = persisted.projects.clone();
    for project in &mut initial_state.projects {
        project.agents.clear();
    }
    let state = Arc::new(RwLock::new(initial_state));
    let dirty = DirtyFlag::new();
    let auth = lmux_core::AuthSecret::load_or_create(&dir.join("secret"))
        .expect("load/create lmux auth secret");
    let server = LmuxServer::new_with_runtime_and_auth(
        dir.join("lmux.sock"),
        Arc::clone(&state),
        dirty,
        rt.handle().clone(),
        auth,
    );
    server.set_persistence_path(store_path.clone());

    {
        let srv = Arc::clone(&server);
        rt.spawn(async move { srv.serve().await.expect("server died") });
    }
    {
        let state = Arc::clone(&state);
        let dirty = server.dirty.clone();
        let sessions = Arc::clone(&server.sessions);
        let subs = Arc::clone(&server.subs);
        let events = server.events.clone();
        rt.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let mut exited = Vec::new();
                let mut screens = Vec::new();
                {
                    let map = sessions.lock().await;
                    for (id, sess) in map.iter() {
                        if let Some(ev) = sess.try_take_exit() {
                            if matches!(ev, lmux_term::SessionEvent::Exit { .. }) {
                                exited.push(id.clone());
                                continue;
                            }
                        }
                        let replay = sess.replay_snapshot();
                        let start = replay.len().saturating_sub(64 * 1024);
                        let tail = &replay[start..];
                        let mut lines = lmux_term::strip_ansi(tail);
                        if lines.len() > 8 {
                            lines = lines.split_off(lines.len() - 8);
                        }
                        screens.push((
                            id.clone(),
                            lmux_core::detect::ScreenInput {
                                bottom_lines: lines,
                                osc_title: lmux_term::extract_osc_title(tail),
                                secs_since_output: None,
                                bell: tail.last() == Some(&0x07),
                            },
                        ));
                    }
                }
                if !exited.is_empty() {
                    {
                        let mut map = sessions.lock().await;
                        for id in &exited {
                            map.remove(id);
                        }
                    }
                    {
                        let mut registry = subs.lock().await;
                        for id in &exited {
                            registry.mark_agent_exit(id);
                        }
                    }
                    let mut st = state.write().await;
                    for id in &exited {
                        for event in st.agent_exit(id) {
                            let _ = events.send(event);
                        }
                    }
                    drop(st);
                    dirty.bump();
                }
                if !screens.is_empty() {
                    let mut st = state.write().await;
                    let mut changed = false;
                    for (id, screen) in &screens {
                        if !st.observe_screen(id, screen).is_empty() {
                            changed = true;
                        }
                    }
                    drop(st);
                    if changed {
                        dirty.bump();
                    }
                }
            }
        });
    }

    // ── Hook 注入（P3）：落盘脚本 + 注入 agent 配置（LMUX_HOOKS=off 可关闭）
    if std::env::var("LMUX_HOOKS").as_deref() != Ok("off") {
        let scripts_dir = lmux_core::hook::hook_scripts_dir(&dir);
        std::fs::create_dir_all(&scripts_dir).ok();
        let report_js = scripts_dir.join("report.mjs");
        std::fs::write(&report_js, lmux_core::hook::REPORT_SCRIPT).ok();
        let socket_path = dir.join("lmux.sock");
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());

        // claude: settings.json hooks（Stop→done / Notification→blocked）
        let claude_settings = PathBuf::from(&home).join(".claude/settings.json");
        if claude_settings
            .parent()
            .map(|p| p.exists())
            .unwrap_or(false)
        {
            let hooks =
                lmux_core::hook::claude_hooks_value(&scripts_dir, "claude-hook", &socket_path);
            if let Err(e) = lmux_core::hook::inject_claude_hooks(&claude_settings, hooks) {
                tracing::warn!(error = %e, "claude hooks 注入失败");
            }
        }
        // codex: config.toml notify
        let codex_config = PathBuf::from(&home).join(".codex/config.toml");
        if codex_config.exists() {
            if let Err(e) =
                lmux_core::hook::inject_codex_notify(&codex_config, &scripts_dir, &socket_path)
            {
                tracing::warn!(error = %e, "codex notify 注入失败");
            }
        }
        if let Err(e) = lmux_core::hook::install_agent_plugins(std::path::Path::new(&home)) {
            tracing::warn!(error = %e, "OpenCode/Pi plugins 安装失败");
        }
        tracing::info!("hook/plugin 注入完成（Claude/Codex/OpenCode/Pi）");
    }

    // 恢复持久 tmux 会话；GUI 关闭只 detach，进程继续运行。
    {
        let sessions = Arc::clone(&server.sessions);
        let mut restored = 0usize;
        for saved in &persisted.sessions {
            let alive = std::process::Command::new("tmux")
                .args(["-L", "lmux", "has-session", "-t", &saved.tmux_session])
                .status()
                .is_ok_and(|s| s.success());
            if !alive {
                continue;
            }
            let Some(project) = persisted
                .projects
                .iter()
                .find(|p| p.id == saved.project_id)
                .cloned()
            else {
                continue;
            };
            let cfg = lmux_term::LaunchCfg {
                agent: saved.agent_id.clone(),
                agent_type: saved.agent_type,
                cwd: project.path.clone(),
                env: vec![
                    ("LMUX_AGENT_ID".into(), saved.agent_id.clone()),
                    (
                        "LMUX_SOCKET".into(),
                        server.socket_path.display().to_string(),
                    ),
                    ("LMUX_HOOK_TOKEN".into(), server.hook_token(&saved.agent_id)),
                ],
                program_override: None,
                args: vec![],
                cols: 120,
                rows: 32,
                tmux_session: Some(saved.tmux_session.clone()),
            };
            if let Ok(session) = lmux_term::PtySession::spawn(cfg) {
                let instance = lmux_core::model::AgentInstance {
                    id: saved.agent_id.clone(),
                    project: saved.project_id.clone(),
                    agent_type: saved.agent_type,
                    title: saved.title.clone(),
                    status: lmux_core::model::AgentStatus::Idle,
                    status_since: lmux_core::model::now_secs(),
                    seen: true,
                    tmux_session: Some(saved.tmux_session.clone()),
                };
                state.blocking_write().add_agent(project, instance);
                sessions
                    .blocking_lock()
                    .insert(saved.agent_id.clone(), session);
                restored += 1;
            }
        }

        // 首次运行才创建一个默认 shell；恢复成功时不额外新增。
        if restored == 0 && !persisted.initialized {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let project =
                persisted
                    .projects
                    .first()
                    .cloned()
                    .unwrap_or_else(|| lmux_core::model::Project {
                        id: "p_demo".into(),
                        name: cwd
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "demo".into()),
                        path: cwd,
                        branch: None,
                        agents: vec![],
                    });
            let agent_id = lmux_core::model::new_id("shell");
            let mut cfg = lmux_term::LaunchCfg::shell(agent_id.clone(), project.path.clone());
            let tmux_name = cfg.tmux_session.clone();
            cfg.env.push(("LMUX_AGENT_ID".into(), agent_id.clone()));
            cfg.env.push((
                "LMUX_SOCKET".into(),
                server.socket_path.display().to_string(),
            ));
            cfg.env
                .push(("LMUX_HOOK_TOKEN".into(), server.hook_token(&agent_id)));
            if let Ok(session) = lmux_term::PtySession::spawn(cfg) {
                let instance = lmux_core::model::AgentInstance {
                    id: agent_id.clone(),
                    project: project.id.clone(),
                    agent_type: lmux_core::model::AgentType::Shell,
                    title: lmux_term::default_shell_program()
                        .rsplit('/')
                        .next()
                        .unwrap_or("shell")
                        .to_string(),
                    status: lmux_core::model::AgentStatus::Idle,
                    status_since: lmux_core::model::now_secs(),
                    seen: true,
                    tmux_session: tmux_name,
                };
                state.blocking_write().add_agent(project, instance);
                sessions.blocking_lock().insert(agent_id, session);
            }
        }
    }
    // 立即落盘当前可恢复会话，避免用户未点开 tab 就关闭导致元数据丢失。
    {
        let snap = state.blocking_read().snapshot();
        persisted.projects = snap.projects.clone();
        persisted.initialized = true;
        for project in &mut persisted.projects {
            project.agents.clear();
        }
        persisted.sessions = snap
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
            .collect();
        let _ = lmux_store::save(&store_path, &persisted);
    }

    if headless {
        // 纯服务端：持续落盘 authoritative state，远端删除后不会在重启时复活。
        tracing::info!("lmux headless server running");
        let state = Arc::clone(&state);
        let mut headless_persisted = persisted.clone();
        let headless_store_path = store_path.clone();
        rt.block_on(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let snap = state.read().await.snapshot();
                headless_persisted.projects = snap.projects.clone();
                for project in &mut headless_persisted.projects {
                    project.agents.clear();
                }
                headless_persisted.sessions = snap
                    .agents
                    .iter()
                    .filter_map(|agent| {
                        Some(lmux_store::PersistedSession {
                            agent_id: agent.id.clone(),
                            project_id: agent.project.clone(),
                            agent_type: agent.agent_type,
                            title: agent.title.clone(),
                            tmux_session: agent.tmux_session.clone()?,
                        })
                    })
                    .collect();
                if let Err(error) = lmux_store::save(&headless_store_path, &headless_persisted) {
                    tracing::warn!(%error, "persist headless state failed");
                }
            }
        });
    }

    let server_for_app = Arc::clone(&server);
    let connect_for_app = connect_to.clone();
    let persisted_for_app = persisted.clone();
    let store_for_app = store_path.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-k", app::TogglePalette, None),
            KeyBinding::new("ctrl-w", app::CloseTab, None),
            KeyBinding::new("ctrl-shift-t", app::NewShellTab, None),
            KeyBinding::new("escape", app::ClosePalette, None),
        ]);
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("lmux");
                cx.new(|cx| {
                    app::LmuxApp::new(
                        cx,
                        Arc::clone(&server_for_app),
                        connect_for_app.clone(),
                        persisted_for_app.clone(),
                        store_for_app.clone(),
                    )
                })
            },
        );
        cx.activate(true);
    });
}
