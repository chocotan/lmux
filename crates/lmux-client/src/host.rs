//! RemoteHost：一台远端 lmux 实例的连接管理（含 SSH 隧道与重连）
use crate::fetch_snapshot;
use lmux_core::model::Snapshot;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use tokio::sync::{mpsc, Mutex, Notify};

#[derive(Clone, Default)]
pub enum SshAuth {
    #[default]
    SshConfig,
    PublicKey {
        username: Option<String>,
        identity_file: Option<String>,
    },
    Password {
        username: String,
        password: String,
    },
}

impl std::fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshAuth::SshConfig => f.write_str("SshConfig"),
            SshAuth::PublicKey {
                username,
                identity_file,
            } => f
                .debug_struct("PublicKey")
                .field("username", username)
                .field("identity_file", identity_file)
                .finish(),
            SshAuth::Password { username, .. } => f
                .debug_struct("Password")
                .field("username", username)
                .field("password", &"[redacted]")
                .finish(),
        }
    }
}

impl SshAuth {
    pub fn destination(&self, host: &str) -> String {
        if host.contains('@') {
            return host.to_string();
        }
        match self {
            SshAuth::PublicKey {
                username: Some(username),
                ..
            }
            | SshAuth::Password { username, .. }
                if !username.trim().is_empty() =>
            {
                format!("{}@{}", username.trim(), host)
            }
            _ => host.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostCfg {
    pub name: String,
    /// 直连 socket 路径（本机/已转发）或 SSH 目标（user@host）
    pub target: Target,
    pub auth: SshAuth,
    /// 重连初始退避
    pub retry_base_ms: u64,
}

#[derive(Debug, Clone)]
pub enum Target {
    /// 本机 unix socket 直连
    Socket(String),
    /// SSH：host + 远端 socket 路径（远端和本地路径通常一致）
    Ssh { host: String, socket: String },
}

/// 用户输入 → 连接目标：
/// - `/path/lmux.sock`：本地/已转发 Unix socket
/// - `user@host:/path/lmux.sock`：SSH StreamLocalForward
pub fn parse_target(input: &str) -> Target {
    let input = input.trim();
    if input.starts_with('/') {
        return Target::Socket(input.into());
    }
    if let Some((host, socket)) = input.split_once(':') {
        if !host.is_empty() && socket.starts_with('/') {
            return Target::Ssh {
                host: host.into(),
                socket: socket.into(),
            };
        }
    }
    // 普通用户只填 SSH config alias / user@host；socket 自动发现。
    Target::Ssh {
        host: input.into(),
        socket: String::new(),
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;

    #[test]
    fn ssh_auth_builds_destination_without_exposing_password() {
        let password = SshAuth::Password {
            username: "alice".into(),
            password: "super-secret".into(),
        };
        assert_eq!(password.destination("server"), "alice@server");
        assert!(!format!("{password:?}").contains("super-secret"));
        let key = SshAuth::PublicKey {
            username: Some("bob".into()),
            identity_file: Some("~/.ssh/id_ed25519".into()),
        };
        assert_eq!(key.destination("nuc"), "bob@nuc");
    }

    #[test]
    fn ssh_alias_does_not_require_internal_socket_path() {
        match parse_target("choco@192.168.1.20") {
            Target::Ssh { host, socket } => {
                assert_eq!(host, "choco@192.168.1.20");
                assert!(socket.is_empty());
            }
            Target::Socket(_) => panic!("expected SSH target"),
        }
    }

    #[test]
    fn advanced_socket_targets_remain_supported() {
        assert!(matches!(parse_target("/tmp/lmux.sock"), Target::Socket(_)));
        assert!(matches!(
            parse_target("nuc:/run/user/1000/lmux.sock"),
            Target::Ssh { socket, .. } if socket == "/run/user/1000/lmux.sock"
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteStage {
    SshProbe,
    Tunnel,
    Subscribe,
}

impl RemoteStage {
    pub fn label(self) -> &'static str {
        match self {
            RemoteStage::SshProbe => "SSH 探测",
            RemoteStage::Tunnel => "建立 tunnel",
            RemoteStage::Subscribe => "订阅状态",
        }
    }
}

#[derive(Debug, Clone)]
pub enum RemoteState {
    Connecting(RemoteStage),
    NeedsInstall {
        remote_socket: String,
    },
    NeedsStart {
        remote_socket: String,
        binary: String,
    },
    NeedsUpgrade {
        remote_socket: String,
    },
    AuthenticationFailed(String),
    Online(Snapshot),
    Offline(String),
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    StateChanged {
        host: String,
        state: RemoteState,
    },
    StatusChanged {
        host: String,
        agent: lmux_core::model::AgentId,
        from: lmux_core::model::AgentStatus,
        to: lmux_core::model::AgentStatus,
        message: Option<String>,
    },
    TermData {
        agent: lmux_core::model::AgentId,
        data: Vec<u8>,
    },
}

pub struct RemoteHost {
    pub cfg: HostCfg,
    state: Arc<Mutex<RemoteState>>,
    events_tx: mpsc::Sender<ClientEvent>,
    endpoint: StdRwLock<Option<String>>,
    stopped: AtomicBool,
    retry: Notify,
    latency_ms: AtomicU64,
}

impl RemoteHost {
    pub fn new(cfg: HostCfg, events_tx: mpsc::Sender<ClientEvent>) -> Arc<Self> {
        Arc::new(RemoteHost {
            cfg,
            state: Arc::new(Mutex::new(RemoteState::Connecting(RemoteStage::SshProbe))),
            events_tx,
            endpoint: StdRwLock::new(None),
            stopped: AtomicBool::new(false),
            retry: Notify::new(),
            latency_ms: AtomicU64::new(0),
        })
    }

    pub async fn state(&self) -> RemoteState {
        self.state.lock().await.clone()
    }

    /// 本地可连的 socket 路径（直连或隧道端口对应的 socket）
    async fn local_socket(&self) -> Result<String, crate::tunnel::TunnelError> {
        let endpoint = match &self.cfg.target {
            Target::Socket(p) => p.clone(),
            Target::Ssh { host, socket } => {
                crate::tunnel::ensure_tunnel(host, socket, &self.cfg.auth).await?
            }
        };
        if let Ok(mut slot) = self.endpoint.write() {
            *slot = Some(endpoint.clone());
        }
        Ok(endpoint)
    }

    pub fn endpoint_now(&self) -> Option<String> {
        self.endpoint.read().ok().and_then(|v| v.clone())
    }

    pub async fn install_and_start(&self) -> Result<(), crate::tunnel::TunnelError> {
        let Target::Ssh { host, .. } = &self.cfg.target else {
            return Err(crate::tunnel::TunnelError::Other(
                "direct socket target cannot be installed".into(),
            ));
        };
        crate::tunnel::install_and_start(host, &self.cfg.auth).await?;
        self.retry.notify_one();
        Ok(())
    }

    pub async fn start_and_retry(&self, binary: &str) -> Result<(), crate::tunnel::TunnelError> {
        let Target::Ssh { host, .. } = &self.cfg.target else {
            return Err(crate::tunnel::TunnelError::Other(
                "direct socket target cannot be started".into(),
            ));
        };
        crate::tunnel::start_remote(host, &self.cfg.auth, Some(binary)).await?;
        self.retry.notify_one();
        Ok(())
    }

    pub async fn upgrade_and_retry(&self) -> Result<(), crate::tunnel::TunnelError> {
        let Target::Ssh { host, .. } = &self.cfg.target else {
            return Err(crate::tunnel::TunnelError::Other(
                "direct socket target cannot be upgraded".into(),
            ));
        };
        crate::tunnel::install_and_restart(host, &self.cfg.auth).await?;
        self.retry.notify_one();
        Ok(())
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.retry.notify_waiters();
    }

    pub fn reconnect(&self) {
        self.retry.notify_one();
    }

    pub fn latency_ms(&self) -> Option<u64> {
        let latency = self.latency_ms.load(Ordering::Relaxed);
        (latency > 0).then_some(latency)
    }

    async fn wait_retry(&self, delay: std::time::Duration) {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            _ = self.retry.notified() => {},
        }
    }

    /// 连接循环（由调用方 spawn：tokio::spawn(host.run_loop()) 或 runtime.spawn）
    pub async fn run_loop(self: Arc<Self>) {
        let this = self;
        let mut backoff = this.cfg.retry_base_ms.max(200);
        loop {
            if this.stopped.load(Ordering::Acquire) {
                break;
            }
            this.set_state(
                RemoteState::Connecting(RemoteStage::SshProbe),
                &this.events_tx,
            )
            .await;
            let socket = match this.local_socket().await {
                Ok(socket) => socket,
                Err(crate::tunnel::TunnelError::NeedsInstall { remote_socket }) => {
                    this.set_state(RemoteState::NeedsInstall { remote_socket }, &this.events_tx)
                        .await;
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {},
                        _ = this.retry.notified() => {},
                    }
                    continue;
                }
                Err(crate::tunnel::TunnelError::NeedsStart {
                    remote_socket,
                    binary,
                }) => {
                    this.set_state(
                        RemoteState::NeedsStart {
                            remote_socket,
                            binary,
                        },
                        &this.events_tx,
                    )
                    .await;
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {},
                        _ = this.retry.notified() => {},
                    }
                    continue;
                }
                Err(crate::tunnel::TunnelError::Authentication(error)) => {
                    this.set_state(RemoteState::AuthenticationFailed(error), &this.events_tx)
                        .await;
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {},
                        _ = this.retry.notified() => {},
                    }
                    continue;
                }
                Err(error) => {
                    this.set_state(RemoteState::Offline(error.to_string()), &this.events_tx)
                        .await;
                    this.wait_retry(std::time::Duration::from_millis(backoff))
                        .await;
                    backoff = (backoff * 2).min(30_000);
                    continue;
                }
            };
            this.set_state(
                RemoteState::Connecting(RemoteStage::Tunnel),
                &this.events_tx,
            )
            .await;
            match crate::open(&socket).await {
                Ok(mut conn) => {
                    let started = std::time::Instant::now();
                    let hello = conn
                        .call(
                            lmux_core::protocol::methods::SYSTEM_HELLO,
                            serde_json::json!({}),
                        )
                        .await
                        .and_then(|value| {
                            serde_json::from_value::<lmux_core::protocol::HelloResult>(value)
                                .map_err(Into::into)
                        });
                    let compatible = hello.as_ref().is_ok_and(|hello| {
                        hello.protocol >= 2
                            && hello.features.iter().any(|feature| feature == "term.input")
                            && hello
                                .features
                                .iter()
                                .any(|feature| feature == "project.add")
                    });
                    if !compatible {
                        this.set_state(
                            RemoteState::NeedsUpgrade {
                                remote_socket: socket.clone(),
                            },
                            &this.events_tx,
                        )
                        .await;
                        tokio::select! {
                            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {},
                            _ = this.retry.notified() => {},
                        }
                        continue;
                    }
                    this.set_state(
                        RemoteState::Connecting(RemoteStage::Subscribe),
                        &this.events_tx,
                    )
                    .await;
                    // 订阅事件流
                    if let Err(e) = conn
                        .call(
                            lmux_core::protocol::methods::EVENTS_SUBSCRIBE,
                            serde_json::json!(null),
                        )
                        .await
                    {
                        this.set_state(RemoteState::Offline(e.to_string()), &this.events_tx)
                            .await;
                        this.wait_retry(std::time::Duration::from_millis(backoff))
                            .await;
                        backoff = (backoff * 2).min(30_000);
                        continue;
                    }
                    // 拉全量快照
                    match fetch_snapshot(&mut conn).await {
                        Ok(snap) => {
                            this.latency_ms.store(
                                started.elapsed().as_millis().clamp(1, u64::MAX as u128) as u64,
                                Ordering::Relaxed,
                            );
                            this.set_state(RemoteState::Online(snap), &this.events_tx)
                                .await;
                            backoff = this.cfg.retry_base_ms.max(200);
                        }
                        Err(e) => {
                            this.set_state(RemoteState::Offline(e.to_string()), &this.events_tx)
                                .await;
                            this.wait_retry(std::time::Duration::from_millis(backoff))
                                .await;
                            backoff = (backoff * 2).min(30_000);
                            continue;
                        }
                    }
                    // 读循环：事件推送
                    let (mut writer, mut reader) = conn.into_split();
                    loop {
                        if this.stopped.load(Ordering::Acquire) {
                            return;
                        }
                        let next = tokio::select! {
                            frame = reader.next() => frame,
                            _ = this.retry.notified() => {
                                if this.stopped.load(Ordering::Acquire) {
                                    return;
                                }
                                break;
                            }
                        };
                        match next {
                            Ok(crate::Frame::Event(ev)) => {
                                let _ = &mut writer;
                                match ev.event.as_str() {
                                    lmux_core::protocol::events::STATE_CHANGED => {
                                        // 200ms 防抖后短连接重拉全量快照（项目/新增/删除/标题/分支）。
                                        tokio::time::sleep(std::time::Duration::from_millis(200))
                                            .await;
                                        if let Ok(mut refresh) = crate::open(&socket).await {
                                            if let Ok(snap) = fetch_snapshot(&mut refresh).await {
                                                this.set_state(
                                                    RemoteState::Online(snap),
                                                    &this.events_tx,
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                    lmux_core::protocol::events::AGENT_STATUS => {
                                        if let Ok(s) =
                                            serde_json::from_value::<
                                                lmux_core::protocol::AgentStatusEvent,
                                            >(ev.params)
                                        {
                                            let _ = this
                                                .events_tx
                                                .send(ClientEvent::StatusChanged {
                                                    host: this.cfg.name.clone(),
                                                    agent: s.agent,
                                                    from: s.from,
                                                    to: s.to,
                                                    message: s.message,
                                                })
                                                .await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Ok(crate::Frame::Response(_)) => {}
                            Err(e) => {
                                this.set_state(
                                    RemoteState::Offline(e.to_string()),
                                    &this.events_tx,
                                )
                                .await;
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    this.set_state(RemoteState::Offline(e.to_string()), &this.events_tx)
                        .await;
                }
            }
            this.wait_retry(std::time::Duration::from_millis(backoff))
                .await;
            if this.stopped.load(Ordering::Acquire) {
                break;
            }
            backoff = (backoff * 2).min(30_000);
        }
    }

    async fn set_state(&self, s: RemoteState, tx: &mpsc::Sender<ClientEvent>) {
        *self.state.lock().await = s.clone();
        let _ = tx
            .send(ClientEvent::StateChanged {
                host: self.cfg.name.clone(),
                state: s,
            })
            .await;
    }
}
