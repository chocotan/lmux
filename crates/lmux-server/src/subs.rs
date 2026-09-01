//! 订阅注册表：term.data 无损边界；lag/backpressure 后显式 term.resync。
use bytes::Bytes;
use lmux_core::model::AgentId;
use lmux_core::protocol::{EventMsg, TermDataEvent, TermResyncEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

struct PendingResync {
    replay_b64: String,
    rx: broadcast::Receiver<Bytes>,
}

struct SubEntry {
    agent: AgentId,
    sink: mpsc::Sender<EventMsg>,
    rx: broadcast::Receiver<Bytes>,
    session: Arc<lmux_term::PtySession>,
    needs_resync: bool,
    pending_resync: Option<PendingResync>,
    ending: bool,
}

#[derive(Default)]
pub struct SubRegistry {
    subs: HashMap<String, SubEntry>,
}

impl SubRegistry {
    pub fn add(
        &mut self,
        sub_id: &str,
        agent: &AgentId,
        sink: mpsc::Sender<EventMsg>,
        rx: broadcast::Receiver<Bytes>,
        session: Arc<lmux_term::PtySession>,
    ) {
        self.subs.insert(
            sub_id.to_string(),
            SubEntry {
                agent: agent.clone(),
                sink,
                rx,
                session,
                needs_resync: false,
                pending_resync: None,
                ending: false,
            },
        );
    }

    pub fn remove(&mut self, sub_id: &str) {
        self.subs.remove(sub_id);
    }

    pub fn remove_agent(&mut self, agent: &AgentId) {
        self.subs.retain(|_, e| &e.agent != agent);
    }

    /// 标记 agent 结束；pump 会在 sink 可写时先发 term.exit，再删除订阅。
    pub fn mark_agent_exit(&mut self, agent: &AgentId) {
        for entry in self.subs.values_mut().filter(|e| &e.agent == agent) {
            entry.ending = true;
            entry.pending_resync = None;
        }
    }

    /// 一次泵：队列满/Lagged 不是可忽略条件；停止增量并在队列可写时发送完整 replay resync。
    pub async fn pump_once(&mut self) {
        let mut remove = Vec::new();
        for (id, entry) in self.subs.iter_mut() {
            if entry.ending {
                let msg = EventMsg::new(
                    lmux_core::protocol::events::TERM_EXIT,
                    serde_json::to_value(lmux_core::protocol::TermExitEvent {
                        agent: entry.agent.clone(),
                    })
                    .unwrap_or_default(),
                );
                match entry.sink.try_send(msg) {
                    Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => remove.push(id.clone()),
                    Err(mpsc::error::TrySendError::Full(_)) => {}
                }
                continue;
            }

            if entry.needs_resync {
                if entry.pending_resync.is_none() {
                    let (snapshot, rx) = entry.session.subscribe();
                    entry.pending_resync = Some(PendingResync {
                        replay_b64: lmux_term::b64_encode(&snapshot),
                        rx,
                    });
                }
                let pending = entry.pending_resync.as_ref().expect("created above");
                let msg = EventMsg::new(
                    lmux_core::protocol::events::TERM_RESYNC,
                    serde_json::to_value(TermResyncEvent {
                        agent: entry.agent.clone(),
                        replay_b64: pending.replay_b64.clone(),
                    })
                    .unwrap_or_default(),
                );
                match entry.sink.try_send(msg) {
                    Ok(()) => {
                        let pending = entry.pending_resync.take().expect("pending resync");
                        entry.rx = pending.rx;
                        entry.needs_resync = false;
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => continue,
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        remove.push(id.clone());
                        continue;
                    }
                }
            }

            match entry.rx.try_recv() {
                Ok(bytes) => {
                    let msg = EventMsg::new(
                        lmux_core::protocol::events::TERM_DATA,
                        serde_json::to_value(TermDataEvent {
                            agent: entry.agent.clone(),
                            data_b64: lmux_term::b64_encode(&bytes),
                        })
                        .unwrap_or_default(),
                    );
                    match entry.sink.try_send(msg) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => entry.needs_resync = true,
                        Err(mpsc::error::TrySendError::Closed(_)) => remove.push(id.clone()),
                    }
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => entry.needs_resync = true,
                Err(broadcast::error::TryRecvError::Closed) => remove.push(id.clone()),
                Err(broadcast::error::TryRecvError::Empty) => {}
            }
        }
        for id in remove {
            self.subs.remove(&id);
        }
    }

    pub fn len(&self) -> usize {
        self.subs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmux_core::model::AgentType;

    #[tokio::test]
    async fn full_sink_recovers_with_explicit_resync() {
        let agent = "shell_resync".to_string();
        let session = lmux_term::PtySession::spawn(lmux_term::LaunchCfg {
            agent: agent.clone(),
            agent_type: AgentType::Shell,
            cwd: std::env::temp_dir(),
            env: vec![],
            program_override: Some("bash".into()),
            args: vec!["-c".into(), "sleep .1; echo RESYNC-MARK; sleep 1".into()],
            cols: 80,
            rows: 24,
            tmux_session: None,
        })
        .unwrap();
        let (_, rx) = session.subscribe();
        let (tx, mut sink_rx) = mpsc::channel(1);
        // 预填满 connection queue。
        tx.try_send(EventMsg::new("dummy", serde_json::json!({})))
            .unwrap();
        let mut registry = SubRegistry::default();
        registry.add("s1", &agent, tx, rx, Arc::clone(&session));
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        registry.pump_once().await; // term.data -> Full => needs_resync
        assert_eq!(sink_rx.recv().await.unwrap().event, "dummy"); // 释放队列
        registry.pump_once().await; // 发送 resync
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), sink_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(msg.event, lmux_core::protocol::events::TERM_RESYNC);
        let event: TermResyncEvent = serde_json::from_value(msg.params).unwrap();
        let replay = lmux_term::b64_decode(&event.replay_b64).unwrap();
        assert!(replay.windows(11).any(|w| w == b"RESYNC-MARK"));
        session.kill();
    }

    #[tokio::test]
    async fn ending_agent_emits_term_exit_before_subscription_cleanup() {
        let agent = "shell_exit_notice".to_string();
        let session = lmux_term::PtySession::spawn(lmux_term::LaunchCfg {
            agent: agent.clone(),
            agent_type: AgentType::Shell,
            cwd: std::env::temp_dir(),
            env: vec![],
            program_override: Some("bash".into()),
            args: vec!["-c".into(), "sleep 1".into()],
            cols: 80,
            rows: 24,
            tmux_session: None,
        })
        .unwrap();
        let (_, rx) = session.subscribe();
        let (tx, mut sink_rx) = mpsc::channel(2);
        let mut registry = SubRegistry::default();
        registry.add("ending", &agent, tx, rx, Arc::clone(&session));

        registry.mark_agent_exit(&agent);
        registry.pump_once().await;

        let msg = sink_rx.recv().await.unwrap();
        assert_eq!(msg.event, lmux_core::protocol::events::TERM_EXIT);
        let event: lmux_core::protocol::TermExitEvent = serde_json::from_value(msg.params).unwrap();
        assert_eq!(event.agent, agent);
        assert!(registry.is_empty());
        session.kill();
    }
}
