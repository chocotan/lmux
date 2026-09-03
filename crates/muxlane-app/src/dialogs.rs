//! Connection, local project, and remote project dialogs.
use crate::app::MuxlaneApp;
use crate::theme::Theme;
use gpui::{
    div, prelude::*, px, rgba, Context, Focusable, MouseButton, ParentElement, Styled, Window,
};
use std::sync::Arc;

fn resolve_local_project_path(raw_path: &str) -> Option<std::path::PathBuf> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() {
        return None;
    }
    let expanded = if raw_path == "~" {
        std::env::var_os("HOME").map(std::path::PathBuf::from)?
    } else if let Some(rest) = raw_path.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)?
            .join(rest)
    } else {
        std::path::PathBuf::from(raw_path)
    };
    expanded.canonicalize().ok().filter(|path| path.is_dir())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectAuthMode {
    SshConfig,
    PublicKey,
    Password,
}

impl MuxlaneApp {
    fn cycle_connect_focus(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fields = match self.connect_auth_mode {
            ConnectAuthMode::SshConfig => vec![self.connect_input.focus_handle(cx)],
            ConnectAuthMode::PublicKey => vec![
                self.connect_input.focus_handle(cx),
                self.connect_username.focus_handle(cx),
                self.connect_key_path.focus_handle(cx),
            ],
            ConnectAuthMode::Password => vec![
                self.connect_input.focus_handle(cx),
                self.connect_username.focus_handle(cx),
                self.connect_password.focus_handle(cx),
            ],
        };
        if backwards {
            self.connect_focus_index = self
                .connect_focus_index
                .checked_sub(1)
                .unwrap_or(fields.len().saturating_sub(1));
        } else {
            self.connect_focus_index = (self.connect_focus_index + 1) % fields.len().max(1);
        }
        if let Some(focus) = fields.get(self.connect_focus_index) {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    fn handle_connect_key(
        &mut self,
        ks: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ks.key.as_str() {
            "escape" => {
                self.connect_dialog = false;
                self.dialog_error = None;
            }
            "tab" => {
                self.cycle_connect_focus(ks.modifiers.shift, window, cx);
                return;
            }
            "enter" => {
                let target = self.connect_input.read(cx).text();
                self.add_remote_target(target, cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn add_local_project(&mut self, raw_path: String, cx: &mut Context<Self>) {
        let raw_path = raw_path.trim();
        if raw_path.is_empty() {
            self.dialog_error = Some("请输入本地项目目录".into());
            cx.notify();
            return;
        }
        let Some(path) = resolve_local_project_path(raw_path) else {
            self.dialog_error = Some("目录不存在或不是文件夹".into());
            cx.notify();
            return;
        };
        if self
            .last_snapshot
            .projects
            .iter()
            .any(|project| project.path == path)
        {
            self.dialog_error = Some("这个项目已经存在".into());
            cx.notify();
            return;
        }
        let server = Arc::clone(&self.server);
        let params = muxlane_core::protocol::ProjectAddParams {
            path: path.display().to_string(),
            name: None,
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    server.add_project(params).await?;
                    Ok::<_, anyhow::Error>(server.snapshot().await)
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(snapshot) => {
                    this.last_snapshot = snapshot;
                    this.project_dialog = false;
                    this.dialog_error = None;
                    this.project_input.update(cx, |input, cx| input.reset(cx));
                    this.persist();
                    cx.notify();
                }
                Err(error) => {
                    this.dialog_error = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn handle_project_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        match ks.key.as_str() {
            "escape" => {
                self.project_dialog = false;
                self.dialog_error = None;
            }
            "enter" => {
                let path = self.project_input.read(cx).text();
                self.add_local_project(path, cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    pub(crate) fn render_connect_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let input = self.connect_input.clone();
        let username = self.connect_username.clone();
        let password = self.connect_password.clone();
        let key_path = self.connect_key_path.clone();
        let auth_mode = self.connect_auth_mode;
        let error = self.dialog_error.clone();
        div()
            .id("connect-dialog-backdrop")
            .absolute()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(90.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, cx| {
                    this.connect_dialog = false;
                    this.dialog_error = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _ev, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, window, cx| {
                        if matches!(ev.keystroke.key.as_str(), "enter" | "escape" | "tab") {
                            this.handle_connect_key(&ev.keystroke, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child("连接远程机器"),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child("输入 SSH Host 或 ~/.ssh/config 别名；socket 自动发现"),
                    )
                    .child(div().mx_4().mt_3().child(input))
                    .child(
                        div()
                            .mx_4()
                            .mt_2()
                            .flex()
                            .border_1()
                            .border_color(rgba(theme.line))
                            .rounded_sm()
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("auth-config")
                                    .flex_1()
                                    .px_2()
                                    .py_1()
                                    .text_size(px(11.))
                                    .text_color(rgba(theme.fg1))
                                    .when(auth_mode == ConnectAuthMode::SshConfig, |item| {
                                        item.bg(rgba(theme.accent))
                                            .text_color(rgba(theme.on_accent))
                                    })
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.connect_auth_mode = ConnectAuthMode::SshConfig;
                                        cx.notify();
                                    }))
                                    .child("SSH 配置"),
                            )
                            .child(
                                div()
                                    .id("auth-key")
                                    .flex_1()
                                    .px_2()
                                    .py_1()
                                    .text_size(px(11.))
                                    .text_color(rgba(theme.fg1))
                                    .when(auth_mode == ConnectAuthMode::PublicKey, |item| {
                                        item.bg(rgba(theme.accent))
                                            .text_color(rgba(theme.on_accent))
                                    })
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.connect_auth_mode = ConnectAuthMode::PublicKey;
                                        cx.notify();
                                    }))
                                    .child("SSH 公钥"),
                            )
                            .child(
                                div()
                                    .id("auth-password")
                                    .flex_1()
                                    .px_2()
                                    .py_1()
                                    .text_size(px(11.))
                                    .text_color(rgba(theme.fg1))
                                    .when(auth_mode == ConnectAuthMode::Password, |item| {
                                        item.bg(rgba(theme.accent))
                                            .text_color(rgba(theme.on_accent))
                                    })
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.connect_auth_mode = ConnectAuthMode::Password;
                                        cx.notify();
                                    }))
                                    .child("用户名密码"),
                            ),
                    )
                    .when(auth_mode == ConnectAuthMode::PublicKey, |dialog| {
                        dialog
                            .child(div().mx_4().mt_2().child(username.clone()))
                            .child(div().mx_4().mt_2().child(key_path))
                    })
                    .when(auth_mode == ConnectAuthMode::Password, |dialog| {
                        dialog
                            .child(div().mx_4().mt_2().child(username))
                            .child(div().mx_4().mt_2().child(password))
                    })
                    .when_some(error, |dialog, error| {
                        dialog.child(
                            div()
                                .mx_4()
                                .mt_2()
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
                                    .id("connect-cancel")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        this.connect_dialog = false;
                                        this.dialog_error = None;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("connect-submit")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        let target = this.connect_input.read(cx).text();
                                        this.add_remote_target(target, cx);
                                    }))
                                    .child("连接"),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_remote_project_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let Some(host) = self.remote_project_dialog.clone() else {
            return div().into_any_element();
        };
        let input = self.remote_project_input.clone();
        div()
            .id("remote-project-backdrop")
            .absolute()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(90.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.remote_project_dialog = None;
                    this.dialog_error = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _ev, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_key_down(
                        cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                            match event.keystroke.key.as_str() {
                                "escape" => {
                                    this.remote_project_dialog = None;
                                    this.dialog_error = None;
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                                "enter" => {
                                    if let Some(host) = this.remote_project_dialog.clone() {
                                        let path = this.remote_project_input.read(cx).text();
                                        this.submit_remote_project(host, path, cx);
                                    }
                                    cx.stop_propagation();
                                }
                                _ => {}
                            }
                        }),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child(format!("在 {host} 添加项目")),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child("输入远端已存在的目录；不会上传或删除项目文件"),
                    )
                    .child(div().mx_4().mt_3().child(input))
                    .when_some(self.dialog_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .mx_4()
                                .mt_2()
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
                                    .id("remote-project-cancel")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.remote_project_dialog = None;
                                        this.dialog_error = None;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("remote-project-submit")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        if let Some(host) = this.remote_project_dialog.clone() {
                                            let path = this.remote_project_input.read(cx).text();
                                            this.submit_remote_project(host, path, cx);
                                        }
                                    }))
                                    .child("添加"),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_project_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let input = self.project_input.clone();
        let error = self.dialog_error.clone();
        div()
            .id("project-dialog-backdrop")
            .absolute()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(90.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, cx| {
                    this.project_dialog = false;
                    this.dialog_error = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .rounded_md()
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _ev, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                        if matches!(ev.keystroke.key.as_str(), "enter" | "escape") {
                            this.handle_project_key(&ev.keystroke, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg0))
                            .child("添加本地项目"),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child("输入已有项目目录；远程项目由连接机器后自动发现"),
                    )
                    .child(div().mx_4().mt_3().child(input))
                    .when_some(error, |dialog, error| {
                        dialog.child(
                            div()
                                .mx_4()
                                .mt_2()
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
                                    .id("project-cancel")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .text_color(rgba(theme.fg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        this.project_dialog = false;
                                        this.dialog_error = None;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("project-submit")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        let path = this.project_input.read(cx).text();
                                        this.add_local_project(path, cx);
                                    }))
                                    .child("添加"),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn open_connect_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.connect_dialog = true;
        self.connect_focus_index = 0;
        self.project_dialog = false;
        self.dialog_error = None;
        self.connect_input.update(cx, |input, cx| input.reset(cx));
        self.connect_username
            .update(cx, |input, cx| input.reset(cx));
        self.connect_password
            .update(cx, |input, cx| input.reset(cx));
        self.connect_key_path
            .update(cx, |input, cx| input.reset(cx));
        self.connect_input.focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_project_path_requires_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_local_project_path(dir.path().to_str().unwrap()),
            Some(dir.path().canonicalize().unwrap())
        );
        assert!(resolve_local_project_path("/definitely/missing/muxlane-project").is_none());
    }
}
