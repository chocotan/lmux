//! muxlane-server：本机 Unix socket 服务端（client 也能连它：对端机器的 muxlane、或本机 hook 脚本）
mod state;
mod subs;

pub use state::ServerState;
pub use subs::SubRegistry;

use fs2::FileExt;
use muxlane_core::protocol::{
    methods, read_frame, write_frame, AgentReportParams, EventMsg, Response, TermSubscribeParams,
    TermSubscribeResult,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Mutex, RwLock};

/// app 每次状态变更后调用 bump() → 所有连接的 events 订阅者收到 state.changed
#[derive(Clone)]
pub struct DirtyFlag(Arc<tokio::sync::watch::Sender<u64>>);

impl DirtyFlag {
    pub fn new() -> Self {
        DirtyFlag(Arc::new(tokio::sync::watch::channel(0u64).0))
    }
    pub fn bump(&self) {
        let cur = *self.0.borrow();
        let _ = self.0.send(cur + 1);
    }
    fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.subscribe()
    }
}

impl Default for DirtyFlag {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MuxlaneServer {
    pub state: Arc<RwLock<ServerState>>,
    pub runtime: tokio::runtime::Handle,
    /// 会话表独立（PtySession 非 Sync，不能进 RwLock 的共享读）
    pub sessions: Arc<Mutex<HashMap<muxlane_core::model::AgentId, Arc<muxlane_term::PtySession>>>>,
    pub subs: Arc<Mutex<SubRegistry>>,
    pub socket_path: PathBuf,
    pub dirty: DirtyFlag,
    /// 全局状态事件广播（agent.status_changed 等）
    pub events: tokio::sync::broadcast::Sender<EventMsg>,
    pub auth: muxlane_core::AuthSecret,
    pub lifecycle: Arc<Mutex<()>>,
    persistence_path: StdRwLock<Option<PathBuf>>,
}

impl MuxlaneServer {
    pub fn new(
        socket_path: PathBuf,
        state: Arc<RwLock<ServerState>>,
        dirty: DirtyFlag,
    ) -> Arc<Self> {
        Self::new_with_runtime(socket_path, state, dirty, tokio::runtime::Handle::current())
    }

    pub fn new_with_runtime(
        socket_path: PathBuf,
        state: Arc<RwLock<ServerState>>,
        dirty: DirtyFlag,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Self> {
        Self::new_with_runtime_and_auth(
            socket_path,
            state,
            dirty,
            runtime,
            muxlane_core::AuthSecret::generate(),
        )
    }

    pub fn new_with_runtime_and_auth(
        socket_path: PathBuf,
        state: Arc<RwLock<ServerState>>,
        dirty: DirtyFlag,
        runtime: tokio::runtime::Handle,
        auth: muxlane_core::AuthSecret,
    ) -> Arc<Self> {
        // 复用 ServerState 的广播 channel（状态事件单源）
        let events = match state.try_read() {
            Ok(st) => st.events.clone(),
            Err(_) => state.blocking_read().events.clone(),
        };
        Arc::new(MuxlaneServer {
            runtime,
            state,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            subs: Arc::new(Mutex::new(SubRegistry::default())),
            socket_path,
            dirty,
            events,
            auth,
            lifecycle: Arc::new(Mutex::new(())),
            persistence_path: StdRwLock::new(None),
        })
    }

    pub fn hook_token(&self, agent: &muxlane_core::model::AgentId) -> String {
        self.auth.token(agent, 24 * 60 * 60)
    }

    /// 从同步上下文往 runtime 投递任务
    pub fn rt_spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(fut);
    }

    pub fn set_persistence_path(&self, path: PathBuf) {
        if let Ok(mut slot) = self.persistence_path.write() {
            *slot = Some(path);
        }
    }

    async fn persist_runtime_state(&self) -> anyhow::Result<()> {
        let path = self
            .persistence_path
            .read()
            .ok()
            .and_then(|path| path.clone());
        let Some(path) = path else { return Ok(()) };
        let snapshot = self.state.read().await.snapshot();
        let mut persisted = muxlane_store::load(&path).unwrap_or_default();
        persisted.initialized = true;
        persisted.projects = snapshot.projects.clone();
        for project in &mut persisted.projects {
            project.agents.clear();
        }
        persisted.sessions = snapshot
            .agents
            .iter()
            .filter_map(|agent| {
                Some(muxlane_store::PersistedSession {
                    agent_id: agent.id.clone(),
                    project_id: agent.project.clone(),
                    agent_type: agent.agent_type,
                    title: agent.title.clone(),
                    tmux_session: agent.tmux_session.clone()?,
                })
            })
            .collect();
        persisted.maximized_pane = None;
        muxlane_store::save(&path, &persisted)
    }

    /// 启动监听（阻塞当前 async 上下文）
    pub async fn serve(self: Arc<Self>) -> anyhow::Result<()> {
        if let Some(parent) = self.socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 单实例锁：只有持锁者才能清理 stale socket，禁止第二进程 unlink 活跃 listener。
        let lock_path = self.socket_path.with_extension("lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        lock_file.try_lock_exclusive().map_err(|_| {
            anyhow::anyhow!("another muxlane instance owns {}", lock_path.display())
        })?;
        let _instance_lock = lock_file; // 保持到 serve 结束
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
        let listener = UnixListener::bind(&self.socket_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o600))?;
        }
        tracing::info!(path = %self.socket_path.display(), "muxlane server listening");

        // 订阅流泵：把 broadcast 字节流转成 TermData EventMsg 分发给订阅者
        {
            let subs = Arc::clone(&self.subs);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(4)).await;
                    subs.lock().await.pump_once().await;
                }
            });
        }

        loop {
            let (stream, _addr) = listener.accept().await?;
            let server = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(e) = server.handle_conn(stream).await {
                    tracing::debug!(error = %e, "connection closed");
                }
            });
        }
    }

    async fn destroy_sessions(
        &self,
        agents: &[muxlane_core::model::AgentId],
    ) -> (
        Vec<muxlane_core::model::AgentId>,
        Vec<muxlane_core::model::AgentId>,
    ) {
        let sessions: HashMap<_, _> = {
            let mut live = self.sessions.lock().await;
            agents
                .iter()
                .filter_map(|agent| live.remove(agent).map(|session| (agent.clone(), session)))
                .collect()
        };
        let mut destroyed = Vec::new();
        let mut failed = Vec::new();
        for agent in agents {
            if let Some(session) = sessions.get(agent) {
                if session.kill_persistent() {
                    destroyed.push(agent.clone());
                } else {
                    failed.push(agent.clone());
                }
            } else {
                destroyed.push(agent.clone());
            }
        }
        let mut subs = self.subs.lock().await;
        for agent in &destroyed {
            subs.mark_agent_exit(agent);
        }
        (destroyed, failed)
    }

    async fn handle_conn(self: Arc<Self>, stream: UnixStream) -> anyhow::Result<()> {
        let (read_half, mut write_half) = stream.into_split();
        // 独立 reader task 持有累积缓冲：select! 切换到事件分支时不会取消半帧读取。
        let (frame_tx, mut frame_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(read_half);
            loop {
                let frame = read_frame(&mut reader).await;
                let eof = matches!(frame, Err(muxlane_core::Error::Eof));
                if frame_tx.send(frame).await.is_err() || eof {
                    break;
                }
            }
        });

        let (ev_tx, mut ev_rx) = mpsc::channel::<EventMsg>(256);
        let mut status_rx: Option<tokio::sync::broadcast::Receiver<EventMsg>> = None;
        let mut dirty_rx = self.dirty.subscribe();
        let mut dirty_seen = *dirty_rx.borrow_and_update();
        let mut connection_subs: Vec<String> = Vec::new();

        loop {
            tokio::select! {
                frame = frame_rx.recv() => {
                    match frame {
                        Some(Ok(v)) => {
                            let req: muxlane_core::protocol::Request = match serde_json::from_value(v) {
                                Ok(r) => r,
                                Err(e) => {
                                    write_frame(&mut write_half, &Response::err(0, "bad_request", e.to_string())).await?;
                                    continue;
                                }
                            };
                            match req.method.as_str() {
                                methods::SYSTEM_HELLO => {
                                    write_frame(
                                        &mut write_half,
                                        &Response::ok(
                                            req.id,
                                            serde_json::to_value(muxlane_core::protocol::HelloResult {
                                                version: env!("CARGO_PKG_VERSION").into(),
                                                protocol: 2,
                                                features: vec![
                                                    "project.add".into(),
                                                    "agent.spawn".into(),
                                                    "term.input".into(),
                                                    "term.resize".into(),
                                                ],
                                            })?,
                                        ),
                                    )
                                    .await?;
                                }
                                methods::STATE_LIST => {
                                    let snap = self.state.read().await.snapshot();
                                    write_frame(&mut write_half, &Response::ok(req.id, serde_json::to_value(snap)?)).await?;
                                }
                                methods::EVENTS_SUBSCRIBE => {
                                    status_rx = Some(self.events.subscribe());
                                    let resp = Response::ok(req.id, serde_json::json!({"ok": true}));
                                    write_frame(&mut write_half, &resp).await?;
                                    let _ = ev_tx.send(EventMsg::new(muxlane_core::protocol::events::STATE_CHANGED, serde_json::json!({}))).await;
                                }
                                methods::TERM_SUBSCRIBE => {
                                    let resp = match serde_json::from_value::<TermSubscribeParams>(req.params) {
                                        Ok(params) => {
                                            // 块内完成订阅准备，Arc 不跨 await 进入 future 状态机
                                            let prepared = {
                                                let sess_opt = self.sessions.lock().await.get(&params.agent).cloned();
                                                sess_opt.map(|sess| {
                                                    let (snap, rx) = sess.subscribe();
                                                    (muxlane_core::model::new_id("sub"), muxlane_term::b64_encode(&snap), rx, sess)
                                                })
                                            };
                                            match prepared {
                                                Some((sub_id, replay_b64, rx, sess)) => {
                                                    self.subs.lock().await.add(&sub_id, &params.agent, ev_tx.clone(), rx, sess);
                                                    connection_subs.push(sub_id.clone());
                                                    Response::ok(req.id, serde_json::to_value(TermSubscribeResult { sub_id, replay_b64 })?)
                                                }
                                                None => Response::err(req.id, "no_such_agent", format!("agent {} not running", params.agent)),
                                            }
                                        }
                                        Err(e) => Response::err(req.id, "bad_params", e.to_string()),
                                    };
                                    write_frame(&mut write_half, &resp).await?;
                                }
                                methods::TERM_UNSUBSCRIBE => {
                                    if let Some(sid) = req.params["sub_id"].as_str() {
                                        self.subs.lock().await.remove(sid);
                                        connection_subs.retain(|id| id != sid);
                                    }
                                    write_frame(&mut write_half, &Response::ok(req.id, serde_json::json!({"ok": true}))).await?;
                                }
                                methods::TERM_INPUT => {
                                    match serde_json::from_value::<muxlane_core::protocol::TermInputParams>(req.params) {
                                        Ok(params) => {
                                            let session = self.sessions.lock().await.get(&params.agent).cloned();
                                            match session {
                                                Some(session) => {
                                                    let data = muxlane_term::b64_decode(&params.data_b64)?;
                                                    session.write_input(&data);
                                                    // 提交型输入（含换行）→ working；命令结束后
                                                    // 屏幕检测（提示符规则）自动 Working→Idle。
                                                    if data.iter().any(|byte| matches!(byte, b'\r' | b'\n')) {
                                                        let mut st = self.state.write().await;
                                                        let events = st.mark_screen_working(&params.agent);
                                                        drop(st);
                                                        for event in events {
                                                            let _ = self.events.send(event);
                                                        }
                                                        self.dirty.bump();
                                                    }
                                                    write_frame(&mut write_half, &Response::ok(req.id, serde_json::json!({"ok": true}))).await?;
                                                }
                                                None => write_frame(&mut write_half, &Response::err(req.id, "no_such_agent", params.agent)).await?,
                                            }
                                        }
                                        Err(error) => write_frame(&mut write_half, &Response::err(req.id, "bad_params", error.to_string())).await?,
                                    }
                                }
                                methods::TERM_RESIZE => {
                                    match serde_json::from_value::<muxlane_core::protocol::TermResizeParams>(req.params) {
                                        Ok(params) => {
                                            let session = self.sessions.lock().await.get(&params.agent).cloned();
                                            match session {
                                                Some(session) => {
                                                    session.resize(params.cols, params.rows)?;
                                                    write_frame(&mut write_half, &Response::ok(req.id, serde_json::json!({"ok": true}))).await?;
                                                }
                                                None => write_frame(&mut write_half, &Response::err(req.id, "no_such_agent", params.agent)).await?,
                                            }
                                        }
                                        Err(error) => write_frame(&mut write_half, &Response::err(req.id, "bad_params", error.to_string())).await?,
                                    }
                                }
                                methods::AGENT_SPAWN => {
                                    match serde_json::from_value::<muxlane_core::protocol::AgentSpawnParams>(req.params) {
                                        Ok(params) => {
                                            let _lifecycle = self.lifecycle.lock().await;
                                            let project = self.state.read().await.projects.iter()
                                                .find(|project| project.id == params.project)
                                                .cloned();
                                            match project {
                                                Some(project) => {
                                                    let agent_type = params.agent_type.unwrap_or(muxlane_core::model::AgentType::Shell);
                                                    let agent_id = muxlane_core::model::new_id(agent_type.as_str());
                                                    let tmux_name = format!("muxlane-{}", agent_id);
                                                    let title = params.preset_name.unwrap_or_else(|| {
                                                        if agent_type == muxlane_core::model::AgentType::Shell {
                                                            muxlane_term::default_shell_program().rsplit('/').next().unwrap_or("shell").into()
                                                        } else {
                                                            agent_type.as_str().to_string()
                                                        }
                                                    });
                                                    let mut cfg = if agent_type == muxlane_core::model::AgentType::Shell && params.program.is_none() {
                                                        muxlane_term::LaunchCfg::shell(
                                                            agent_id.clone(),
                                                            project.path.clone(),
                                                        )
                                                    } else {
                                                        let program = params.program.unwrap_or_else(|| agent_type.as_str().to_string());
                                                        muxlane_term::LaunchCfg {
                                                            agent: agent_id.clone(),
                                                            agent_type,
                                                            cwd: project.path.clone(),
                                                            env: params.env.unwrap_or_default(),
                                                            program_override: Some(program),
                                                            args: params.args.unwrap_or_default(),
                                                            cols: 120,
                                                            rows: 32,
                                                            tmux_session: Some(tmux_name.clone()),
                                                        }
                                                    };
                                                    cfg.tmux_session = Some(tmux_name.clone());
                                                    cfg.env.push(("MUXLANE_AGENT_ID".into(), agent_id.clone()));
                                                    cfg.env.push(("MUXLANE_SOCKET".into(), self.socket_path.display().to_string()));
                                                    cfg.env.push(("MUXLANE_HOOK_TOKEN".into(), self.hook_token(&agent_id)));
                                                    match muxlane_term::PtySession::spawn(cfg) {
                                                        Ok(session) => {
                                                            let tmux_session = session.tmux_session_name().map(str::to_string);
                                                            self.sessions.lock().await.insert(agent_id.clone(), Arc::clone(&session));
                                                            let instance = muxlane_core::model::AgentInstance {
                                                                id: agent_id.clone(),
                                                                project: project.id.clone(),
                                                                agent_type,
                                                                title,
                                                                status: muxlane_core::model::AgentStatus::Idle,
                                                                status_since: muxlane_core::model::now_secs(),
                                                                seen: true,
                                                                tmux_session,
                                                            };
                                                            self.state.write().await.add_agent(project, instance.clone());
                                                            self.persist_runtime_state().await?;
                                                            self.dirty.bump();
                                                            write_frame(&mut write_half, &Response::ok(req.id, serde_json::to_value(instance)?)).await?;
                                                        }
                                                        Err(error) => write_frame(&mut write_half, &Response::err(req.id, "spawn_failed", error.to_string())).await?,
                                                    }
                                                }
                                                None => write_frame(&mut write_half, &Response::err(req.id, "no_such_project", params.project)).await?,
                                            }
                                        }
                                        Err(error) => write_frame(&mut write_half, &Response::err(req.id, "bad_params", error.to_string())).await?,
                                    }
                                }
                                methods::AGENT_DELETE => {
                                    match serde_json::from_value::<muxlane_core::protocol::AgentDeleteParams>(req.params) {
                                        Ok(params) => {
                                            let session = self.sessions.lock().await.remove(&params.agent);
                                            if let Some(sess) = session { sess.kill_persistent(); }
                                            self.subs.lock().await.mark_agent_exit(&params.agent);
                                            self.state.write().await.remove_agent(&params.agent);
                                            self.dirty.bump();
                                            write_frame(&mut write_half, &Response::ok(req.id, serde_json::json!({"ok": true}))).await?;
                                        }
                                        Err(e) => {
                                            write_frame(&mut write_half, &Response::err(req.id, "bad_params", e.to_string())).await?;
                                        }
                                    }
                                }
                                methods::PROJECT_ADD => {
                                    match serde_json::from_value::<muxlane_core::protocol::ProjectAddParams>(req.params) {
                                        Ok(params) => {
                                            let path = std::path::PathBuf::from(&params.path)
                                                .canonicalize()
                                                .map_err(|error| anyhow::anyhow!("invalid project path: {error}"));
                                            match path {
                                                Ok(path) if path.is_dir() => {
                                                    let name = params.name
                                                        .filter(|name| !name.trim().is_empty())
                                                        .unwrap_or_else(|| path.file_name()
                                                            .map(|value| value.to_string_lossy().into_owned())
                                                            .unwrap_or_else(|| path.display().to_string()));
                                                    let project = muxlane_core::model::Project {
                                                        id: muxlane_core::model::new_id("project"),
                                                        name,
                                                        path,
                                                        branch: None,
                                                        agents: vec![],
                                                    };
                                                    let added = self.state.write().await.add_project(project.clone());
                                                    if added {
                                                        self.persist_runtime_state().await?;
                                                        self.dirty.bump();
                                                    }
                                                    write_frame(&mut write_half, &Response::ok(req.id, serde_json::to_value(project)?)).await?;
                                                }
                                                _ => write_frame(&mut write_half, &Response::err(req.id, "invalid_path", params.path)).await?,
                                            }
                                        }
                                        Err(error) => write_frame(&mut write_half, &Response::err(req.id, "bad_params", error.to_string())).await?,
                                    }
                                }
                                methods::PROJECT_DELETE => {
                                    match serde_json::from_value::<muxlane_core::protocol::ProjectDeleteParams>(req.params) {
                                        Ok(params) => {
                                            let _lifecycle = self.lifecycle.lock().await;
                                            let agents: Vec<_> = self.state.read().await.agents.iter()
                                                .filter(|agent| agent.project == params.project)
                                                .map(|agent| agent.id.clone())
                                                .collect();
                                            let (destroyed_agents, failed_agents) = self.destroy_sessions(&agents).await;
                                            {
                                                let mut state = self.state.write().await;
                                                for agent in &destroyed_agents {
                                                    state.remove_agent(agent);
                                                }
                                                if failed_agents.is_empty() {
                                                    state.projects.retain(|project| project.id != params.project);
                                                }
                                            }
                                            self.dirty.bump();
                                            if let Err(error) = self.persist_runtime_state().await {
                                                write_frame(
                                                    &mut write_half,
                                                    &Response::err(req.id, "persistence_failed", error.to_string()),
                                                ).await?;
                                                continue;
                                            }
                                            write_frame(&mut write_half, &Response::ok(
                                                req.id,
                                                serde_json::to_value(muxlane_core::protocol::DeleteScopeResult {
                                                    destroyed_agents,
                                                    failed_agents,
                                                })?,
                                            )).await?;
                                        }
                                        Err(e) => write_frame(&mut write_half, &Response::err(req.id, "bad_params", e.to_string())).await?,
                                    }
                                }
                                methods::AGENT_REPORT => {
                                    match serde_json::from_value::<AgentReportParams>(req.params) {
                                        Ok(params) => {
                                            if !self.auth.verify(&params.agent, &params.token) {
                                                write_frame(
                                                    &mut write_half,
                                                    &Response::err(req.id, "unauthorized", "invalid or expired hook token"),
                                                )
                                                .await?;
                                                continue;
                                            }
                                            let _events = self.state.write().await.report_hook(&params).await;
                                            write_frame(&mut write_half, &Response::ok(req.id, serde_json::json!({"ok": true}))).await?;
                                            self.dirty.bump();
                                        }
                                        Err(e) => {
                                            write_frame(&mut write_half, &Response::err(req.id, "bad_params", e.to_string())).await?;
                                        }
                                    }
                                }
                                other => {
                                    write_frame(&mut write_half, &Response::err(req.id, "unknown_method", format!("unknown method: {other}"))).await?;
                                }
                            }
                        }
                        Some(Err(muxlane_core::Error::Eof)) | None => break,
                        Some(Err(e)) => return Err(e.into()),
                    }
                }
                ev = ev_rx.recv() => {
                    match ev {
                        Some(msg) => write_frame(&mut write_half, &msg).await?,
                        None => break,
                    }
                }
                status = async { match status_rx.as_mut() { Some(rx) => rx.recv().await, None => std::future::pending().await } }, if true => {
                    match status {
                        Ok(msg) => write_frame(&mut write_half, &msg).await?,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {} // 丢帧继续
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                Ok(()) = dirty_rx.changed() => {
                    let cur = *dirty_rx.borrow_and_update();
                    if cur != dirty_seen {
                        dirty_seen = cur;
                        let _ = ev_tx.try_send(EventMsg::new(muxlane_core::protocol::events::STATE_CHANGED, serde_json::json!({})));
                    }
                }
                else => break,
            }
        }
        if !connection_subs.is_empty() {
            let mut subs = self.subs.lock().await;
            for id in connection_subs {
                subs.remove(&id);
            }
        }
        Ok(())
    }
}
