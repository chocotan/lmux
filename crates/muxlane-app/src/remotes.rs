//! Remote host connection, bootstrap, deletion, and agent/project flows.
use crate::app::{ConnectAuthMode, DeleteConfirm, DeleteTarget, MuxlaneApp};
use gpui::{AppContext, Context, Window};
use std::sync::Arc;

impl MuxlaneApp {
    pub(crate) fn add_remote_target(&mut self, target: String, cx: &mut Context<Self>) {
        let target = target.trim().to_string();
        if target.is_empty() {
            self.dialog_error = Some("请输入 SSH host 或别名".into());
            cx.notify();
            return;
        }
        let parsed = muxlane_client::parse_target(&target);
        let name = match &parsed {
            muxlane_client::Target::Socket(path) => {
                path.rsplit('/').next().unwrap_or(path).to_string()
            }
            muxlane_client::Target::Ssh { host, .. } => host.clone(),
        };
        if let Some(index) = self.remotes.iter().position(|host| host.cfg.name == name) {
            self.remotes[index].stop();
            let release_name = name.clone();
            self.server.rt_spawn(async move {
                muxlane_client::release_remote_tunnel(&release_name).await;
            });
            self.remotes.remove(index);
            self.remote_snaps.remove(&name);
            self.remote_states.remove(&name);
        }
        let username = self.connect_username.read(cx).text();
        let auth = match self.connect_auth_mode {
            ConnectAuthMode::SshConfig => muxlane_client::SshAuth::SshConfig,
            ConnectAuthMode::PublicKey => muxlane_client::SshAuth::PublicKey {
                username: (!username.trim().is_empty()).then(|| username.trim().to_string()),
                identity_file: {
                    let path = self.connect_key_path.read(cx).text();
                    (!path.trim().is_empty()).then(|| path.trim().to_string())
                },
            },
            ConnectAuthMode::Password => {
                let password = self.connect_password.read(cx).text();
                if username.trim().is_empty() || password.is_empty() {
                    self.dialog_error = Some("密码连接需要用户名和密码".into());
                    cx.notify();
                    return;
                }
                muxlane_client::SshAuth::Password {
                    username: username.trim().to_string(),
                    password,
                }
            }
        };
        let host = muxlane_client::RemoteHost::new(
            muxlane_client::HostCfg {
                name,
                target: parsed,
                auth,
                retry_base_ms: 500,
            },
            self.remote_event_tx.clone(),
        );
        self.server.rt_spawn(Arc::clone(&host).run_loop());
        self.remotes.push(host);
        self.persist();
        self.connect_dialog = false;
        self.dialog_error = None;
        self.connect_input.update(cx, |input, cx| input.reset(cx));
        self.connect_username
            .update(cx, |input, cx| input.reset(cx));
        self.connect_password
            .update(cx, |input, cx| input.reset(cx));
        self.connect_key_path
            .update(cx, |input, cx| input.reset(cx));
        cx.notify();
    }

    pub(crate) fn begin_delete(&mut self, target: DeleteTarget, cx: &mut Context<Self>) {
        let affected_sessions = match &target {
            DeleteTarget::LocalProject { project, .. } => self
                .last_snapshot
                .agents
                .iter()
                .filter(|agent| &agent.project == project)
                .count(),
            DeleteTarget::RemoteProject { host, project, .. } => self
                .remote_snaps
                .get(host)
                .map(|snapshot| {
                    snapshot
                        .agents
                        .iter()
                        .filter(|agent| &agent.project == project)
                        .count()
                })
                .unwrap_or(0),
            DeleteTarget::RemoteMachine { .. } => 0,
        };
        self.tree_menu = None;
        self.delete_error = None;
        self.delete_confirm = Some(DeleteConfirm {
            target,
            affected_sessions,
        });
        cx.notify();
    }

    pub(crate) fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        if self.delete_busy {
            return;
        }
        let Some(confirm) = self.delete_confirm.clone() else {
            return;
        };
        self.delete_busy = true;
        match confirm.target {
            DeleteTarget::LocalProject { project, .. } => {
                let server = Arc::clone(&self.server);
                let project_for_delete = project.clone();
                let affected_agents: Vec<_> = self
                    .last_snapshot
                    .agents
                    .iter()
                    .filter(|agent| agent.project == project)
                    .map(|agent| agent.id.clone())
                    .collect();
                cx.spawn(async move |this, cx| {
                    let (result, snapshot) = cx
                        .background_spawn(async move {
                            let result = server.delete_project(&project_for_delete).await;
                            let snapshot = server.snapshot().await;
                            (result, snapshot)
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.last_snapshot = snapshot;
                        match result {
                            Ok(result) => {
                                this.cleanup_removed_agents(&result.destroyed_agents, cx);
                                if result.failed_agents.is_empty() {
                                    this.delete_confirm = None;
                                } else {
                                    this.delete_error = Some(format!(
                                        "{} 个 tmux 会话未能销毁，项目仍保留",
                                        result.failed_agents.len()
                                    ));
                                }
                            }
                            Err(error) => {
                                let removed: Vec<_> = affected_agents
                                    .iter()
                                    .filter(|agent| this.last_snapshot.agent(agent).is_none())
                                    .cloned()
                                    .collect();
                                this.cleanup_removed_agents(&removed, cx);
                                this.delete_error = Some(error.to_string());
                            }
                        }
                        this.delete_busy = false;
                        this.persist();
                        cx.notify();
                    });
                })
                .detach();
            }
            DeleteTarget::RemoteProject { host, project, .. } => {
                let remote = self
                    .remotes
                    .iter()
                    .find(|remote| remote.cfg.name == host)
                    .cloned();
                let Some(remote) = remote else {
                    self.delete_error = Some("远端当前不可连接，未执行删除".into());
                    self.delete_busy = false;
                    cx.notify();
                    return;
                };
                let project_for_rpc = project.clone();
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_spawn(
                            async move { remote.delete_project(&project_for_rpc).await },
                        )
                        .await;
                    let _ = this.update(cx, |this, cx| match result {
                        Ok(result) => {
                            if let Some(snapshot) = this.remote_snaps.get_mut(&host) {
                                if result.failed_agents.is_empty() {
                                    snapshot.projects.retain(|item| item.id != project);
                                }
                                snapshot
                                    .agents
                                    .retain(|agent| !result.destroyed_agents.contains(&agent.id));
                            }
                            this.cleanup_removed_agents(&result.destroyed_agents, cx);
                            if result.failed_agents.is_empty() {
                                this.delete_confirm = None;
                            } else {
                                this.delete_error = Some(format!(
                                    "{} 个远端 tmux 会话未能销毁，项目仍保留",
                                    result.failed_agents.len()
                                ));
                            }
                            this.delete_busy = false;
                            this.persist();
                            cx.notify();
                        }
                        Err(error) => {
                            this.delete_error = Some(error.to_string());
                            this.delete_busy = false;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            DeleteTarget::RemoteMachine { host } => {
                let removed_agents: Vec<_> = self
                    .remote_snaps
                    .get(&host)
                    .map(|snapshot| {
                        snapshot
                            .agents
                            .iter()
                            .map(|agent| agent.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(remote) = self
                    .remotes
                    .iter()
                    .find(|remote| remote.cfg.name == host)
                    .cloned()
                {
                    remote.stop();
                }
                let release_host = host.clone();
                self.server.rt_spawn(async move {
                    muxlane_client::release_remote_tunnel(&release_host).await;
                });
                self.remotes.retain(|remote| remote.cfg.name != host);
                self.remote_snaps.remove(&host);
                self.remote_states.remove(&host);
                self.cleanup_removed_agents(&removed_agents, cx);
                self.delete_confirm = None;
                self.delete_busy = false;
                self.persist();
                cx.notify();
            }
        }
    }

    pub(crate) fn cancel_bootstrap_for_host(&mut self, host: &str, cx: &mut Context<Self>) {
        if let Some(remote) = self.remotes.iter().find(|r| r.cfg.name == host) {
            remote.cancel_bootstrap();
        }
        self.bootstrap_progress.remove(host);
        if self.bootstrap_confirm.as_ref().map(|c| c.host.as_str()) == Some(host) {
            self.bootstrap_confirm = None;
            self.bootstrap_error = None;
        }
        cx.notify();
    }

    pub(crate) fn confirm_bootstrap(&mut self, cx: &mut Context<Self>) {
        let Some(confirm) = self.bootstrap_confirm.clone() else {
            return;
        };
        let Some(remote) = self
            .remotes
            .iter()
            .find(|remote| remote.cfg.name == confirm.host)
            .cloned()
        else {
            self.bootstrap_error = Some("远程机器已不存在".into());
            cx.notify();
            return;
        };
        let host_name = confirm.host.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if confirm.upgrade {
                        remote.upgrade_and_retry().await
                    } else if confirm.install {
                        remote.install_and_start().await
                    } else {
                        remote
                            .start_and_retry(confirm.binary.as_deref().unwrap_or("muxlane"))
                            .await
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.bootstrap_progress.remove(&host_name);
                match result {
                    Ok(()) => {
                        this.bootstrap_confirm = None;
                        this.bootstrap_error = None;
                        cx.notify();
                    }
                    Err(error) => {
                        let error = error.to_string();
                        if error.contains("已取消") {
                            this.bootstrap_confirm = None;
                            this.bootstrap_error = None;
                        } else {
                            this.bootstrap_error = Some(error);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn spawn_remote_agent(
        &mut self,
        host: String,
        project: String,
        preset: Option<muxlane_core::AgentPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote = self
            .remotes
            .iter()
            .find(|remote| remote.cfg.name == host)
            .cloned();
        let Some(remote) = remote else {
            return;
        };
        let target_project = project.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { remote.spawn_agent(&project, preset.as_ref()).await },
                )
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(agent) => {
                    let agent_id = agent.id.clone();
                    if let Some(snapshot) = this.remote_snaps.get_mut(&host) {
                        if let Some(project) = snapshot
                            .projects
                            .iter_mut()
                            .find(|project| project.id == agent.project)
                        {
                            project.agents.push(agent_id.clone());
                        }
                        snapshot.agents.push(agent);
                    }
                    let remote_project_key = format!("remote:{}:{}", host, target_project);
                    this.collapsed_projects.remove(&remote_project_key);
                    let remote_machine_key = format!("machine:remote:{}", host);
                    this.collapsed_machines.remove(&remote_machine_key);
                    this.open_remote_agent(&agent_id, cx);
                    this.focus_agent(&agent_id, window, cx);
                    this.persist();
                    cx.notify();
                }
                Err(error) => {
                    let text = error.to_string();
                    this.notifications.update(cx, |center, cx| {
                        center.show_error(format!("远程创建会话失败：{text}"), cx)
                    });
                    // 类型不匹配通常意味着远端仍在运行旧版 Muxlane，
                    // 直接切换到已有的更新引导状态。
                    if text.contains("远端 Muxlane 版本过旧") {
                        if let Some(remote) = this
                            .remotes
                            .iter()
                            .find(|remote| remote.cfg.name == host)
                            .cloned()
                        {
                            cx.spawn(async move |_, _| {
                                remote.mark_needs_upgrade().await;
                            })
                            .detach();
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn submit_remote_project(
        &mut self,
        host: String,
        path: String,
        cx: &mut Context<Self>,
    ) {
        let remote = self
            .remotes
            .iter()
            .find(|remote| remote.cfg.name == host)
            .cloned();
        let Some(remote) = remote else {
            self.dialog_error = Some("远端尚未连接".into());
            cx.notify();
            return;
        };
        if path.trim().is_empty() {
            self.dialog_error = Some("请输入远端已有目录".into());
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { remote.add_project(path.trim()).await })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(project) => {
                    if let Some(snapshot) = this.remote_snaps.get_mut(&host) {
                        if !snapshot.projects.iter().any(|item| item.id == project.id) {
                            snapshot.projects.push(project);
                        }
                    }
                    this.remote_project_dialog = None;
                    this.dialog_error = None;
                    this.persist();
                    cx.notify();
                }
                Err(error) => {
                    let text = error.to_string();
                    if text.contains("unknown_method")
                        && text.contains(muxlane_core::protocol::features::PROJECT_ADD)
                    {
                        // 旧版远端没有 project.add：转入升级引导，而不是弹原始错误
                        this.remote_project_dialog = None;
                        this.dialog_error = None;
                        if let Some(remote) = this
                            .remotes
                            .iter()
                            .find(|remote| remote.cfg.name == host)
                            .cloned()
                        {
                            cx.spawn(async move |this, cx| {
                                remote.mark_needs_upgrade().await;
                                let _ = this.update(cx, |_, _| {});
                            })
                            .detach();
                        }
                    } else {
                        this.dialog_error = Some(text);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
