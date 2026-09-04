//! 订阅注册表：term.data 无损边界；lag/backpressure 后显式 term.resync。
use bytes::Bytes;
use muxlane_core::model::AgentId;
use muxlane_core::protocol::{EventMsg, TermDataEvent, TermResyncEvent};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

const MAX_FRAMES_PER_TICK: usize = 64;
const RESYNC_MIN_INTERVAL: Duration = Duration::from_secs(1);

struct PendingResync {
    replay_b64: String,
    rx: broadcast::Receiver<Bytes>,
}

struct SubEntry {
    agent: AgentId,
    sink: mpsc::Sender<EventMsg>,
    rx: broadcast::Receiver<Bytes>,
    session: Arc<muxlane_term::PtySession>,
    needs_resync: bool,
    pending_resync: Option<PendingResync>,
    last_resync: Option<Instant>,
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
        session: Arc<muxlane_term::PtySession>,
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
                last_resync: None,
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
                    muxlane_core::protocol::events::TERM_EXIT,
                    serde_json::to_value(muxlane_core::protocol::TermExitEvent {
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
                    if entry
                        .last_resync
                        .is_some_and(|last| last.elapsed() < RESYNC_MIN_INTERVAL)
                    {
                        continue;
                    }
                    let (snapshot, rx) = entry.session.subscribe();
                    entry.pending_resync = Some(PendingResync {
                        replay_b64: muxlane_core::protocol::b64_encode(&snapshot),
                        rx,
                    });
                }
                let pending = entry.pending_resync.as_ref().expect("created above");
                let msg = EventMsg::new(
                    muxlane_core::protocol::events::TERM_RESYNC,
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
                        entry.last_resync = Some(Instant::now());
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => continue,
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        remove.push(id.clone());
                        continue;
                    }
                }
            }

            for _ in 0..MAX_FRAMES_PER_TICK {
                match entry.rx.try_recv() {
                    Ok(bytes) => {
                        let msg = EventMsg::new(
                            muxlane_core::protocol::events::TERM_DATA,
                            serde_json::to_value(TermDataEvent {
                                agent: entry.agent.clone(),
                                data_b64: muxlane_core::protocol::b64_encode(&bytes),
                            })
                            .unwrap_or_default(),
                        );
                        match entry.sink.try_send(msg) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                entry.needs_resync = true;
                                break;
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                remove.push(id.clone());
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        entry.needs_resync = true;
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Closed) => {
                        remove.push(id.clone());
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Empty) => break,
                }
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
    use muxlane_core::model::AgentType;

    fn idle_session(agent: &str) -> Arc<muxlane_term::PtySession> {
        muxlane_term::PtySession::spawn(muxlane_term::LaunchCfg {
            agent: agent.into(),
            agent_type: AgentType::Shell,
            cwd: std::env::temp_dir(),
            env: vec![],
            program_override: Some("bash".into()),
            args: vec!["-c".into(), "sleep 2".into()],
            cols: 80,
            rows: 24,
            tmux_session: None,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn pump_drains_high_frequency_frames_up_to_budget() {
        let agent = "shell_burst".to_string();
        let session = idle_session(&agent);
        let (tap_tx, rx) = broadcast::channel(128);
        let (sink, mut sink_rx) = mpsc::channel(MAX_FRAMES_PER_TICK);
        let mut registry = SubRegistry::default();
        registry.subs.insert(
            "burst".into(),
            SubEntry {
                agent: agent.clone(),
                sink,
                rx,
                session: Arc::clone(&session),
                needs_resync: false,
                pending_resync: None,
                last_resync: None,
                ending: false,
            },
        );
        for byte in 0..MAX_FRAMES_PER_TICK {
            tap_tx.send(Bytes::from(vec![byte as u8])).unwrap();
        }

        registry.pump_once().await;

        for expected in 0..MAX_FRAMES_PER_TICK {
            let message = sink_rx.try_recv().expect("burst frame");
            assert_eq!(message.event, muxlane_core::protocol::events::TERM_DATA);
            let event: TermDataEvent = serde_json::from_value(message.params).unwrap();
            assert_eq!(
                muxlane_core::protocol::b64_decode(&event.data_b64).unwrap(),
                vec![expected as u8]
            );
        }
        assert!(sink_rx.try_recv().is_err());
        session.kill();
    }

    #[tokio::test]
    async fn repeated_lag_is_coalesced_within_resync_interval() {
        let agent = "shell_resync_throttle".to_string();
        let session = idle_session(&agent);
        let (tap_tx, rx) = broadcast::channel(1);
        let (sink, mut sink_rx) = mpsc::channel(8);
        let mut registry = SubRegistry::default();
        registry.subs.insert(
            "throttled".into(),
            SubEntry {
                agent,
                sink,
                rx,
                session: Arc::clone(&session),
                needs_resync: false,
                pending_resync: None,
                last_resync: None,
                ending: false,
            },
        );
        tap_tx.send(Bytes::from_static(b"one")).unwrap();
        tap_tx.send(Bytes::from_static(b"two")).unwrap();

        registry.pump_once().await;
        registry.pump_once().await;
        let first = sink_rx.try_recv().expect("initial resync");
        assert_eq!(first.event, muxlane_core::protocol::events::TERM_RESYNC);

        registry
            .subs
            .get_mut("throttled")
            .expect("subscription")
            .needs_resync = true;
        for _ in 0..10 {
            registry.pump_once().await;
        }
        assert!(sink_rx.try_recv().is_err(), "resync should be coalesced");
        session.kill();
    }

    #[tokio::test]
    async fn full_sink_recovers_with_explicit_resync() {
        let agent = "shell_resync".to_string();
        let session = muxlane_term::PtySession::spawn(muxlane_term::LaunchCfg {
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
        assert_eq!(msg.event, muxlane_core::protocol::events::TERM_RESYNC);
        let event: TermResyncEvent = serde_json::from_value(msg.params).unwrap();
        let replay = muxlane_core::protocol::b64_decode(&event.replay_b64).unwrap();
        assert!(replay.windows(11).any(|w| w == b"RESYNC-MARK"));
        session.kill();
    }

    #[tokio::test]
    async fn ending_agent_emits_term_exit_before_subscription_cleanup() {
        let agent = "shell_exit_notice".to_string();
        let session = muxlane_term::PtySession::spawn(muxlane_term::LaunchCfg {
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
        assert_eq!(msg.event, muxlane_core::protocol::events::TERM_EXIT);
        let event: muxlane_core::protocol::TermExitEvent =
            serde_json::from_value(msg.params).unwrap();
        assert_eq!(event.agent, agent);
        assert!(registry.is_empty());
        session.kill();
    }
}
