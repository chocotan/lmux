//! 服务端集成测试：真 UnixListener + 真 PTY + 裸 TCP 客户端模拟
use muxlane_core::model::{AgentId, AgentInstance, AgentStatus, AgentType, MachineInfo, Project};
use muxlane_core::protocol::{
    events, methods, read_frame, write_frame, EventMsg, Request, Response,
};
use muxlane_server::{DirtyFlag, MuxlaneServer, ServerState};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::sync::RwLock;

async fn spawn_server() -> (
    Arc<MuxlaneServer>,
    PathBuf,
    Arc<RwLock<ServerState>>,
    DirtyFlag,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("muxlane.sock");
    let state = Arc::new(RwLock::new(ServerState::new(MachineInfo {
        machine_id: "m_test".into(),
        name: "test-box".into(),
        os: "linux".into(),
        version: "0.1.0".into(),
    })));
    let dirty = DirtyFlag::new();
    let server = MuxlaneServer::new(sock.clone(), Arc::clone(&state), dirty.clone());
    let srv = Arc::clone(&server);
    tokio::spawn(async move { srv.serve().await.unwrap() });
    // 等 socket 就绪
    for _ in 0..50 {
        if UnixStream::connect(&sock).await.is_ok() {
            // 占位连接已建立了一个（无害，server 支持多连接）
            return (server, sock, state, dirty, dir);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("server socket never became ready");
}

async fn launch_agent(
    server: &Arc<MuxlaneServer>,
    state: &Arc<RwLock<ServerState>>,
    script: &str,
) -> AgentId {
    let agent_id = muxlane_core::model::new_id("shell");
    let cfg = muxlane_term::LaunchCfg {
        agent: agent_id.clone(),
        agent_type: AgentType::Shell,
        cwd: std::env::temp_dir(),
        env: vec![],
        program_override: Some("bash".into()),
        args: vec!["-c".into(), script.into()],
        cols: 80,
        rows: 24,
        tmux_session: None,
    };
    let sess = muxlane_term::PtySession::spawn(cfg).unwrap();
    server.sessions.lock().await.insert(agent_id.clone(), sess);
    let project = Project {
        id: "p_test".into(),
        name: "testproj".into(),
        path: "/tmp/testproj".into(),
        branch: Some("main".into()),
        agents: vec![],
    };
    let instance = AgentInstance {
        id: agent_id.clone(),
        project: "p_test".into(),
        agent_type: AgentType::Shell,
        title: "test".into(),
        status: AgentStatus::Idle,
        status_since: muxlane_core::model::now_secs(),
        seen: true,
        tmux_session: None,
    };
    state.write().await.add_agent(project, instance);
    agent_id
}

#[tokio::test]
async fn remote_term_input_reaches_pty_and_project_add_validates_path() {
    let (server, sock, state, _dirty, dir) = spawn_server().await;
    let agent = launch_agent(&server, &state, "read line; echo REMOTE:$line; sleep 5").await;
    let mut conn = UnixStream::connect(&sock).await.unwrap();
    write_frame(
        &mut conn,
        &Request {
            id: 50,
            method: methods::TERM_INPUT.into(),
            params: serde_json::to_value(muxlane_core::protocol::TermInputParams {
                agent: agent.clone(),
                data_b64: muxlane_core::protocol::b64_encode(b"hello\n"),
            })
            .unwrap(),
        },
    )
    .await
    .unwrap();
    let _: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let replay = server.sessions.lock().await[&agent].replay_snapshot();
    assert!(String::from_utf8_lossy(&replay).contains("REMOTE:hello"));

    let project_dir = dir.path().join("remote-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    write_frame(
        &mut conn,
        &Request {
            id: 51,
            method: methods::PROJECT_ADD.into(),
            params: serde_json::json!({ "path": project_dir }),
        },
    )
    .await
    .unwrap();
    // 提交型输入会先广播 agent.status_changed（working）事件，读响应时跳过事件帧。
    let response: Response = loop {
        let value = read_frame(&mut conn).await.unwrap();
        if value.get("event").is_some() {
            continue;
        }
        break serde_json::from_value(value).unwrap();
    };
    let project: Project = serde_json::from_value(response.result.unwrap()).unwrap();
    assert_eq!(project.name, "remote-project");
    assert!(state
        .read()
        .await
        .projects
        .iter()
        .any(|item| item.id == project.id));
}

#[tokio::test]
async fn project_delete_destroys_scoped_sessions_and_state() {
    let (server, sock, state, _dirty, _dir) = spawn_server().await;
    let agent = launch_agent(&server, &state, "sleep 30").await;
    let mut conn = UnixStream::connect(&sock).await.unwrap();
    write_frame(
        &mut conn,
        &Request {
            id: 40,
            method: methods::PROJECT_DELETE.into(),
            params: serde_json::json!({ "project": "p_test" }),
        },
    )
    .await
    .unwrap();
    let response: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    let result: muxlane_core::protocol::DeleteScopeResult =
        serde_json::from_value(response.result.unwrap()).unwrap();
    assert_eq!(result.destroyed_agents, vec![agent]);
    assert!(state.read().await.projects.is_empty());
    assert!(state.read().await.agents.is_empty());
    assert!(server.sessions.lock().await.is_empty());
}

#[tokio::test]
async fn state_list_returns_snapshot() {
    let (srv, sock, state, _dirty, _dir) = spawn_server().await;
    let _agent = launch_agent(&srv, &state, "sleep 5").await;

    let conn = UnixStream::connect(&sock).await.unwrap();
    let mut conn = conn;
    write_frame(
        &mut conn,
        &Request {
            id: 1,
            method: methods::STATE_LIST.into(),
            params: serde_json::json!(null),
        },
    )
    .await
    .unwrap();
    let resp: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    assert_eq!(resp.id, 1);
    let snap: muxlane_core::model::Snapshot = serde_json::from_value(resp.result.unwrap()).unwrap();
    assert_eq!(snap.machine.as_ref().unwrap().name, "test-box");
    assert_eq!(snap.projects.len(), 1);
    assert_eq!(snap.agents.len(), 1);
}

#[tokio::test]
async fn term_subscribe_replay_and_incremental() {
    let (srv, sock, state, _dirty, _dir) = spawn_server().await;
    let agent = launch_agent(
        &srv,
        &state,
        "echo first-out; sleep 1; echo second-out; sleep 5",
    )
    .await;

    // 等 first-out 出现（回放缓冲里）
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let conn = UnixStream::connect(&sock).await.unwrap();
    let mut conn = conn;
    write_frame(
        &mut conn,
        &Request {
            id: 2,
            method: methods::TERM_SUBSCRIBE.into(),
            params: serde_json::json!({ "agent": agent }),
        },
    )
    .await
    .unwrap();
    let resp: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    let result: muxlane_core::protocol::TermSubscribeResult =
        serde_json::from_value(resp.result.unwrap()).unwrap();
    let replay = muxlane_core::protocol::b64_decode(&result.replay_b64).unwrap();
    assert!(
        replay.windows(9).any(|w| w == b"first-out"),
        "replay covers history"
    );

    // 增量：等 second-out 通过订阅流到达
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut got_incremental = false;
    while std::time::Instant::now() < deadline {
        let frame =
            tokio::time::timeout(std::time::Duration::from_millis(500), read_frame(&mut conn))
                .await;
        match frame {
            Ok(Ok(v)) => {
                if v["event"] == events::TERM_DATA {
                    let data = muxlane_core::protocol::b64_decode(
                        v["params"]["data_b64"].as_str().unwrap(),
                    )
                    .unwrap();
                    if data.windows(10).any(|w| w == b"second-out") {
                        got_incremental = true;
                        break;
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("DEBUG frame err: {e:?}");
                break;
            }
            Err(_) => {
                continue;
            } // timeout：继续等下一个事件
        }
    }
    assert!(
        got_incremental,
        "incremental term.data delivers later output"
    );
}

#[tokio::test]
async fn events_subscribe_receives_status_change() {
    let (srv, sock, state, _dirty, _dir) = spawn_server().await;
    let agent = launch_agent(&srv, &state, "sleep 10").await;

    let conn = UnixStream::connect(&sock).await.unwrap();
    let mut conn = conn;
    write_frame(
        &mut conn,
        &Request {
            id: 3,
            method: methods::EVENTS_SUBSCRIBE.into(),
            params: serde_json::json!(null),
        },
    )
    .await
    .unwrap();
    let resp: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    assert!(resp.result.is_some());

    // 从另一连接上报 hook → 本连接应收到状态事件
    let token = srv.hook_token(&agent);
    let conn2 = UnixStream::connect(&sock).await.unwrap();
    let mut conn2 = conn2;
    write_frame(&mut conn2, &Request {
        id: 9,
        method: methods::AGENT_REPORT.into(),
        params: serde_json::json!({ "token": token, "agent": agent, "event": "done", "message": "unit-test done" }),
    }).await.unwrap();
    let resp2: Response = serde_json::from_value(read_frame(&mut conn2).await.unwrap()).unwrap();
    assert!(resp2.result.is_some());

    // conn 收事件：先是订阅确认时的 state.changed，然后 agent.status_changed
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut got = None;
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(500), read_frame(&mut conn))
            .await
        {
            Ok(Ok(v)) => {
                if v["event"] == events::AGENT_STATUS {
                    got = Some(v);
                    break;
                }
            }
            Ok(Err(e)) => {
                eprintln!("DEBUG frame err: {e:?}");
                break;
            }
            Err(_) => {
                continue;
            }
        }
    }
    let ev: EventMsg =
        serde_json::from_value(got.expect("should receive agent.status_changed")).unwrap();
    assert_eq!(ev.params["to"], "done");
}

#[tokio::test]
async fn unknown_agent_subscribe_errors() {
    let (_srv, sock, _state, _dirty, _dir) = spawn_server().await;
    let conn = UnixStream::connect(&sock).await.unwrap();
    let mut conn = conn;
    write_frame(
        &mut conn,
        &Request {
            id: 4,
            method: methods::TERM_SUBSCRIBE.into(),
            params: serde_json::json!({ "agent": "shell_nope" }),
        },
    )
    .await
    .unwrap();
    let resp: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    assert_eq!(resp.error.unwrap().code, "no_such_agent");
}

#[tokio::test]
async fn hook_rejects_invalid_token() {
    let (srv, sock, state, _dirty, _dir) = spawn_server().await;
    let agent = launch_agent(&srv, &state, "sleep 10").await;
    let mut conn = UnixStream::connect(&sock).await.unwrap();
    write_frame(
        &mut conn,
        &Request {
            id: 10,
            method: methods::AGENT_REPORT.into(),
            params: serde_json::json!({
                "token": "v1:9999999999:invalid",
                "agent": agent,
                "event": "done"
            }),
        },
    )
    .await
    .unwrap();
    let resp: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    assert_eq!(resp.error.unwrap().code, "unauthorized");
}

#[tokio::test]
async fn node_hook_script_reports_with_env_identity() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let (srv, sock, state, _dirty, dir) = spawn_server().await;
    let agent = launch_agent(&srv, &state, "sleep 10").await;
    let script = dir.path().join("report.mjs");
    std::fs::write(&script, muxlane_core::hook::REPORT_SCRIPT).unwrap();
    let token = srv.hook_token(&agent);
    let mut child = tokio::process::Command::new("node")
        .arg(&script)
        .arg("done")
        .env("MUXLANE_SOCKET", &sock)
        .env("MUXLANE_AGENT_ID", &agent)
        .env("MUXLANE_HOOK_TOKEN", token)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use tokio::io::AsyncWriteExt;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{}\n")
        .await
        .unwrap();
    child.stdin.take();
    let status = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
        .await
        .expect("hook exits")
        .unwrap();
    assert!(status.success());
    for _ in 0..20 {
        if state.read().await.agents[0].status == AgentStatus::Done {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("node hook did not update agent state");
}

#[tokio::test]
async fn partial_request_survives_interleaved_event() {
    use tokio::io::AsyncWriteExt;
    let (srv, sock, _state, dirty, _dir) = spawn_server().await;
    let mut conn = UnixStream::connect(&sock).await.unwrap();
    // 开事件订阅。
    write_frame(
        &mut conn,
        &Request {
            id: 1,
            method: methods::EVENTS_SUBSCRIBE.into(),
            params: serde_json::json!(null),
        },
    )
    .await
    .unwrap();
    let _resp: Response = serde_json::from_value(read_frame(&mut conn).await.unwrap()).unwrap();
    // 消耗初始 state.changed。
    let _ = read_frame(&mut conn).await.unwrap();

    // 分两段写 state.list，在中间触发 dirty event；半帧必须留在 reader task 的缓冲中。
    conn.write_all(b"{\"id\":42,\"method\":\"state.")
        .await
        .unwrap();
    dirty.bump();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    conn.write_all(b"list\"}\n").await.unwrap();

    let mut got_response = false;
    for _ in 0..4 {
        let v = tokio::time::timeout(std::time::Duration::from_secs(1), read_frame(&mut conn))
            .await
            .unwrap()
            .unwrap();
        if v.get("id").and_then(|v| v.as_u64()) == Some(42) {
            let resp: Response = serde_json::from_value(v).unwrap();
            assert!(resp.result.is_some());
            got_response = true;
            break;
        }
    }
    assert!(
        got_response,
        "partial state.list response arrives after interleaved event"
    );
    drop(srv);
}

#[tokio::test]
async fn second_server_cannot_unlink_active_socket() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("same.sock");
    let mk_state = || {
        Arc::new(RwLock::new(ServerState::new(MachineInfo {
            machine_id: muxlane_core::model::new_id("m"),
            name: "box".into(),
            os: "linux".into(),
            version: "0.1".into(),
        })))
    };
    let first = MuxlaneServer::new(sock.clone(), mk_state(), DirtyFlag::new());
    let first_task = tokio::spawn(Arc::clone(&first).serve());
    for _ in 0..50 {
        if UnixStream::connect(&sock).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let second = MuxlaneServer::new(sock.clone(), mk_state(), DirtyFlag::new());
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), second.serve())
        .await
        .expect("second returns promptly");
    assert!(result.is_err());
    // 第一台仍可连接。
    assert!(UnixStream::connect(&sock).await.is_ok());
    first_task.abort();
}

#[tokio::test]
async fn connection_drop_cleans_terminal_subscriptions() {
    let (srv, sock, state, _dirty, _dir) = spawn_server().await;
    let agent = launch_agent(&srv, &state, "sleep 10").await;
    let mut conn = UnixStream::connect(&sock).await.unwrap();
    write_frame(
        &mut conn,
        &Request {
            id: 77,
            method: methods::TERM_SUBSCRIBE.into(),
            params: serde_json::json!({"agent": agent}),
        },
    )
    .await
    .unwrap();
    let _ = read_frame(&mut conn).await.unwrap();
    assert_eq!(srv.subs.lock().await.len(), 1);
    drop(conn);
    for _ in 0..20 {
        if srv.subs.lock().await.is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("subscription leaked after connection close");
}

#[tokio::test]
async fn agent_delete_kills_and_cleans_state() {
    let (srv, sock, state, _dirty, _dir) = spawn_server().await;
    let agent = launch_agent(&srv, &state, "sleep 60").await;
    let mut events = UnixStream::connect(&sock).await.unwrap();
    write_frame(
        &mut events,
        &Request {
            id: 1,
            method: methods::EVENTS_SUBSCRIBE.into(),
            params: serde_json::json!(null),
        },
    )
    .await
    .unwrap();
    let _ = read_frame(&mut events).await.unwrap(); // response
    let _ = read_frame(&mut events).await.unwrap(); // initial dirty

    let mut rpc = UnixStream::connect(&sock).await.unwrap();
    write_frame(
        &mut rpc,
        &Request {
            id: 2,
            method: methods::AGENT_DELETE.into(),
            params: serde_json::json!({"agent": agent}),
        },
    )
    .await
    .unwrap();
    let resp: Response = serde_json::from_value(read_frame(&mut rpc).await.unwrap()).unwrap();
    assert!(resp.result.is_some());
    assert!(!srv.sessions.lock().await.contains_key(&agent));
    assert!(!state.read().await.agents.iter().any(|a| a.id == agent));
    assert!(!state.read().await.projects[0].agents.contains(&agent));
    let ev = tokio::time::timeout(std::time::Duration::from_secs(1), read_frame(&mut events))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ev["event"], muxlane_core::protocol::events::STATE_CHANGED);
}
