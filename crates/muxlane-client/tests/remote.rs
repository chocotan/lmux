//! 客户端集成测试：对端起真 muxlane-server，RemoteHost 完整走 连接→快照→事件→term 流
use muxlane_client::{ClientEvent, HostCfg, RemoteHost, Target};
use muxlane_core::model::{AgentInstance, AgentStatus, AgentType, MachineInfo, Project};
use muxlane_server::{DirtyFlag, MuxlaneServer, ServerState};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

async fn spawn_peer(
    dir: &tempfile::TempDir,
) -> (Arc<MuxlaneServer>, Arc<RwLock<ServerState>>, String) {
    let sock = dir.path().join("peer.sock");
    let state = Arc::new(RwLock::new(ServerState::new(MachineInfo {
        machine_id: "m_peer".into(),
        name: "peer-box".into(),
        os: "linux".into(),
        version: "0.1.0".into(),
    })));
    let dirty = DirtyFlag::new();
    let server = MuxlaneServer::new(sock.clone(), Arc::clone(&state), dirty);

    let srv = Arc::clone(&server);
    tokio::spawn(async move { srv.serve().await.unwrap() });

    // 等 socket 就绪
    for _ in 0..50 {
        if tokio::net::UnixStream::connect(&sock).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (server, state, sock.display().to_string())
}

#[tokio::test]
async fn remote_host_connects_and_receives_events() {
    let dir = tempfile::tempdir().unwrap();
    let (server, state, sock) = spawn_peer(&dir).await;

    // 对端注册一个项目+agent
    let agent_id = muxlane_core::model::new_id("shell");
    let inst = AgentInstance {
        id: agent_id.clone(),
        project: "p1".into(),
        agent_type: AgentType::Shell,
        title: "peer bash".into(),
        status: AgentStatus::Idle,
        status_since: muxlane_core::model::now_secs(),
        seen: true,
        tmux_session: None,
    };
    let proj = Project {
        id: "p1".into(),
        name: "peer-proj".into(),
        path: "/tmp/peer-proj".into(),
        branch: Some("main".into()),
        agents: vec![agent_id.clone()],
    };
    state.write().await.add_agent(proj, inst);

    // 客户端连上去
    let (tx, mut rx) = mpsc::channel(64);
    let host = RemoteHost::new(
        HostCfg {
            name: "peer".into(),
            target: Target::Socket(sock),
            auth: muxlane_client::SshAuth::SshConfig,
            retry_base_ms: 200,
        },
        tx,
    );
    tokio::spawn(std::clone::Clone::clone(&host).run_loop());

    // 收到 Online 状态，且快照含 peer-box
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut got_online = false;
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Some(ClientEvent::StateChanged {
                state: RemoteState::Online(snap),
                ..
            })) => {
                assert_eq!(snap.machine.as_ref().unwrap().name, "peer-box");
                assert_eq!(snap.agents.len(), 1);
                got_online = true;
                break;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    assert!(got_online, "should receive Online snapshot");

    // 远端新增会话只发 state.changed；RemoteHost 必须重拉 state.list。
    let second = muxlane_core::model::AgentInstance {
        id: "shell_second".into(),
        project: "p1".into(),
        agent_type: AgentType::Shell,
        title: "second".into(),
        status: AgentStatus::Idle,
        status_since: muxlane_core::model::now_secs(),
        seen: true,
        tmux_session: None,
    };
    let second_id = second.id.clone();
    state.write().await.agents.push(second);
    state.write().await.projects[0]
        .agents
        .push(second_id.clone());
    server
        .add_project(muxlane_core::protocol::ProjectAddParams {
            path: dir.path().display().to_string(),
            name: Some("refresh-trigger".into()),
        })
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut refreshed = false;
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Some(ClientEvent::StateChanged {
                state: RemoteState::Online(snap),
                ..
            })) if snap.agents.len() == 2 => {
                refreshed = true;
                break;
            }
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    assert!(refreshed, "state.changed refreshes remote snapshot");

    // 对端 hook 上报 → 客户端应收到 StatusChanged
    state
        .write()
        .await
        .report_hook(&muxlane_core::protocol::AgentReportParams {
            token: String::new(),
            agent: agent_id.clone(),
            event: "done".into(),
            message: Some("remote done".into()),
        })
        .await;

    let mut got_status = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Some(ClientEvent::StatusChanged { to, .. })) => {
                got_status = Some(to);
                break;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    assert_eq!(
        got_status,
        Some(AgentStatus::Done),
        "client receives remote status change"
    );

    drop(server);
    let _ = agent_id;
}

use muxlane_client::RemoteState;

#[test]
fn parses_socket_and_ssh_targets() {
    assert!(matches!(
        muxlane_client::parse_target("/tmp/muxlane.sock"),
        muxlane_client::Target::Socket(p) if p == "/tmp/muxlane.sock"
    ));
    assert!(matches!(
        muxlane_client::parse_target("nuc"),
        muxlane_client::Target::Ssh { host, socket } if host == "nuc" && socket.is_empty()
    ));
    assert!(matches!(
        muxlane_client::parse_target("choco@192.168.1.20"),
        muxlane_client::Target::Ssh { host, socket } if host == "choco@192.168.1.20" && socket.is_empty()
    ));
    assert!(matches!(
        muxlane_client::parse_target("choco@nuc:/home/choco/.local/share/muxlane/muxlane.sock"),
        muxlane_client::Target::Ssh { host, socket }
            if host == "choco@nuc" && socket.ends_with("/muxlane.sock")
    ));
}
