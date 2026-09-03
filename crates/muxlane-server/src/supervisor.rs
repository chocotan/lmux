use crate::MuxlaneServer;
use muxlane_core::detect::ScreenInput;
use muxlane_core::model::{AgentId, AgentInstance, AgentStatus, Project};
use muxlane_store::PersistedApp;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

impl MuxlaneServer {
    pub fn start_supervisor(self: &Arc<Self>) {
        let server = Arc::clone(self);
        self.rt_spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                server.maintain_sessions().await;
            }
        });
    }

    pub async fn restore_sessions(&self, persisted: &PersistedApp) {
        for saved in &persisted.sessions {
            if !tmux_session_alive(&saved.tmux_session) {
                continue;
            }
            let Some(project) = persisted
                .projects
                .iter()
                .find(|project| project.id == saved.project_id)
                .cloned()
            else {
                continue;
            };
            if let Ok((instance, session)) = self.attach_tmux(
                &saved.agent_id,
                saved.agent_type,
                &saved.title,
                &saved.tmux_session,
                &project,
            ) {
                self.restore_agent(project, instance, session).await;
            }
        }
    }

    pub async fn run_headless_persistence(&self, path: PathBuf, mut previous: PersistedApp) {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let persisted =
                PersistedApp::from_snapshot(&self.snapshot().await).with_ui_prefs_from(&previous);
            if let Err(error) = muxlane_store::save(&path, &persisted) {
                tracing::warn!(%error, "persist headless state failed");
            }
            previous = persisted;
        }
    }

    pub async fn maintain_sessions(&self) {
        let sessions: Vec<_> = self
            .sessions
            .lock()
            .await
            .iter()
            .map(|(id, session)| (id.clone(), Arc::clone(session)))
            .collect();
        let mut exited = Vec::new();
        let mut recovered = Vec::new();
        let mut screens = Vec::new();

        for (id, session) in sessions {
            if session.try_take_exit().is_some() {
                if let Some(name) = session.tmux_session_name() {
                    if tmux_session_alive(name) {
                        recovered.push((id, name.to_string()));
                        continue;
                    }
                }
                exited.push(id);
                continue;
            }
            let replay = session.replay_snapshot();
            let tail = &replay[replay.len().saturating_sub(64 * 1024)..];
            let mut lines = muxlane_core::protocol::strip_ansi(tail);
            if lines.len() > 8 {
                lines = lines.split_off(lines.len() - 8);
            }
            screens.push((
                id,
                ScreenInput {
                    bottom_lines: lines,
                    osc_title: muxlane_core::protocol::extract_osc_title(tail),
                    secs_since_output: None,
                    bell: tail.last() == Some(&0x07),
                },
            ));
        }

        for (id, tmux_session) in recovered {
            if let Err(error) = self.recover_session(&id, tmux_session).await {
                tracing::warn!(agent = %id, %error, "reattach live tmux session failed");
            }
        }
        self.remove_exited_sessions(&exited).await;
        self.observe_screens(&screens).await;
    }

    async fn recover_session(&self, id: &AgentId, tmux_session: String) -> anyhow::Result<()> {
        let (agent, project) = {
            let state = self.state.read().await;
            let agent = state
                .agents
                .iter()
                .find(|agent| &agent.id == id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("agent state missing"))?;
            let project = state
                .projects
                .iter()
                .find(|project| project.id == agent.project)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("project state missing"))?;
            (agent, project)
        };
        let (_, session) = self.attach_tmux(
            &agent.id,
            agent.agent_type,
            &agent.title,
            &tmux_session,
            &project,
        )?;
        self.sessions.lock().await.insert(id.clone(), session);
        Ok(())
    }

    fn attach_tmux(
        &self,
        agent_id: &AgentId,
        agent_type: muxlane_core::model::AgentType,
        title: &str,
        tmux_session: &str,
        project: &Project,
    ) -> anyhow::Result<(AgentInstance, Arc<muxlane_term::PtySession>)> {
        let cfg = muxlane_term::LaunchCfg {
            agent: agent_id.clone(),
            agent_type,
            cwd: project.path.clone(),
            env: vec![
                ("MUXLANE_AGENT_ID".into(), agent_id.clone()),
                (
                    "MUXLANE_SOCKET".into(),
                    self.socket_path().display().to_string(),
                ),
                ("MUXLANE_HOOK_TOKEN".into(), self.hook_token(agent_id)),
            ],
            program_override: None,
            args: vec![],
            cols: 120,
            rows: 32,
            tmux_session: Some(tmux_session.to_string()),
        };
        let session = muxlane_term::PtySession::spawn(cfg)?;
        let instance = AgentInstance {
            id: agent_id.clone(),
            project: project.id.clone(),
            agent_type,
            title: title.to_string(),
            status: AgentStatus::Idle,
            status_since: muxlane_core::model::now_secs(),
            seen: true,
            tmux_session: Some(tmux_session.to_string()),
        };
        Ok((instance, session))
    }

    async fn remove_exited_sessions(&self, exited: &[AgentId]) {
        if exited.is_empty() {
            return;
        }
        let mut sessions = self.sessions.lock().await;
        for id in exited {
            sessions.remove(id);
        }
        drop(sessions);
        let mut subs = self.subs.lock().await;
        for id in exited {
            subs.mark_agent_exit(id);
        }
        drop(subs);
        let mut state = self.state.write().await;
        for id in exited {
            for event in state.agent_exit(id) {
                let _ = self.events.send(event);
            }
        }
        drop(state);
        self.dirty.bump();
    }

    async fn observe_screens(&self, screens: &[(AgentId, ScreenInput)]) {
        if screens.is_empty() {
            return;
        }
        let mut state = self.state.write().await;
        let mut changed = false;
        for (id, screen) in screens {
            if !state.observe_screen(id, screen).is_empty() {
                changed = true;
            }
        }
        drop(state);
        if changed {
            self.dirty.bump();
        }
    }
}

fn tmux_session_alive(name: &str) -> bool {
    let target = format!("={name}");
    std::process::Command::new("tmux")
        .args(["-L", "muxlane", "has-session", "-t", &target])
        .status()
        .is_ok_and(|status| status.success())
}
