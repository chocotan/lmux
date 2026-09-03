//! Context menus and confirmation dialogs.
use crate::app::MuxlaneApp;
use crate::theme::Theme;
use crate::widgets::format_upload_phase;
use gpui::{
    div, prelude::*, px, relative, rgba, Context, Focusable, ParentElement, Pixels, Point, Styled,
};
use muxlane_core::model::AgentId;

#[derive(Clone)]
pub(crate) struct SessionMenu {
    pub(crate) agent: AgentId,
    pub(crate) position: Point<Pixels>,
    pub(crate) remote: bool,
}

#[derive(Clone)]
pub(crate) enum DeleteTarget {
    LocalProject {
        project: String,
        label: String,
    },
    RemoteProject {
        host: String,
        project: String,
        label: String,
    },
    RemoteMachine {
        host: String,
    },
}

#[derive(Clone)]
pub(crate) struct TreeMenu {
    pub(crate) target: DeleteTarget,
    pub(crate) position: Point<Pixels>,
}

pub(crate) fn dismiss_context_menus(
    session_menu: &mut Option<SessionMenu>,
    tree_menu: &mut Option<TreeMenu>,
) -> bool {
    let had_open_menu = session_menu.is_some() || tree_menu.is_some();
    *session_menu = None;
    *tree_menu = None;
    had_open_menu
}

#[derive(Clone)]
pub(crate) struct DeleteConfirm {
    pub(crate) target: DeleteTarget,
    pub(crate) affected_sessions: usize,
}

#[derive(Clone)]
pub(crate) struct BootstrapConfirm {
    pub(crate) host: String,
    pub(crate) install: bool,
    pub(crate) upgrade: bool,
    pub(crate) binary: Option<String>,
}

impl MuxlaneApp {
    pub(crate) fn render_session_menu(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(menu) = self.session_menu.clone() else {
            return div().into_any_element();
        };
        div()
            .absolute()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                if dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu) {
                    cx.notify();
                }
            }))
            .on_any_mouse_down(
                cx.listener(|_this, _event: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                }),
            )
            .left(menu.position.x)
            .top(menu.position.y)
            .w(px(180.))
            .bg(rgba(theme.bg1))
            .border_1()
            .border_color(rgba(theme.line))
            .rounded_md()
            .shadow_lg()
            .child(
                div()
                    .id("session-delete")
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(rgba(theme.red))
                    .hover(|s| s.bg(rgba(theme.bg2)))
                    .on_click(cx.listener({
                        let id = menu.agent.clone();
                        let remote = menu.remote;
                        move |this, _ev, window, cx| this.delete_session(&id, remote, window, cx)
                    }))
                    .child(if menu.remote {
                        "删除远程会话"
                    } else {
                        "删除会话"
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn render_tree_menu(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(menu) = self.tree_menu.clone() else {
            return div().into_any_element();
        };
        let menu_el = match &menu.target {
            DeleteTarget::RemoteMachine { host } => {
                let host_name = host.clone();
                let host_name_2 = host.clone();
                let host_name_3 = host.clone();
                let host_obj = self.remotes.iter().find(|r| r.cfg.name == *host).cloned();
                div()
                    .w(px(200.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .id("tree-reconnect")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg0))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                if let Some(h) = &host_obj {
                                    h.reconnect();
                                    this.focus.focus(window, cx);
                                }
                                this.tree_menu = None;
                                cx.notify();
                            }))
                            .child("重新连接"),
                    )
                    .child(
                        div()
                            .id("tree-add-project")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg0))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                this.tree_menu = None;
                                this.remote_project_dialog = Some(host_name.clone());
                                this.dialog_error = None;
                                this.remote_project_input
                                    .update(cx, |input, cx| input.reset(cx));
                                this.remote_project_input.focus_handle(cx).focus(window, cx);
                                cx.notify();
                            }))
                            .child("添加远程项目…"),
                    )
                    .child(
                        div()
                            .id("tree-upgrade-muxlane")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.accent))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.tree_menu = None;
                                this.bootstrap_error = None;
                                this.bootstrap_confirm = Some(BootstrapConfirm {
                                    host: host_name_2.clone(),
                                    install: false,
                                    upgrade: true,
                                    binary: None,
                                });
                                cx.notify();
                            }))
                            .child("更新远端 Muxlane…"),
                    )
                    .child(
                        div()
                            .id("tree-reinstall-muxlane")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg1))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.tree_menu = None;
                                this.bootstrap_error = None;
                                this.bootstrap_confirm = Some(BootstrapConfirm {
                                    host: host_name_3.clone(),
                                    install: true,
                                    upgrade: false,
                                    binary: None,
                                });
                                cx.notify();
                            }))
                            .child("重新部署 / 安装远端 Muxlane…"),
                    )
                    .child(div().h(px(1.)).bg(rgba(theme.line)).my_1())
                    .child(
                        div()
                            .id("tree-delete")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.red))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener({
                                let target = menu.target.clone();
                                move |this, _event, _window, cx| {
                                    this.tree_menu = None;
                                    this.begin_delete(target.clone(), cx);
                                }
                            }))
                            .child("删除远程机器…"),
                    )
            }
            DeleteTarget::LocalProject { .. } | DeleteTarget::RemoteProject { .. } => {
                let label = match &menu.target {
                    DeleteTarget::LocalProject { .. } => "删除项目…",
                    DeleteTarget::RemoteProject { .. } => "删除远程项目…",
                    _ => unreachable!(),
                };
                div()
                    .w(px(190.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .id("tree-delete")
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgba(theme.red))
                            .hover(|style| style.bg(rgba(theme.bg2)))
                            .on_click(cx.listener({
                                let target = menu.target.clone();
                                move |this, _event, _window, cx| {
                                    this.tree_menu = None;
                                    this.begin_delete(target.clone(), cx);
                                }
                            }))
                            .child(label),
                    )
            }
        };
        div()
            .absolute()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                if dismiss_context_menus(&mut this.session_menu, &mut this.tree_menu) {
                    cx.notify();
                }
            }))
            .on_any_mouse_down(
                cx.listener(|_this, _event: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                }),
            )
            .left(menu.position.x)
            .top(menu.position.y)
            .child(menu_el)
            .into_any_element()
    }

    pub(crate) fn render_delete_confirm(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(confirm) = self.delete_confirm.clone() else {
            return div().into_any_element();
        };
        let (title, label, destructive_copy) = match &confirm.target {
            DeleteTarget::LocalProject { label, .. }
            | DeleteTarget::RemoteProject { label, .. } => (
                "删除项目",
                label.clone(),
                format!(
                    "将结束 {} 个 muxlane tmux 会话。项目文件和用户默认 tmux 不会删除。",
                    confirm.affected_sessions
                ),
            ),
            DeleteTarget::RemoteMachine { host } => (
                "删除远程机器连接",
                host.clone(),
                "只删除本地连接、镜像和 tunnel；目标机器上的项目、session 与 tmux 全部保留。"
                    .into(),
            ),
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme.overlay()))
            .child(
                div()
                    .w(px(460.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child(title),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg1))
                            .child(format!("{} · {}", label, destructive_copy)),
                    )
                    .when_some(self.delete_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .px_4()
                                .pt_2()
                                .text_size(px(11.))
                                .text_color(rgba(theme.red))
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .child(
                                div()
                                    .id("delete-confirm-cancel")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.delete_confirm = None;
                                        this.delete_error = None;
                                        this.delete_busy = false;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child({
                                let busy = self.delete_busy;
                                div()
                                    .id("delete-confirm-submit")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgba(theme.red))
                                    .text_color(rgba(theme.on_accent))
                                    .cursor_pointer()
                                    .when(!busy, |el| {
                                        el.hover(|style| {
                                            style.bg(rgba(Theme::with_alpha(theme.red, 0xcc)))
                                        })
                                        .active(|style| {
                                            style.bg(rgba(Theme::with_alpha(theme.red, 0x99)))
                                        })
                                    })
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        if !this.delete_busy {
                                            this.confirm_delete(cx);
                                        }
                                    }))
                                    .child(if busy { "删除中…" } else { "确认删除" })
                            }),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_bootstrap_confirm(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(confirm) = self.bootstrap_confirm.clone() else {
            return div().into_any_element();
        };
        let action = if confirm.upgrade {
            "更新并重启"
        } else if confirm.install {
            "安装并启动"
        } else {
            "启动并重连"
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme.overlay()))
            .child(
                div()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child(format!("{}远端 Muxlane", action)),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(12.))
                            .text_color(rgba(theme.fg1))
                            .child(format!(
                                "SSH 已连接到 {}。将使用当前认证方式{} headless 进程。",
                                confirm.host,
                                if confirm.upgrade {
                                    "上传新版本并重启"
                                } else if confirm.install {
                                    "上传当前 Muxlane 并启动"
                                } else {
                                    "启动"
                                }
                            )),
                    )
                    .when_some(self.bootstrap_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .px_4()
                                .pt_2()
                                .text_size(px(11.))
                                .text_color(rgba(theme.red))
                                .child(error),
                        )
                    })
                    .when_some(
                        self.bootstrap_progress.get(&confirm.host).cloned(),
                        |dialog, progress| {
                            let overall = progress.phase.overall(progress.percent);
                            let phase_text = format_upload_phase(&progress);
                            dialog
                                .child(
                                    div()
                                        .px_4()
                                        .pt_3()
                                        .text_size(px(11.))
                                        .text_color(rgba(theme.accent))
                                        .child(phase_text),
                                )
                                .child(
                                    div().mx_4().mt_2().h(px(4.)).bg(rgba(theme.bg2)).child(
                                        div()
                                            .w(relative(overall as f32 / 100.0))
                                            .h_full()
                                            .bg(rgba(theme.accent)),
                                    ),
                                )
                        },
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .child(
                                div()
                                    .id("bootstrap-cancel")
                                    .px_3()
                                    .py_1()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener({
                                        let host = confirm.host.clone();
                                        move |this, _event, _window, cx| {
                                            this.cancel_bootstrap_for_host(&host, cx);
                                        }
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("bootstrap-submit")
                                    .px_3()
                                    .py_1()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.confirm_bootstrap(cx)
                                    }))
                                    .child(action),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismissing_context_menus_clears_session_and_tree_menus() {
        let mut session_menu = Some(SessionMenu {
            agent: "agent-1".into(),
            position: Point::new(px(10.), px(20.)),
            remote: false,
        });
        let mut tree_menu = Some(TreeMenu {
            target: DeleteTarget::LocalProject {
                project: "project-1".into(),
                label: "demo".into(),
            },
            position: Point::new(px(30.), px(40.)),
        });

        assert!(dismiss_context_menus(&mut session_menu, &mut tree_menu));
        assert!(session_menu.is_none());
        assert!(tree_menu.is_none());
        assert!(!dismiss_context_menus(&mut session_menu, &mut tree_menu));
    }
}
