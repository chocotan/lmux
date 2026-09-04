//! Connection, local project, and remote project dialogs.
use crate::app::MuxlaneApp;
use crate::i18n;
use crate::theme::Theme;
use gpui::{
    div, prelude::*, px, rgba, Context, Focusable, MouseButton, ParentElement, Styled, Window,
};
use std::sync::Arc;

pub(crate) fn should_prompt_project_creation(
    error_code: Option<&str>,
    create_if_missing: bool,
) -> bool {
    !create_if_missing && error_code == Some(muxlane_core::protocol::error_codes::PATH_NOT_FOUND)
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
        if self.project_add_busy {
            return;
        }
        let path = raw_path.trim();
        if path.is_empty() {
            self.dialog_error =
                Some(i18n::text(self.language, "error.local_project_required").into());
            cx.notify();
            return;
        }
        self.submit_local_project(path.to_string(), false, cx);
    }

    pub(crate) fn submit_local_project(
        &mut self,
        path: String,
        create_if_missing: bool,
        cx: &mut Context<Self>,
    ) {
        if self.project_add_busy {
            return;
        }
        self.project_add_busy = true;
        let server = Arc::clone(&self.server);
        let requested_path = path.clone();
        let params = muxlane_core::protocol::ProjectAddParams {
            path,
            name: None,
            create_if_missing,
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    server.add_project(params).await?;
                    Ok::<_, anyhow::Error>(server.snapshot().await)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.project_add_busy = false;
                match result {
                    Ok(snapshot) => {
                        this.last_snapshot = snapshot;
                        this.project_dialog = false;
                        this.pending_project_creation = None;
                        this.dialog_error = None;
                        this.project_input.update(cx, |input, cx| input.reset(cx));
                        this.persist();
                    }
                    Err(error) => {
                        let error_code = error
                            .downcast_ref::<muxlane_server::ProjectAddError>()
                            .map(muxlane_server::ProjectAddError::code);
                        if should_prompt_project_creation(error_code, create_if_missing) {
                            this.pending_project_creation =
                                Some(crate::menus::PendingProjectCreation {
                                    target: crate::menus::ProjectCreationTarget::Local {
                                        path: requested_path,
                                    },
                                    error: None,
                                });
                            this.dialog_error = None;
                        } else if let Some(pending) = this.pending_project_creation.as_mut() {
                            pending.error = Some(error.to_string());
                        } else {
                            this.dialog_error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn handle_project_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        if self.pending_project_creation.is_some() {
            match ks.key.as_str() {
                "escape" => self.cancel_project_create(cx),
                "enter" => self.confirm_project_create(cx),
                _ => {}
            }
            return;
        }
        match ks.key.as_str() {
            "escape" if !self.project_add_busy => {
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
                            .child(i18n::text(self.language, "dialog.connect_remote")),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child(i18n::text(self.language, "dialog.connect_remote_help")),
                    )
                    .child(div().mx_4().mt_3().child(input))
                    .child(
                        div()
                            .mx_4()
                            .mt_2()
                            .flex()
                            .border_1()
                            .border_color(rgba(theme.line))
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
                                    .child(i18n::text(self.language, "dialog.auth_ssh_config")),
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
                                    .child(i18n::text(self.language, "dialog.auth_public_key")),
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
                                    .child(i18n::text(self.language, "dialog.auth_password")),
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
                                    .text_color(rgba(theme.fg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        this.connect_dialog = false;
                                        this.dialog_error = None;
                                        cx.notify();
                                    }))
                                    .child(i18n::text(self.language, "common.cancel")),
                            )
                            .child(
                                div()
                                    .id("connect-submit")
                                    .px_3()
                                    .py_1()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        let target = this.connect_input.read(cx).text();
                                        this.add_remote_target(target, cx);
                                    }))
                                    .child(i18n::text(self.language, "common.connect")),
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
        let busy = self.project_add_busy;
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
                    if !this.project_add_busy && this.pending_project_creation.is_none() {
                        this.remote_project_dialog = None;
                        this.dialog_error = None;
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
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
                                "escape" if this.pending_project_creation.is_some() => {
                                    this.cancel_project_create(cx);
                                    cx.stop_propagation();
                                }
                                "enter" if this.pending_project_creation.is_some() => {
                                    this.confirm_project_create(cx);
                                    cx.stop_propagation();
                                }
                                "escape" if !this.project_add_busy => {
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
                            .child(
                                i18n::text(self.language, "dialog.add_remote_project")
                                    .replace("{host}", &host),
                            ),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child(i18n::text(self.language, "dialog.add_remote_project_help")),
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
                                    .text_color(rgba(theme.fg0))
                                    .when(!busy, |button| {
                                        button.hover(|style| style.bg(rgba(theme.bg2)))
                                    })
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        if !this.project_add_busy {
                                            this.remote_project_dialog = None;
                                            this.dialog_error = None;
                                            cx.notify();
                                        }
                                    }))
                                    .child(i18n::text(self.language, "common.cancel")),
                            )
                            .child(
                                div()
                                    .id("remote-project-submit")
                                    .px_3()
                                    .py_1()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .when(!busy, |button| {
                                        button.hover(|style| {
                                            style.bg(rgba(Theme::with_alpha(theme.accent, 0xcc)))
                                        })
                                    })
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        if let Some(host) = this.remote_project_dialog.clone() {
                                            let path = this.remote_project_input.read(cx).text();
                                            this.submit_remote_project(host, path, cx);
                                        }
                                    }))
                                    .child(if busy {
                                        i18n::text(self.language, "common.adding")
                                    } else {
                                        i18n::text(self.language, "common.add")
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_project_dialog(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let input = self.project_input.clone();
        let error = self.dialog_error.clone();
        let busy = self.project_add_busy;
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
                    if !this.project_add_busy && this.pending_project_creation.is_none() {
                        this.project_dialog = false;
                        this.dialog_error = None;
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .occlude()
                    .w(px(480.))
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
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
                            .child(i18n::text(self.language, "dialog.add_local_project")),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .text_size(px(11.))
                            .text_color(rgba(theme.fg1))
                            .child(i18n::text(self.language, "dialog.add_local_project_help")),
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
                                    .text_color(rgba(theme.fg0))
                                    .when(!busy, |button| {
                                        button.hover(|style| style.bg(rgba(theme.bg2)))
                                    })
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        if !this.project_add_busy {
                                            this.project_dialog = false;
                                            this.dialog_error = None;
                                            cx.notify();
                                        }
                                    }))
                                    .child(i18n::text(self.language, "common.cancel")),
                            )
                            .child(
                                div()
                                    .id("project-submit")
                                    .px_3()
                                    .py_1()
                                    .bg(rgba(theme.accent))
                                    .text_color(rgba(theme.on_accent))
                                    .when(!busy, |button| {
                                        button.hover(|style| {
                                            style.bg(rgba(Theme::with_alpha(theme.accent, 0xcc)))
                                        })
                                    })
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        let path = this.project_input.read(cx).text();
                                        this.add_local_project(path, cx);
                                    }))
                                    .child(if busy {
                                        i18n::text(self.language, "common.adding")
                                    } else {
                                        i18n::text(self.language, "common.add")
                                    }),
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
    fn project_creation_prompt_requires_first_typed_not_found_error() {
        assert!(should_prompt_project_creation(
            Some(muxlane_core::protocol::error_codes::PATH_NOT_FOUND),
            false
        ));
        assert!(!should_prompt_project_creation(
            Some(muxlane_core::protocol::error_codes::PATH_NOT_FOUND),
            true
        ));
        assert!(!should_prompt_project_creation(
            Some(muxlane_core::protocol::error_codes::NOT_A_DIRECTORY),
            false
        ));
        assert!(!should_prompt_project_creation(None, false));
    }
}
