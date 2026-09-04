use crate::MuxlaneServer;
use anyhow::Context as _;
use muxlane_core::model::{AgentId, AgentInstance, AgentStatus, Project, Snapshot};
use muxlane_core::protocol::{AgentSpawnParams, DeleteScopeResult, EventMsg, ProjectAddParams};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

impl MuxlaneServer {
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<EventMsg> {
        self.events.subscribe()
    }

    pub fn subscribe_dirty(&self) -> tokio::sync::watch::Receiver<u64> {
        self.dirty.subscribe()
    }

    pub async fn snapshot(&self) -> Snapshot {
        self.state.read().await.snapshot()
    }

    pub fn try_session(&self, agent: &AgentId) -> Option<Arc<muxlane_term::PtySession>> {
        self.sessions
            .try_lock()
            .ok()
            .and_then(|sessions| sessions.get(agent).cloned())
    }

    pub async fn session(&self, agent: &AgentId) -> Option<Arc<muxlane_term::PtySession>> {
        self.sessions.lock().await.get(agent).cloned()
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    pub async fn subscription_count(&self) -> usize {
        self.subs.lock().await.len()
    }

    pub async fn spawn_agent(&self, params: AgentSpawnParams) -> anyhow::Result<AgentInstance> {
        let _lifecycle = self.lifecycle.lock().await;
        let project = self
            .state
            .read()
            .await
            .projects
            .iter()
            .find(|project| project.id == params.project)
            .cloned()
            .with_context(|| format!("no such project: {}", params.project))?;
        let agent_type = params
            .agent_type
            .unwrap_or(muxlane_core::model::AgentType::Shell);
        let agent_id = muxlane_core::model::new_id(agent_type.as_str());
        let tmux_name = format!("muxlane-{agent_id}");
        let title = params.preset_name.unwrap_or_else(|| {
            if agent_type == muxlane_core::model::AgentType::Shell {
                muxlane_term::default_shell_program()
                    .rsplit('/')
                    .next()
                    .unwrap_or("shell")
                    .into()
            } else {
                agent_type.as_str().to_string()
            }
        });
        let mut cfg =
            if agent_type == muxlane_core::model::AgentType::Shell && params.program.is_none() {
                muxlane_term::LaunchCfg::shell(agent_id.clone(), project.path.clone())
            } else {
                muxlane_term::LaunchCfg {
                    agent: agent_id.clone(),
                    agent_type,
                    cwd: project.path.clone(),
                    env: params.env.unwrap_or_default(),
                    program_override: Some(
                        params
                            .program
                            .unwrap_or_else(|| agent_type.as_str().to_string()),
                    ),
                    args: params.args.unwrap_or_default(),
                    cols: 120,
                    rows: 32,
                    tmux_session: Some(tmux_name.clone()),
                }
            };
        cfg.tmux_session = Some(tmux_name);
        cfg.env.push(("MUXLANE_AGENT_ID".into(), agent_id.clone()));
        cfg.env.push((
            "MUXLANE_SOCKET".into(),
            self.socket_path.display().to_string(),
        ));
        cfg.env
            .push(("MUXLANE_HOOK_TOKEN".into(), self.hook_token(&agent_id)));

        let session = tokio::task::spawn_blocking(move || muxlane_term::PtySession::spawn(cfg))
            .await
            .context("spawn agent task")?
            .context("spawn agent")?;
        let instance = AgentInstance {
            id: agent_id.clone(),
            project: project.id.clone(),
            agent_type,
            title,
            status: AgentStatus::Idle,
            status_since: muxlane_core::model::now_secs(),
            seen: true,
            tmux_session: session.tmux_session_name().map(str::to_string),
        };
        self.sessions
            .lock()
            .await
            .insert(agent_id, Arc::clone(&session));
        self.state
            .write()
            .await
            .add_agent(project, instance.clone());
        self.dirty.bump();
        self.persist_runtime_state().await?;
        Ok(instance)
    }

    pub async fn delete_agent(&self, agent: &AgentId) -> anyhow::Result<DeleteScopeResult> {
        let (destroyed_agents, failed_agents) =
            self.destroy_sessions(std::slice::from_ref(agent)).await;
        if !destroyed_agents.is_empty() {
            let mut state = self.state.write().await;
            for agent in &destroyed_agents {
                state.remove_agent(agent);
            }
            drop(state);
            self.dirty.bump();
            self.persist_runtime_state().await?;
        }
        Ok(DeleteScopeResult {
            destroyed_agents,
            failed_agents,
        })
    }

    pub async fn add_project(&self, params: ProjectAddParams) -> anyhow::Result<Project> {
        let path = std::path::PathBuf::from(&params.path)
            .canonicalize()
            .with_context(|| format!("invalid project path: {}", params.path))?;
        anyhow::ensure!(path.is_dir(), "invalid project path: {}", params.path);
        let name = params
            .name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            });
        let project = Project {
            id: muxlane_core::model::new_id("project"),
            name,
            path,
            branch: None,
            agents: vec![],
        };
        if self.state.write().await.add_project(project.clone()) {
            self.dirty.bump();
            self.persist_runtime_state().await?;
        }
        Ok(project)
    }

    pub async fn delete_project(
        &self,
        project: &muxlane_core::model::ProjectId,
    ) -> anyhow::Result<DeleteScopeResult> {
        let _lifecycle = self.lifecycle.lock().await;
        let agents: Vec<_> = self
            .state
            .read()
            .await
            .agents
            .iter()
            .filter(|agent| &agent.project == project)
            .map(|agent| agent.id.clone())
            .collect();
        let (destroyed_agents, failed_agents) = self.destroy_sessions(&agents).await;
        {
            let mut state = self.state.write().await;
            for agent in &destroyed_agents {
                state.remove_agent(agent);
            }
            if failed_agents.is_empty() {
                state.projects.retain(|item| &item.id != project);
            }
        }
        self.dirty.bump();
        self.persist_runtime_state().await?;
        Ok(DeleteScopeResult {
            destroyed_agents,
            failed_agents,
        })
    }

    pub async fn mark_seen(&self, agent: &AgentId) {
        if !self.state.write().await.mark_seen(agent).is_empty() {
            self.dirty.bump();
        }
    }

    pub async fn mark_working(&self, agent: &AgentId) {
        if !self
            .state
            .write()
            .await
            .mark_screen_working(agent)
            .is_empty()
        {
            self.dirty.bump();
        }
    }

    pub async fn persist(&self) -> anyhow::Result<()> {
        self.persist_runtime_state().await
    }

    pub async fn restore_agent(
        &self,
        project: Project,
        instance: AgentInstance,
        session: Arc<muxlane_term::PtySession>,
    ) {
        self.sessions
            .lock()
            .await
            .insert(instance.id.clone(), session);
        self.state.write().await.add_agent(project, instance);
        self.dirty.bump();
    }

    async fn destroy_sessions(&self, agents: &[AgentId]) -> (Vec<AgentId>, Vec<AgentId>) {
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
                let session = Arc::clone(session);
                let killed = tokio::task::spawn_blocking(move || session.kill_persistent())
                    .await
                    .unwrap_or(false);
                if killed {
                    destroyed.push(agent.clone());
                } else {
                    failed.push(agent.clone());
                }
            } else {
                destroyed.push(agent.clone());
            }
        }
        if !failed.is_empty() {
            let mut live = self.sessions.lock().await;
            for agent in &failed {
                if let Some(session) = sessions.get(agent) {
                    live.insert(agent.clone(), Arc::clone(session));
                }
            }
        }
        let mut subs = self.subs.lock().await;
        for agent in &destroyed {
            subs.mark_agent_exit(agent);
        }
        (destroyed, failed)
    }
}
