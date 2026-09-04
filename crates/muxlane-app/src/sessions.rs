//! Agent session opening, focus, deletion, and terminal caching.
use crate::app::palette::NewSessionTarget;
use crate::app::MuxlaneApp;
use crate::i18n;
use crate::term_view::TermView;
use crate::theme::Theme;
use gpui::{AppContext, Context, Entity, Focusable, Window};
use muxlane_core::model::AgentId;
use muxlane_term::VTerm;
use std::sync::Arc;

impl MuxlaneApp {
    pub(crate) fn mark_agent_working(&mut self, agent: &AgentId, cx: &mut Context<Self>) {
        let mut local = false;
        if let Some(a) = self.last_snapshot.agent_mut(agent) {
            local = true;
            if a.status != muxlane_core::model::AgentStatus::Working {
                a.status = muxlane_core::model::AgentStatus::Working;
                a.status_since = muxlane_core::model::now_secs();
                cx.notify();
            }
        } else {
            // 远程没有 mark_working RPC，先更新本地镜像，避免输入后仍显示 Idle。
            for snapshot in self.remote_snaps.values_mut() {
                if let Some(a) = snapshot.agent_mut(agent) {
                    a.status = muxlane_core::model::AgentStatus::Working;
                    a.status_since = muxlane_core::model::now_secs();
                    cx.notify();
                    break;
                }
            }
        }
        // 与屏幕采样同走 DetectionEngine：既避免与引擎内部状态互斥（否则引擎
        // 推导出的候选 idle 会因等于陈旧内部状态而永不提交，spinner 卡死），
        // 又保证 Idle 状态下输入命令立即显示 working 反馈。
        let agent_id = agent.clone();
        if local {
            let server = Arc::clone(&self.server);
            server.rt_spawn({
                let server = Arc::clone(&server);
                async move { server.mark_working(&agent_id).await }
            });
        }
    }

    pub(crate) fn create_local_term(
        agent: AgentId,
        session: Arc<muxlane_term::PtySession>,
        font_family: &str,
        theme: Theme,
        osc52_clipboard_enabled: bool,
        cx: &mut Context<Self>,
    ) -> Entity<TermView> {
        let font_family = font_family.to_string();
        let term = cx.new(|cx| {
            TermView::new_local(
                agent.clone(),
                session,
                font_family,
                theme,
                osc52_clipboard_enabled,
                cx,
            )
        });
        cx.subscribe(
            &term,
            |this, _term, ev: &crate::term_view::TermEnterEvent, cx| {
                this.mark_agent_working(&ev.0, cx);
            },
        )
        .detach();
        term
    }

    pub(crate) fn create_remote_term(
        agent: AgentId,
        terminal: (VTerm, tokio::sync::mpsc::UnboundedReceiver<String>),
        remote_input: tokio::sync::mpsc::UnboundedSender<crate::term_view::RemoteTermCommand>,
        font_family: &str,
        theme: Theme,
        osc52_clipboard_enabled: bool,
        cx: &mut Context<Self>,
    ) -> Entity<TermView> {
        let font_family = font_family.to_string();
        let term = cx.new(|cx| {
            TermView::new_remote(
                agent.clone(),
                terminal,
                remote_input,
                font_family,
                theme,
                osc52_clipboard_enabled,
                cx,
            )
        });
        cx.subscribe(
            &term,
            |this, _term, ev: &crate::term_view::TermEnterEvent, cx| {
                this.mark_agent_working(&ev.0, cx);
            },
        )
        .detach();
        term
    }

    pub(crate) fn open_agent(&mut self, agent: &AgentId, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.activate_tab(&pane, agent, cx);
            cx.notify();
            return;
        }
        if !self.terms.contains_key(agent) {
            let Some(sess) = self.server.try_session(agent) else {
                return;
            };
            let term = Self::create_local_term(
                agent.clone(),
                sess,
                &self.font_family,
                Theme::for_mode(self.theme_mode),
                self.osc52_clipboard_enabled,
                cx,
            );
            self.terms.insert(agent.clone(), term);
        }
        let pane = self.active_pane.clone();
        self.pane_tree.open_tab(&pane, agent.clone());
        self.activate_tab(&pane, agent, cx);
        cx.notify();
    }

    pub(crate) fn open_remote_agent(&mut self, agent: &AgentId, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.activate_tab(&pane, agent, cx);
            cx.notify();
            return;
        }
        if !self.terms.contains_key(agent) {
            let (vterm, clipboard_rx) = VTerm::new_with_clipboard(120, 32);
            vterm.feed(
                format!(
                    "\u{1b}[2m{}\u{1b}[0m\r\n",
                    i18n::text(self.language, "terminal.attaching")
                )
                .as_bytes(),
            );
            let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
            let term = Self::create_remote_term(
                agent.clone(),
                (vterm.clone(), clipboard_rx),
                command_tx,
                &self.font_family,
                Theme::for_mode(self.theme_mode),
                self.osc52_clipboard_enabled,
                cx,
            );
            let weak_term = term.downgrade();
            self.terms.insert(agent.clone(), term);
            // agent → 所属 RemoteHost，禁止全局 endpoint 串台。
            let host_name = self
                .remote_snaps
                .iter()
                .find(|(_, snap)| snap.agents.iter().any(|a| &a.id == agent))
                .map(|(host, _)| host.clone());
            let remote = host_name
                .and_then(|name| self.remotes.iter().find(|h| h.cfg.name == name).cloned());
            if let Some(remote) = remote {
                let (mirror_notify, mut mirror_notify_rx) = tokio::sync::mpsc::channel(1);
                cx.spawn(async move |_this, cx| {
                    while mirror_notify_rx.recv().await.is_some() {
                        if weak_term.update(cx, |_term, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                })
                .detach();

                let command_remote = Arc::clone(&remote);
                let command_agent = agent.clone();
                let command_vterm = vterm.clone();
                self.server.rt_spawn(async move {
                    while let Some(first) = command_rx.recv().await {
                        let mut input = Vec::new();
                        let mut resize = None;
                        let mut collect =
                            |command: crate::term_view::RemoteTermCommand| match command {
                                crate::term_view::RemoteTermCommand::Input(bytes) => {
                                    input.extend(bytes)
                                }
                                crate::term_view::RemoteTermCommand::Resize(cols, rows) => {
                                    resize = Some((cols, rows));
                                }
                            };
                        collect(first);
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        while let Ok(command) = command_rx.try_recv() {
                            collect(command);
                        }
                        if !input.is_empty() {
                            if let Err(error) =
                                command_remote.send_term_input(&command_agent, &input).await
                            {
                                command_vterm.feed(
                                    format!("\r\n\x1b[31mremote input failed: {error}\x1b[0m\r\n")
                                        .as_bytes(),
                                );
                            }
                        }
                        if let Some((cols, rows)) = resize {
                            let _ = command_remote.resize_term(&command_agent, cols, rows).await;
                        }
                    }
                });

                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.mirror_cancel
                    .insert(agent.clone(), Arc::clone(&cancelled));
                let agent2 = agent.clone();
                let vterm2 = vterm.clone();
                let language = self.language;
                self.server.rt_spawn(async move {
                    let mut backoff = 250u64;
                    loop {
                        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        let Some(sock) = remote.endpoint_now() else {
                            tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                            backoff = (backoff * 2).min(5_000);
                            continue;
                        };
                        let vterm3 = vterm2.clone();
                        let notify = mirror_notify.clone();
                        let result = muxlane_client::stream_term(&sock, &agent2, move |update| {
                            match update {
                                muxlane_client::TermUpdate::Resync(bytes) => {
                                    vterm3.feed(b"\x1bc");
                                    vterm3.feed(&bytes);
                                }
                                muxlane_client::TermUpdate::Data(bytes) => vterm3.feed(&bytes),
                            }
                            let _ = notify.try_send(());
                        })
                        .await;
                        if result.is_ok() || cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        vterm2.feed(
                            format!(
                                "\u{1b}[31m{}\u{1b}[0m\r\n",
                                i18n::text(language, "terminal.reconnecting")
                            )
                            .as_bytes(),
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                        backoff = (backoff * 2).min(5_000);
                    }
                });
            }
        }
        let pane = self.active_pane.clone();
        self.pane_tree.open_tab(&pane, agent.clone());
        self.activate_tab(&pane, agent, cx);
        cx.notify();
    }

    pub(crate) fn focus_agent(
        &mut self,
        agent: &AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(term) = self.terms.get(agent) {
            term.focus_handle(cx).focus(window, cx);
        }
        // 清理当前 agent 的 Toast 与标记通知已读
        self.notifications
            .update(cx, |center, cx| center.mark_agent_read(agent, cx));
        if let Some(a) = self.last_snapshot.agent_mut(agent) {
            a.seen = true;
            if a.status == muxlane_core::model::AgentStatus::Done {
                a.status = muxlane_core::model::AgentStatus::Idle;
            }
        } else {
            // 远端没有 mark_seen RPC，先同步本地镜像，避免点击后仍持续闪烁。
            for snapshot in self.remote_snaps.values_mut() {
                if let Some(a) = snapshot.agent_mut(agent) {
                    a.seen = true;
                    if a.status == muxlane_core::model::AgentStatus::Done {
                        a.status = muxlane_core::model::AgentStatus::Idle;
                    }
                    break;
                }
            }
        }
        if self.last_snapshot.agent(agent).is_some() {
            let server = Arc::clone(&self.server);
            let agent = agent.clone();
            server.rt_spawn({
                let server = Arc::clone(&server);
                async move { server.mark_seen(&agent).await }
            });
        }
        cx.notify();
    }

    pub(crate) fn jump_to_agent(
        &mut self,
        agent: &AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_remote = self
            .remote_snaps
            .values()
            .any(|snap| snap.agent(agent).is_some());
        if is_remote {
            self.open_remote_agent(agent, cx);
        } else {
            self.open_agent(agent, cx);
        }
        self.focus_agent(agent, window, cx);
    }

    pub(crate) fn delete_session(
        &mut self,
        agent: &AgentId,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if remote {
            let host_name = self
                .remote_snaps
                .iter()
                .find(|(_, snap)| snap.agents.iter().any(|a| &a.id == agent))
                .map(|(host, _)| host.clone());
            if let Some(host_name) = host_name {
                if let Some(host) = self
                    .remotes
                    .iter()
                    .find(|host| host.cfg.name == host_name)
                    .cloned()
                {
                    let id = agent.clone();
                    self.server.rt_spawn(async move {
                        let _ = host.delete_agent(&id).await;
                    });
                }
                if let Some(snapshot) = self.remote_snaps.get_mut(&host_name) {
                    snapshot.agents.retain(|candidate| &candidate.id != agent);
                    for project in &mut snapshot.projects {
                        project.agents.retain(|candidate| candidate != agent);
                    }
                }
            }
            self.finish_delete_session(agent, window, cx);
            return;
        }

        let server = Arc::clone(&self.server);
        let agent = agent.clone();
        cx.spawn_in(window, async move |this, cx| {
            let agent_for_delete = agent.clone();
            let (result, snapshot) = cx
                .background_spawn(async move {
                    let result = server.delete_agent(&agent_for_delete).await;
                    let snapshot = server.snapshot().await;
                    (result, snapshot)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.last_snapshot = snapshot;
                match result {
                    Ok(result) if result.failed_agents.is_empty() => {
                        this.finish_delete_session(&agent, window, cx);
                    }
                    Ok(result) => {
                        this.notifications.update(cx, |center, cx| {
                            center.show_error(
                                i18n::text(this.language, "error.delete_sessions_session")
                                    .replace("{count}", &result.failed_agents.len().to_string()),
                                cx,
                            )
                        });
                        cx.notify();
                    }
                    Err(error) => {
                        if this.last_snapshot.agent(&agent).is_none() {
                            this.finish_delete_session(&agent, window, cx);
                        }
                        this.notifications.update(cx, |center, cx| {
                            center.show_error(
                                i18n::text(this.language, "error.delete_session")
                                    .replace("{error}", &error.to_string()),
                                cx,
                            )
                        });
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn finish_delete_session(
        &mut self,
        agent: &AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.pane_tree.pane_for_agent(agent) {
            self.pane_tree.close_tab(&pane, agent);
        }
        self.terms.remove(agent);
        if let Some(cancelled) = self.mirror_cancel.remove(agent) {
            cancelled.store(true, std::sync::atomic::Ordering::Release);
        }
        self.notifications
            .update(cx, |center, cx| center.remove_agent(agent, cx));
        if self.active.as_ref() == Some(agent) {
            self.active = self
                .pane_tree
                .group(&self.active_pane)
                .and_then(|group| group.active.clone());
        }
        self.session_menu = None;
        if let Some(active) = self.active.clone() {
            self.focus_agent(&active, window, cx);
        }
        self.persist();
        cx.notify();
    }

    pub(crate) fn cleanup_removed_agents(&mut self, removed: &[AgentId], cx: &mut Context<Self>) {
        let removed: std::collections::HashSet<_> = removed.iter().cloned().collect();
        self.terms.retain(|agent, _| !removed.contains(agent));
        for agent in &removed {
            if let Some(cancelled) = self.mirror_cancel.remove(agent) {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        self.notifications
            .update(cx, |center, cx| center.remove_agents(&removed, cx));
        let valid: std::collections::HashSet<_> = self
            .last_snapshot
            .agents
            .iter()
            .map(|agent| agent.id.clone())
            .chain(
                self.remote_snaps
                    .values()
                    .flat_map(|snapshot| snapshot.agents.iter().map(|agent| agent.id.clone())),
            )
            .filter(|agent| !removed.contains(agent))
            .collect();
        self.pane_tree.retain_agents(&valid);
        if self
            .active
            .as_ref()
            .is_some_and(|agent| removed.contains(agent))
        {
            self.active = self
                .pane_tree
                .group(&self.active_pane)
                .and_then(|group| group.active.clone());
        }
        let clear_maximized = self
            .maximized_pane
            .as_ref()
            .and_then(|pane| self.pane_tree.group(pane))
            .map(|group| group.active.is_none())
            .unwrap_or(true);
        if clear_maximized {
            self.maximized_pane = None;
        }
    }
}

impl MuxlaneApp {
    pub(crate) fn spawn_preset(
        &mut self,
        preset: &muxlane_core::AgentPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.new_session_target.take();
        if let Some(NewSessionTarget::Remote { host, project }) = target.clone() {
            self.palette_open = false;
            self.spawn_remote_agent(host, project, Some(preset.clone()), window, cx);
            return;
        }
        let target_local_id = match target {
            Some(NewSessionTarget::Local(id)) => Some(id),
            _ => None,
        };
        let project = target_local_id
            .as_ref()
            .and_then(|id| self.last_snapshot.project(id))
            .cloned()
            .or_else(|| {
                self.active
                    .as_ref()
                    .and_then(|id| self.last_snapshot.agent(id))
                    .and_then(|agent| self.last_snapshot.project(&agent.project))
                    .cloned()
            })
            .or_else(|| self.last_snapshot.projects.first().cloned());
        let Some(project) = project else { return };
        let params = muxlane_core::protocol::AgentSpawnParams {
            project: project.id.clone(),
            agent_type: Some(preset.agent_type),
            program: (preset.agent_type != muxlane_core::model::AgentType::Shell).then(|| {
                preset
                    .executable_in(&project.path)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| preset.program.clone())
            }),
            args: Some(preset.args.clone()),
            env: Some(preset.env.clone().into_iter().collect()),
            preset_name: Some(preset.label.clone()),
        };
        let server = Arc::clone(&self.server);
        let pane = self.active_pane.clone();
        let project_key = format!("local:{}", project.id);
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let agent = server.spawn_agent(params).await?;
                    let session = server
                        .session(&agent.id)
                        .await
                        .ok_or_else(|| anyhow::anyhow!("spawned agent has no session"))?;
                    Ok::<_, anyhow::Error>((agent, session))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok((agent, session)) => {
                    let agent_id = agent.id.clone();
                    let term = Self::create_local_term(
                        agent_id.clone(),
                        session,
                        &this.font_family,
                        Theme::for_mode(this.theme_mode),
                        this.osc52_clipboard_enabled,
                        cx,
                    );
                    this.collapsed_projects.remove(&project_key);
                    this.terms.insert(agent_id.clone(), term);
                    this.pane_tree.open_tab(&pane, agent_id.clone());
                    this.activate_tab(&pane, &agent_id, cx);
                    this.focus_agent(&agent_id, window, cx);
                    this.palette_open = false;
                    this.new_session_target = None;
                    this.persist();
                    cx.notify();
                }
                Err(error) => {
                    this.notifications.update(cx, |center, cx| {
                        center.show_error(
                            i18n::text(this.language, "error.create_session")
                                .replace("{error}", &error.to_string()),
                            cx,
                        )
                    });
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
