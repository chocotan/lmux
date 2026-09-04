//! Sidebar machine tree, project/session rows, and footer controls.
use super::palette::NewSessionTarget;
use super::MuxlaneApp;
use crate::dialogs::ConnectAuthMode;
use crate::i18n;
use crate::icons::*;
use crate::menus::{dismiss_context_menus, BootstrapConfirm, DeleteTarget, SessionMenu, TreeMenu};
use crate::theme::Theme;
use crate::widgets::*;
use gpui::{
    div, prelude::*, px, relative, rgba, Context, Focusable, MouseButton, ParentElement, Render,
    SharedString, Styled, Window,
};
use std::sync::Arc;

struct HoverTip {
    text: SharedString,
}

impl Render for HoverTip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgba(0x1a1d24f2))
            .border_1()
            .border_color(rgba(0x00000066))
            .text_size(px(11.))
            .text_color(rgba(0xffffffff))
            .child(self.text.clone())
    }
}

fn hover_tip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut gpui::App) -> gpui::AnyView {
    let text = text.into();
    move |_, cx| cx.new(|_| HoverTip { text: text.clone() }).into()
}

impl MuxlaneApp {
    fn render_project_row(
        &self,
        project: &muxlane_core::model::Project,
        remote_host: Option<&str>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (row_id, add_id, project_key, project_group, branch, delete_target, new_session_target) =
            if let Some(host) = remote_host {
                (
                    format!("remote-project-row-{host}-{}", project.id),
                    format!("remote-session-add-{host}-{}", project.id),
                    format!("remote:{host}:{}", project.id),
                    format!("remote-project-hover-{host}-{}", project.id),
                    project.branch.clone(),
                    DeleteTarget::RemoteProject {
                        host: host.to_string(),
                        project: project.id.clone(),
                        label: project.name.clone(),
                    },
                    NewSessionTarget::Remote {
                        host: host.to_string(),
                        project: project.id.clone(),
                    },
                )
            } else {
                (
                    format!("project-row-{}", project.id),
                    format!("project-add-{}", project.id),
                    format!("local:{}", project.id),
                    format!("project-hover-{}", project.id),
                    project
                        .branch
                        .clone()
                        .filter(|branch| !branch.trim().is_empty()),
                    DeleteTarget::LocalProject {
                        project: project.id.clone(),
                        label: project.name.clone(),
                    },
                    NewSessionTarget::Local(project.id.clone()),
                )
            };
        div()
            .id(gpui::ElementId::Name(row_id.into()))
            .flex()
            .items_center()
            .gap_1()
            .h(px(28.))
            .pl_4()
            .pr_2()
            .text_size(px(12.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(rgba(theme.fg0))
            .group(project_group.clone())
            .hover(|style| style.bg(rgba(theme.bg2)))
            .on_click(cx.listener(move |this, _event, _window, cx| {
                if !this.collapsed_projects.remove(&project_key) {
                    this.collapsed_projects.insert(project_key.clone());
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.palette_open = false;
                    this.session_menu = None;
                    this.tree_menu = Some(TreeMenu {
                        target: delete_target.clone(),
                        position: event.position,
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(project.name.clone())
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when_some(branch, |controls, branch| {
                        controls.child(
                            div()
                                .px_1()
                                .bg(rgba(theme.bg2))
                                .text_size(px(9.))
                                .font_weight(gpui::FontWeight::NORMAL)
                                .text_color(rgba(theme.fg1))
                                .child(branch),
                        )
                    })
                    .child(
                        div()
                            .id(gpui::ElementId::Name(add_id.into()))
                            .w(px(20.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgba(theme.fg1))
                            .invisible()
                            .group_hover(project_group, |style| style.visible())
                            .hover(|style| style.bg(rgba(theme.bg2)).text_color(rgba(theme.accent)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                cx.stop_propagation();
                                this.new_session_target = Some(new_session_target.clone());
                                this.palette_open = true;
                                this.palette_index = 0;
                                this.palette_scroll.scroll_to_item(0);
                                this.palette_input.update(cx, |input, cx| input.reset(cx));
                                this.palette_input.focus_handle(cx).focus(window, cx);
                                dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu);
                                cx.notify();
                            }))
                            .child(panel_icon(PLUS_ICON, theme.fg1)),
                    ),
            )
    }

    fn render_agent_row(
        &self,
        agent: &muxlane_core::model::AgentInstance,
        remote: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = agent.id.clone();
        let active = self.active.as_deref() == Some(&id);
        let status = agent.status;
        let is_error = agent.title.contains("异常") || agent.title.contains("错误");
        let attention = compute_attention_style(
            status,
            agent.seen || active,
            is_error,
            self.pulse_phase,
            theme,
        );
        div()
            .id(gpui::ElementId::Name(id.clone().into()))
            .flex()
            .items_center()
            .gap_1()
            .h(px(26.))
            .pl(px(24.))
            .pr_2()
            .text_size(px(11.5))
            .when(
                attention.is_alerting && attention.text_color.is_some(),
                |el| el.text_color(rgba(attention.text_color.unwrap())),
            )
            .when(!attention.is_alerting, |el| {
                el.text_color(rgba(if active { theme.fg0 } else { theme.fg1 }))
            })
            .hover(|style| style.bg(rgba(theme.bg2)))
            .when(
                attention.is_alerting && attention.bg_color.is_some(),
                |el| el.bg(rgba(attention.bg_color.unwrap())),
            )
            .when(!attention.is_alerting && active, |el| {
                el.bg(rgba(theme.bg2))
            })
            .on_click(cx.listener({
                let id = id.clone();
                move |this, _event, window, cx| {
                    if remote {
                        this.open_remote_agent(&id, cx);
                    } else {
                        this.open_agent(&id, cx);
                    }
                    this.focus_agent(&id, window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.tree_menu = None;
                    this.session_menu = Some(SessionMenu {
                        agent: id.clone(),
                        position: event.position,
                        remote,
                    });
                    this.palette_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(render_status_indicator(
                status,
                is_error,
                self.spinner_frame,
                theme,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(truncate(&agent.title, 20)),
            )
    }
}

impl MuxlaneApp {
    pub(crate) fn render_machine_tree(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let snap = self.last_snapshot.clone();
        let machine_name = snap
            .machine
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "local".into());

        // ── 侧栏：机器树（统一 Machines 树：Local Machine + Projects + Sessions）
        let local_machine_key = "local".to_string();
        let local_collapsed = self.collapsed_machines.contains(&local_machine_key);
        let mut tree = div().flex().flex_col().py_1();
        tree = tree.child(
            div()
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgba(theme.fg1))
                .child("MACHINES")
                .child(
                    div()
                        .id("connect-machine")
                        .ml_auto()
                        .w(px(20.))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(rgba(theme.fg1))
                        .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.accent)))
                        .active(|s| s.bg(rgba(theme.bg3)))
                        .tooltip(hover_tip(i18n::text(
                            self.language,
                            "sidebar.connect_remote",
                        )))
                        .on_click(cx.listener(|this, _ev, window, cx| {
                            this.open_connect_dialog(window, cx);
                        }))
                        .child(panel_icon(PLUS_ICON, theme.fg1)),
                ),
        );
        tree = tree.child(
            div()
                .id("machine-local")
                .flex()
                .items_center()
                .gap_1()
                .h(px(32.))
                .pl_4()
                .pr_2()
                .text_size(px(12.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgba(theme.fg0))
                .group("local-machine")
                .hover(|style| style.bg(rgba(theme.bg2)))
                .on_click(cx.listener({
                    let key = local_machine_key.clone();
                    move |this, _event, _window, cx| {
                        if !this.collapsed_machines.remove(&key) {
                            this.collapsed_machines.insert(key.clone());
                        }
                        cx.notify();
                    }
                }))
                .child(machine_name.clone())
                .child(
                    div()
                        .ml_auto()
                        .px_1()
                        .bg(rgba(theme.bg2))
                        .text_size(px(9.))
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(rgba(theme.fg1))
                        .child("local"),
                )
                .child(
                    div()
                        .id("add-local-project")
                        .w(px(20.))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(rgba(theme.fg1))
                        .invisible()
                        .group_hover("local-machine", |style| style.visible())
                        .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.accent)))
                        .on_click(cx.listener(|this, _ev, window, cx| {
                            cx.stop_propagation();
                            this.project_dialog = true;
                            this.connect_dialog = false;
                            this.dialog_error = None;
                            this.project_input.update(cx, |input, cx| input.reset(cx));
                            this.project_input.focus_handle(cx).focus(window, cx);
                            cx.notify();
                        }))
                        .child(panel_icon(PLUS_ICON, theme.fg1)),
                ),
        );
        if !local_collapsed {
            for project in &snap.projects {
                let project_key = format!("local:{}", project.id);
                let project_collapsed = self.collapsed_projects.contains(&project_key);
                let mut pnode = div().flex().flex_col().ml(px(18.));
                pnode = pnode.child(self.render_project_row(project, None, theme, cx));
                if !project_collapsed {
                    for agent in snap.agents_of(&project.id) {
                        pnode = pnode.child(self.render_agent_row(agent, false, theme, cx));
                    }
                }
                tree = tree.child(pnode);
            }
        }

        // ── 远程机器分组
        for host in &self.remotes {
            let name = host.cfg.name.clone();
            let machine_target = DeleteTarget::RemoteMachine { host: name.clone() };
            let (dot_color, status_text, remediation) = match self.remote_states.get(&name) {
                Some(muxlane_client::RemoteState::Online(_)) => {
                    if self.remote_snaps.contains_key(&name) {
                        (
                            theme.green,
                            i18n::text(self.language, "status.connected"),
                            None,
                        )
                    } else {
                        (
                            theme.fg1,
                            i18n::text(self.language, "status.connecting"),
                            None,
                        )
                    }
                }
                Some(muxlane_client::RemoteState::NeedsInstall { .. }) => (
                    theme.yellow,
                    i18n::text(self.language, "status.needs_install"),
                    Some((true, false, None)),
                ),
                Some(muxlane_client::RemoteState::NeedsStart { binary, .. }) => (
                    theme.yellow,
                    i18n::text(self.language, "status.needs_start"),
                    Some((false, false, Some(binary.clone()))),
                ),
                Some(muxlane_client::RemoteState::NeedsUpgrade { .. }) => (
                    theme.yellow,
                    i18n::text(self.language, "status.needs_update"),
                    Some((false, true, None)),
                ),
                Some(muxlane_client::RemoteState::AuthenticationFailed(_)) => (
                    theme.red,
                    i18n::text(self.language, "status.auth_failed"),
                    None,
                ),
                Some(muxlane_client::RemoteState::Connecting(stage)) => (
                    theme.yellow,
                    i18n::text(
                        self.language,
                        match stage {
                            muxlane_client::RemoteStage::SshProbe => "status.remote_ssh_probe",
                            muxlane_client::RemoteStage::Tunnel => "status.remote_tunnel",
                            muxlane_client::RemoteStage::Subscribe => "status.remote_subscribe",
                        },
                    ),
                    None,
                ),
                Some(muxlane_client::RemoteState::Offline(_)) => {
                    (theme.fg1, i18n::text(self.language, "status.offline"), None)
                }
                _ => (
                    theme.fg1,
                    i18n::text(self.language, "status.connecting"),
                    None,
                ),
            };
            let reconnectable = !matches!(
                self.remote_states.get(&name),
                Some(muxlane_client::RemoteState::Online(_))
            );
            let status_text = if matches!(
                self.remote_states.get(&name),
                Some(muxlane_client::RemoteState::Online(_))
            ) {
                host.latency_ms()
                    .map(|latency| format!("{latency} ms"))
                    .unwrap_or_else(|| i18n::text(self.language, "status.connected").into())
            } else if matches!(
                self.remote_states.get(&name),
                Some(muxlane_client::RemoteState::Offline(_))
            ) {
                i18n::text(self.language, "status.disconnected").into()
            } else {
                status_text.to_string()
            };
            let machine_key = format!("remote:{name}");
            let machine_collapsed = self.collapsed_machines.contains(&machine_key);
            let snap_ref = self.remote_snaps.get(&name);
            let machine_attention = snap_ref
                .map(|snapshot| {
                    if snapshot
                        .agents
                        .iter()
                        .any(|agent| agent.status == muxlane_core::model::AgentStatus::Blocked)
                    {
                        compute_attention_style(
                            muxlane_core::model::AgentStatus::Blocked,
                            false,
                            false,
                            self.pulse_phase,
                            theme,
                        )
                    } else if snapshot.agents.iter().any(|agent| {
                        agent.status == muxlane_core::model::AgentStatus::Done
                            && !agent.seen
                            && self.active.as_ref() != Some(&agent.id)
                    }) {
                        compute_attention_style(
                            muxlane_core::model::AgentStatus::Done,
                            false,
                            false,
                            self.pulse_phase,
                            theme,
                        )
                    } else {
                        AttentionStyle::default()
                    }
                })
                .unwrap_or_default();
            let remediation_host = name.clone();
            let remote_project_host = name.clone();
            let mut rnode = div().flex().flex_col().mt_1().child(
                div()
                    .id(gpui::ElementId::Name(format!("machine-row-{name}").into()))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(32.))
                    .pl_4()
                    .pr_2()
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgba(theme.fg0))
                    .when(machine_attention.is_alerting, |row| {
                        row.bg(rgba(machine_attention.bg_color.unwrap_or(theme.bg2)))
                    })
                    .group(gpui::SharedString::from(format!("machine-hover-{name}")))
                    .hover(|style| style.bg(rgba(theme.bg2)))
                    .on_click(cx.listener({
                        let key = machine_key.clone();
                        move |this, _event, _window, cx| {
                            if !this.collapsed_machines.remove(&key) {
                                this.collapsed_machines.insert(key.clone());
                            }
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener({
                            let target = machine_target.clone();
                            move |this, event: &gpui::MouseDownEvent, window, cx| {
                                this.focus.focus(window, cx);
                                this.session_menu = None;
                                this.tree_menu = Some(TreeMenu {
                                    target: target.clone(),
                                    position: event.position,
                                });
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }),
                    )
                    .child(name.clone())
                    .child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("remote-reconnect-{name}").into(),
                            ))
                            .ml_auto()
                            .px_1()
                            .bg(rgba(theme.bg2))
                            .text_size(px(9.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgba(dot_color))
                            .on_click(cx.listener({
                                let host = Arc::clone(host);
                                let host_cfg = host.cfg.clone();
                                let is_auth_failed = matches!(
                                    self.remote_states.get(&name),
                                    Some(muxlane_client::RemoteState::AuthenticationFailed(_))
                                );
                                move |this, _event, window, cx| {
                                    if is_auth_failed {
                                        this.connect_dialog = true;
                                        this.dialog_error = None;
                                        let target_str = match &host_cfg.target {
                                            muxlane_client::Target::Socket(path) => path.clone(),
                                            muxlane_client::Target::Ssh { host, socket }
                                                if socket.is_empty() =>
                                            {
                                                host.clone()
                                            }
                                            muxlane_client::Target::Ssh { host, socket } => {
                                                format!("{host}:{socket}")
                                            }
                                        };
                                        this.connect_input.update(cx, |input, cx| {
                                            input.set_text(&target_str, cx);
                                        });
                                        match &host_cfg.auth {
                                            muxlane_client::SshAuth::Password {
                                                username, ..
                                            } => {
                                                this.connect_auth_mode = ConnectAuthMode::Password;
                                                this.connect_username.update(cx, |input, cx| {
                                                    input.set_text(username, cx);
                                                });
                                                this.connect_password
                                                    .update(cx, |input, cx| input.reset(cx));
                                                this.connect_password
                                                    .focus_handle(cx)
                                                    .focus(window, cx);
                                            }
                                            muxlane_client::SshAuth::PublicKey {
                                                username,
                                                identity_file,
                                            } => {
                                                this.connect_auth_mode = ConnectAuthMode::PublicKey;
                                                if let Some(u) = username {
                                                    this.connect_username.update(
                                                        cx,
                                                        |input, cx| {
                                                            input.set_text(u, cx);
                                                        },
                                                    );
                                                }
                                                if let Some(k) = identity_file {
                                                    this.connect_key_path.update(
                                                        cx,
                                                        |input, cx| {
                                                            input.set_text(k, cx);
                                                        },
                                                    );
                                                }
                                                this.connect_key_path
                                                    .focus_handle(cx)
                                                    .focus(window, cx);
                                            }
                                            muxlane_client::SshAuth::SshConfig => {
                                                this.connect_auth_mode = ConnectAuthMode::SshConfig;
                                                this.connect_input
                                                    .focus_handle(cx)
                                                    .focus(window, cx);
                                            }
                                        }
                                        cx.notify();
                                    } else if reconnectable {
                                        host.reconnect();
                                        this.focus.focus(window, cx);
                                        cx.notify();
                                    }
                                    cx.stop_propagation();
                                }
                            }))
                            .child(status_text),
                    )
                    .child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("remote-project-add-{remote_project_host}").into(),
                            ))
                            .w(px(20.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgba(theme.fg1))
                            .invisible()
                            .group_hover(format!("machine-hover-{name}"), |style| style.visible())
                            .hover(|style| style.bg(rgba(theme.bg2)).text_color(rgba(theme.accent)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                cx.stop_propagation();
                                this.remote_project_dialog = Some(remote_project_host.clone());
                                this.dialog_error = None;
                                this.remote_project_input
                                    .update(cx, |input, cx| input.reset(cx));
                                this.remote_project_input.focus_handle(cx).focus(window, cx);
                                cx.notify();
                            }))
                            .child(panel_icon(PLUS_ICON, theme.fg1)),
                    )
                    .when_some(remediation, |row, (install, upgrade, binary)| {
                        row.child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("remote-remediate-{remediation_host}").into(),
                                ))
                                .px_2()
                                .py_1()
                                .text_size(px(10.))
                                .text_color(rgba(theme.accent))
                                .hover(|style| style.bg(rgba(theme.bg2)))
                                .on_click(cx.listener({
                                    let host = remediation_host.clone();
                                    move |this, _event, _window, cx| {
                                        this.bootstrap_error = None;
                                        this.bootstrap_confirm = Some(BootstrapConfirm {
                                            host: host.clone(),
                                            install,
                                            upgrade,
                                            binary: binary.clone(),
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                }))
                                .child(if install {
                                    i18n::text(self.language, "bootstrap.install")
                                } else if upgrade {
                                    i18n::text(self.language, "bootstrap.update")
                                } else {
                                    i18n::text(self.language, "bootstrap.start")
                                }),
                        )
                    }),
            );
            // 进度条
            if let Some(progress) = self.bootstrap_progress.get(&name) {
                let overall = progress.phase.overall(progress.percent);
                let phase_text = format_upload_phase(progress, self.language);
                let host_to_cancel = name.clone();
                rnode = rnode.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pl_4()
                        .pr_2()
                        .h(px(22.))
                        .text_size(px(10.))
                        .text_color(rgba(theme.accent))
                        .child(phase_text)
                        .child(
                            div().flex_1().h(px(3.)).bg(rgba(theme.bg2)).child(
                                div()
                                    .w(relative(overall as f32 / 100.0))
                                    .h_full()
                                    .bg(rgba(theme.accent)),
                            ),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("sidebar-cancel-bootstrap-{host_to_cancel}").into(),
                                ))
                                .px_1()
                                .cursor_pointer()
                                .text_color(rgba(theme.fg2))
                                .hover(|s| s.text_color(rgba(theme.red)))
                                .on_click(cx.listener(move |this, _ev, _window, cx| {
                                    this.cancel_bootstrap_for_host(&host_to_cancel, cx);
                                }))
                                .child("×"),
                        ),
                );
            }
            if !machine_collapsed {
                if let Some(rsnap) = snap_ref {
                    for project in &rsnap.projects {
                        let project_key = format!("remote:{name}:{}", project.id);
                        let project_collapsed = self.collapsed_projects.contains(&project_key);
                        let mut pnode = div().flex().flex_col().ml(px(18.));
                        pnode =
                            pnode.child(self.render_project_row(project, Some(&name), theme, cx));
                        if !project_collapsed {
                            for agent in rsnap.agents_of(&project.id) {
                                pnode = pnode.child(self.render_agent_row(agent, true, theme, cx));
                            }
                        }
                        rnode = rnode.child(pnode);
                    }
                }
            }
            tree = tree.child(rnode);
        }
        tree.into_any_element()
    }

    pub(crate) fn render_sidebar_footer(
        &mut self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (unread_count, has_blocked, notifications_open) = self.notifications.read(cx).summary();
        div()
            .h(px(40.))
            .px_2()
            .flex()
            .items_center()
            .justify_end()
            .gap_1()
            .border_t_1()
            .border_color(rgba(theme.line))
            .child({
                let pulse =
                    (1.0 - (self.pulse_phase as f32 * std::f32::consts::TAU / 36.0).cos()) * 0.5;
                let badge_color = if has_blocked {
                    theme.yellow
                } else if unread_count > 0 {
                    theme.accent
                } else {
                    theme.fg2
                };
                let badge_glow = if unread_count > 0 {
                    let glow_alpha = (0x30 as f32 + pulse * 0x60 as f32) as u32;
                    Some(Theme::with_alpha(badge_color, glow_alpha as u8))
                } else {
                    None
                };

                div()
                    .id("sidebar-notification-button")
                    .relative()
                    .w(px(32.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(theme.bg2)))
                    .active(|s| s.bg(rgba(theme.bg3)))
                    .tooltip(hover_tip(i18n::text(
                        self.language,
                        "sidebar.notifications",
                    )))
                    .when(unread_count > 0, |el| {
                        el.bg(rgba(Theme::with_alpha(badge_color, 0x18)))
                    })
                    .when(notifications_open, |el| el.bg(rgba(theme.bg2)))
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.notifications
                            .update(cx, |center, cx| center.toggle_open(cx));
                        cx.notify();
                    }))
                    .child(panel_icon(
                        NOTIFICATION_ICON,
                        if notifications_open || unread_count > 0 {
                            badge_color
                        } else {
                            theme.fg1
                        },
                    ))
                    .when(unread_count > 0, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(2.))
                                .right(px(2.))
                                .min_w(px(14.))
                                .h(px(14.))
                                .px(px(3.))
                                .rounded_full()
                                .bg(rgba(badge_color))
                                .when_some(badge_glow, |b, glow| {
                                    b.border_1().border_color(rgba(glow))
                                })
                                .text_size(px(9.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgba(theme.on_accent))
                                .child(if unread_count > 99 {
                                    "99+".to_string()
                                } else {
                                    format!("{unread_count}")
                                }),
                        )
                    })
            })
            .child(
                div()
                    .id("open-settings")
                    .w(px(32.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(theme.bg2)))
                    .active(|s| s.bg(rgba(theme.bg3)))
                    .when(self.settings_open, |el| el.bg(rgba(theme.bg2)))
                    .tooltip(hover_tip(i18n::text(self.language, "common.settings")))
                    .on_click(cx.listener(|this, _ev, window, cx| {
                        this.settings_open = true;
                        this.palette_open = false;
                        this.focus.focus(window, cx);
                        cx.notify();
                    }))
                    .child(panel_icon(
                        SETTINGS_ICON,
                        if self.settings_open {
                            theme.fg0
                        } else {
                            theme.fg1
                        },
                    )),
            )
            .into_any_element()
    }
}
