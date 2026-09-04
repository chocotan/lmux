//! Settings page and appearance controls.
use crate::app::MuxlaneApp;
use crate::i18n::{self, Language};
use crate::shortcuts::{self, ShortcutAction, ShortcutError};
use crate::theme::{Theme, ThemeMode};
use gpui::{
    deferred, div, prelude::*, px, rgba, Context, MouseButton, ParentElement, Styled, Window,
};

pub(crate) const FONT_FAMILIES: &[&str] = &[
    "Noto Sans Mono",
    "JetBrains Mono",
    "Iosevka",
    "DejaVu Sans Mono",
    "Liberation Mono",
];
pub(crate) const DEFAULT_FONT_FAMILY: &str = "Noto Sans Mono";

impl MuxlaneApp {
    pub(crate) fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        self.theme_mode = if self.theme_mode.is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        self.apply_theme_to_inputs(cx);
        self.persist();
        cx.notify();
    }

    fn apply_theme_to_inputs(&mut self, cx: &mut Context<Self>) {
        let mode = self.theme_mode;
        self.notifications.update(cx, |center, cx| {
            center.set_appearance(mode, self.language, cx)
        });
        self.palette_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_username
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_password
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.connect_key_path
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.project_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        self.remote_project_input
            .update(cx, |input, cx| input.set_theme_mode(mode, cx));
        let theme = Theme::for_mode(self.theme_mode);
        for term in self.terms.values() {
            term.update(cx, |term, cx| term.set_theme(theme, cx));
        }
    }

    fn dismiss_settings_menus(&mut self) {
        self.settings_theme_menu = false;
        self.settings_font_menu = false;
        self.settings_language_menu = false;
    }

    fn set_theme(&mut self, mode: ThemeMode, cx: &mut Context<Self>) {
        self.theme_mode = mode;
        self.dismiss_settings_menus();
        self.apply_theme_to_inputs(cx);
        self.persist();
        cx.notify();
    }

    fn set_font_family(&mut self, font_family: &str, cx: &mut Context<Self>) {
        self.font_family = font_family.to_string();
        self.dismiss_settings_menus();
        let family = self.font_family.clone();
        let theme = Theme::for_mode(self.theme_mode);
        for term in self.terms.values() {
            let family = family.clone();
            term.update(cx, |term, cx| {
                term.set_font_family(family, cx);
                term.set_theme(theme, cx);
            });
        }
        self.persist();
        cx.notify();
    }

    fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        self.language = language;
        self.notifications.update(cx, |center, cx| {
            center.set_appearance(self.theme_mode, language, cx)
        });
        for (input, key) in [
            (&self.palette_input, "palette.placeholder"),
            (&self.connect_input, "placeholder.remote_target"),
            (&self.connect_username, "placeholder.username"),
            (&self.connect_password, "placeholder.password"),
            (&self.connect_key_path, "placeholder.private_key"),
        ] {
            input.update(cx, |input, cx| {
                input.set_placeholder(i18n::text(language, key), cx)
            });
        }
        self.dismiss_settings_menus();
        self.persist();
        cx.notify();
    }

    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_shortcut_capture();
        self.settings_open = false;
        self.dismiss_settings_menus();
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn cancel_shortcut_capture(&mut self) {
        self.shortcut_capture_subscription.take();
        self.shortcut_capture = None;
    }

    fn start_shortcut_capture(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        self.cancel_shortcut_capture();
        self.shortcut_capture = Some(action);
        self.shortcut_error = None;
        let listener = cx.listener(|this, event: &gpui::KeystrokeEvent, _window, cx| {
            cx.stop_propagation();
            if event.keystroke.key.as_str() == "escape" {
                this.cancel_shortcut_capture();
                cx.notify();
                return;
            }
            let Some(action) = this.shortcut_capture else {
                return;
            };
            match shortcuts::captured_chord(&event.keystroke) {
                Ok(chord) => this.apply_shortcut_binding(action, Some(chord), cx),
                Err(error) => {
                    this.shortcut_error = Some(error);
                    cx.notify();
                }
            }
        });
        self.shortcut_capture_subscription = Some(cx.intercept_keystrokes(listener));
        cx.notify();
    }

    fn apply_shortcut_binding(
        &mut self,
        action: ShortcutAction,
        binding: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match shortcuts::install_binding(cx, &self.shortcut_bindings, action, binding) {
            Ok(normalized) => {
                self.shortcut_bindings = normalized;
                self.shortcut_error = None;
                self.cancel_shortcut_capture();
                self.persist();
            }
            Err(error) => self.shortcut_error = Some(error),
        }
        cx.notify();
    }

    fn restore_default_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.cancel_shortcut_capture();
        let defaults = muxlane_store::PersistedShortcutBindings::default();
        match shortcuts::install_keymap(cx, &defaults) {
            Ok(normalized) => {
                self.shortcut_bindings = normalized;
                self.shortcut_error = None;
                self.persist();
            }
            Err(error) => self.shortcut_error = Some(error),
        }
        cx.notify();
    }

    fn shortcut_error_text(&self) -> Option<String> {
        self.shortcut_error.as_ref().map(|error| match error {
            ShortcutError::Invalid => {
                i18n::text(self.language, "settings.shortcut_error_invalid").into()
            }
            ShortcutError::MultipleChords => {
                i18n::text(self.language, "settings.shortcut_error_multiple").into()
            }
            ShortcutError::Conflict(chord) => {
                i18n::text(self.language, "settings.shortcut_error_conflict")
                    .replace("{shortcut}", chord)
            }
        })
    }

    pub(crate) fn render_settings(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
        let project_workspaces_enabled = self.workspace.enabled();
        let shortcut_bindings = self.shortcut_bindings.clone();
        let shortcut_capture = self.shortcut_capture;
        let shortcut_error = self.shortcut_error_text();
        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(48.))
            .bg(rgba(theme.overlay()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    this.close_settings(window, cx);
                }),
            )
            .child(
                div()
                    .id("settings-page")
                    .relative()
                    .occlude()
                    .w(px(560.))
                    .max_h(px(700.))
                    .overflow_y_scroll()
                    .bg(rgba(theme.bg1))
                    .border_1()
                    .border_color(rgba(theme.line))
                    .shadow_lg()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _event, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key.as_str() == "escape" {
                            this.close_settings(window, cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgba(theme.line))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgba(theme.fg0))
                                    .child(i18n::text(self.language, "common.settings")),
                            )
                            .child(
                                div()
                                    .id("settings-close")
                                    .w(px(24.))
                                    .h(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .text_color(rgba(theme.fg1))
                                    .hover(|s| s.bg(rgba(theme.bg2)).text_color(rgba(theme.fg0)))
                                    .on_click(cx.listener(|this, _event, window, cx| {
                                        this.close_settings(window, cx);
                                    }))
                                    .child("×"),
                            ),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg2))
                            .child(i18n::text(self.language, "settings.workspaces")),
                    )
                    .child(
                        div()
                            .px_4()
                            .pb_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div().text_size(px(12.)).text_color(rgba(theme.fg0)).child(
                                            i18n::text(
                                                self.language,
                                                "settings.project_workspaces",
                                            ),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .mt_1()
                                            .text_size(px(10.))
                                            .text_color(rgba(theme.fg2))
                                            .child(i18n::text(
                                                self.language,
                                                "settings.project_workspaces_help",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("settings-project-workspaces-toggle")
                                    .h(px(30.))
                                    .px_2()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .border_1()
                                    .border_color(rgba(if project_workspaces_enabled {
                                        theme.accent
                                    } else {
                                        theme.line
                                    }))
                                    .text_size(px(11.))
                                    .text_color(rgba(if project_workspaces_enabled {
                                        theme.accent
                                    } else {
                                        theme.fg2
                                    }))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, window, cx| {
                                        this.set_project_workspaces_enabled(
                                            !this.workspace.enabled(),
                                            window,
                                            cx,
                                        );
                                    }))
                                    .child(if project_workspaces_enabled {
                                        i18n::text(self.language, "common.enabled")
                                    } else {
                                        i18n::text(self.language, "common.disabled")
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgba(theme.fg2))
                                    .child(i18n::text(self.language, "settings.shortcuts")),
                            )
                            .child(
                                div()
                                    .id("settings-shortcuts-restore")
                                    .h(px(28.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_1()
                                    .border_color(rgba(theme.line))
                                    .text_size(px(10.))
                                    .text_color(rgba(theme.fg1))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.restore_default_shortcuts(cx);
                                    }))
                                    .child(i18n::text(self.language, "settings.shortcuts_restore")),
                            ),
                    )
                    .children(ShortcutAction::ALL.into_iter().map(|action| {
                        let recording = shortcut_capture == Some(action);
                        let binding = action.binding(&shortcut_bindings).clone();
                        let record_id = format!("settings-shortcut-record-{action:?}");
                        let clear_id = format!("settings-shortcut-clear-{action:?}");
                        div()
                            .px_4()
                            .pb_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(11.))
                                    .text_color(rgba(theme.fg0))
                                    .child(i18n::text(self.language, action.label_key())),
                            )
                            .child(
                                div()
                                    .id(gpui::ElementId::Name(record_id.into()))
                                    .w(px(170.))
                                    .h(px(30.))
                                    .px_2()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .border_1()
                                    .border_color(rgba(if recording {
                                        theme.accent
                                    } else {
                                        theme.line
                                    }))
                                    .bg(rgba(theme.bg0))
                                    .text_size(px(10.))
                                    .text_color(rgba(if recording {
                                        theme.accent
                                    } else {
                                        theme.fg1
                                    }))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.start_shortcut_capture(action, cx);
                                    }))
                                    .child(if recording {
                                        i18n::text(self.language, "settings.shortcut_recording")
                                            .to_string()
                                    } else {
                                        binding.unwrap_or_else(|| {
                                            i18n::text(self.language, "settings.shortcut_disabled")
                                                .into()
                                        })
                                    }),
                            )
                            .child(
                                div()
                                    .id(gpui::ElementId::Name(clear_id.into()))
                                    .w(px(58.))
                                    .h(px(30.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .border_1()
                                    .border_color(rgba(theme.line))
                                    .text_size(px(10.))
                                    .text_color(rgba(theme.fg2))
                                    .hover(|style| style.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.cancel_shortcut_capture();
                                        this.apply_shortcut_binding(action, None, cx);
                                    }))
                                    .child(i18n::text(self.language, "common.clear")),
                            )
                    }))
                    .when_some(shortcut_error, |page, error| {
                        page.child(
                            div()
                                .px_4()
                                .pb_2()
                                .text_size(px(10.))
                                .text_color(rgba(theme.red))
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg2))
                            .child(i18n::text(self.language, "settings.theme")),
                    )
                    .child(
                        div()
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgba(theme.fg0))
                                    .child(i18n::text(self.language, "settings.interface_theme")),
                            )
                            .child({
                                let selected = Theme::for_mode(self.theme_mode);
                                let language = self.language;
                                let current_mode = self.theme_mode;
                                div()
                                    .relative()
                                    .child(
                                        div()
                                            .id("settings-theme-select")
                                            .w(px(210.))
                                            .h(px(32.))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .border_1()
                                            .border_color(rgba(theme.line))
                                            .bg(rgba(theme.bg0))
                                            .hover(|s| s.bg(rgba(theme.bg2)))
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                let open = !this.settings_theme_menu;
                                                this.dismiss_settings_menus();
                                                this.settings_theme_menu = open;
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .w(px(24.))
                                                    .h(px(16.))
                                                    .bg(rgba(selected.bg0))
                                                    .border_1()
                                                    .border_color(rgba(selected.accent)),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(11.))
                                                    .text_color(rgba(theme.fg0))
                                                    .child(self.theme_mode.label(self.language)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(rgba(theme.fg1))
                                                    .child(if self.settings_theme_menu {
                                                        "⌃"
                                                    } else {
                                                        "⌄"
                                                    }),
                                            ),
                                    )
                                    .when(self.settings_theme_menu, |anchor| {
                                        anchor.child(
                                            deferred(
                                                div()
                                                    .id("settings-theme-menu")
                                                    .absolute()
                                                    .top_full()
                                                    .left_0()
                                                    .w(px(210.))
                                                    .max_h(px(280.))
                                                    .overflow_y_scroll()
                                                    .bg(rgba(theme.bg1))
                                                    .border_1()
                                                    .border_color(rgba(theme.line))
                                                    .shadow_lg()
                                                    .occlude()
                                                    .children(ThemeMode::ALL.into_iter().map(
                                                        |mode| {
                                                            let selected = mode == current_mode;
                                                            let swatch = Theme::for_mode(mode);
                                                            div()
                                                        .id(gpui::ElementId::Name(
                                                            format!(
                                                                "settings-theme-option-{}",
                                                                mode.id()
                                                            )
                                                            .into(),
                                                        ))
                                                        .h(px(30.))
                                                        .px_2()
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .when(selected, |el| el.bg(rgba(theme.bg2)))
                                                        .when(!selected, |el| {
                                                            el.hover(|s| s.bg(rgba(theme.bg2)))
                                                        })
                                                        .on_click(cx.listener(
                                                            move |this, _event, _window, cx| {
                                                                this.set_theme(mode, cx);
                                                            },
                                                        ))
                                                        .child(
                                                            div()
                                                                .w(px(22.))
                                                                .h(px(14.))
                                                                .bg(rgba(swatch.bg0))
                                                                .border_1()
                                                                .border_color(rgba(swatch.accent)),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .text_size(px(11.))
                                                                .text_color(rgba(theme.fg0))
                                                                .child(mode.label(language)),
                                                        )
                                                        .child(if selected { "✓" } else { "" })
                                                        },
                                                    )),
                                            )
                                            .with_priority(1),
                                        )
                                    })
                            }),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgba(theme.fg2))
                            .child(i18n::text(self.language, "settings.terminal_font")),
                    )
                    .child(div().px_4().pb_4().flex().justify_end().child({
                        let current_font = self.font_family.clone();
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("settings-font-select")
                                    .w(px(260.))
                                    .h(px(32.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_1()
                                    .border_color(rgba(theme.line))
                                    .bg(rgba(theme.bg0))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        let open = !this.settings_font_menu;
                                        this.dismiss_settings_menus();
                                        this.settings_font_menu = open;
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(11.))
                                            .text_color(rgba(theme.fg0))
                                            .font_family(self.font_family.clone())
                                            .child(self.font_family.clone()),
                                    )
                                    .child(
                                        div().text_size(px(12.)).text_color(rgba(theme.fg1)).child(
                                            if self.settings_font_menu {
                                                "⌃"
                                            } else {
                                                "⌄"
                                            },
                                        ),
                                    ),
                            )
                            .when(self.settings_font_menu, |anchor| {
                                anchor.child(
                                    deferred(
                                        div()
                                            .id("settings-font-menu")
                                            .absolute()
                                            .top_full()
                                            .left_0()
                                            .w(px(260.))
                                            .max_h(px(280.))
                                            .overflow_y_scroll()
                                            .bg(rgba(theme.bg1))
                                            .border_1()
                                            .border_color(rgba(theme.line))
                                            .shadow_lg()
                                            .occlude()
                                            .children(FONT_FAMILIES.iter().map(|family| {
                                                let selected = current_font == *family;
                                                let family = (*family).to_string();
                                                div()
                                                    .id(gpui::ElementId::Name(
                                                        format!(
                                                            "settings-font-option-{}",
                                                            family.replace(' ', "-")
                                                        )
                                                        .into(),
                                                    ))
                                                    .h(px(30.))
                                                    .px_2()
                                                    .flex()
                                                    .items_center()
                                                    .when(selected, |el| el.bg(rgba(theme.bg2)))
                                                    .when(!selected, |el| {
                                                        el.hover(|s| s.bg(rgba(theme.bg2)))
                                                    })
                                                    .on_click(cx.listener({
                                                        let family = family.clone();
                                                        move |this, _event, _window, cx| {
                                                            this.set_font_family(&family, cx);
                                                        }
                                                    }))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .text_size(px(11.))
                                                            .text_color(rgba(theme.fg0))
                                                            .font_family(family.clone())
                                                            .child(family),
                                                    )
                                                    .child(if selected { "✓" } else { "" })
                                            })),
                                    )
                                    .with_priority(1),
                                )
                            })
                    }))
                    .child(
                        div()
                            .px_4()
                            .pb_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgba(theme.fg0))
                                    .child(i18n::text(self.language, "settings.language")),
                            )
                            .child({
                                let current_language = self.language;
                                div()
                                    .relative()
                                    .child(
                                        div()
                                            .id("settings-language-select")
                                            .w(px(180.))
                                            .h(px(32.))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .border_1()
                                            .border_color(rgba(theme.line))
                                            .bg(rgba(theme.bg0))
                                            .hover(|s| s.bg(rgba(theme.bg2)))
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                let open = !this.settings_language_menu;
                                                this.dismiss_settings_menus();
                                                this.settings_language_menu = open;
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(11.))
                                                    .text_color(rgba(theme.fg0))
                                                    .child(self.language.label()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(rgba(theme.fg1))
                                                    .child(if self.settings_language_menu {
                                                        "⌃"
                                                    } else {
                                                        "⌄"
                                                    }),
                                            ),
                                    )
                                    .when(self.settings_language_menu, |anchor| {
                                        anchor.child(
                                            deferred(
                                                div()
                                                    .id("settings-language-menu")
                                                    .absolute()
                                                    .top_full()
                                                    .left_0()
                                                    .w(px(180.))
                                                    .bg(rgba(theme.bg1))
                                                    .border_1()
                                                    .border_color(rgba(theme.line))
                                                    .shadow_lg()
                                                    .occlude()
                                                    .children(Language::ALL.into_iter().map(
                                                        |language| {
                                                            let selected =
                                                                language == current_language;
                                                            div()
                                                            .id(gpui::ElementId::Name(
                                                                format!(
                                                                    "settings-language-option-{}",
                                                                    language.id()
                                                                )
                                                                .into(),
                                                            ))
                                                            .h(px(30.))
                                                            .px_2()
                                                            .flex()
                                                            .items_center()
                                                            .when(selected, |el| {
                                                                el.bg(rgba(theme.bg2))
                                                            })
                                                            .when(!selected, |el| {
                                                                el.hover(|s| s.bg(rgba(theme.bg2)))
                                                            })
                                                            .on_click(cx.listener(
                                                                move |this, _event, _window, cx| {
                                                                    this.set_language(language, cx);
                                                                },
                                                            ))
                                                            .child(language.label())
                                                        },
                                                    )),
                                            )
                                            .with_priority(1),
                                        )
                                    })
                            }),
                    )
                    .child(
                        div()
                            .px_4()
                            .pb_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div().text_size(px(12.)).text_color(rgba(theme.fg0)).child(
                                    i18n::text(self.language, "settings.notification_sound"),
                                ),
                            )
                            .child(
                                div()
                                    .id("settings-sound-toggle")
                                    .h(px(30.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_1()
                                    .border_color(rgba(if self.sound_enabled {
                                        theme.accent
                                    } else {
                                        theme.line
                                    }))
                                    .text_size(px(11.))
                                    .text_color(rgba(if self.sound_enabled {
                                        theme.accent
                                    } else {
                                        theme.fg2
                                    }))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.sound_enabled = !this.sound_enabled;
                                        this.persist();
                                        cx.notify();
                                    }))
                                    .child(if self.sound_enabled {
                                        i18n::text(self.language, "common.enabled")
                                    } else {
                                        i18n::text(self.language, "common.disabled")
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .px_4()
                            .pb_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgba(theme.fg0))
                                    .child(i18n::text(self.language, "settings.osc52")),
                            )
                            .child(
                                div()
                                    .id("settings-osc52-toggle")
                                    .h(px(30.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_1()
                                    .border_color(rgba(if self.osc52_clipboard_enabled {
                                        theme.accent
                                    } else {
                                        theme.line
                                    }))
                                    .text_size(px(11.))
                                    .text_color(rgba(if self.osc52_clipboard_enabled {
                                        theme.accent
                                    } else {
                                        theme.fg2
                                    }))
                                    .hover(|s| s.bg(rgba(theme.bg2)))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.toggle_osc52_clipboard(cx);
                                    }))
                                    .child(if self.osc52_clipboard_enabled {
                                        i18n::text(self.language, "common.enabled")
                                    } else {
                                        i18n::text(self.language, "common.disabled")
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl MuxlaneApp {
    pub(crate) fn toggle_osc52_clipboard(&mut self, cx: &mut Context<Self>) {
        self.osc52_clipboard_enabled = !self.osc52_clipboard_enabled;
        for term in self.terms.values() {
            term.update(cx, |term, _cx| {
                term.set_osc52_clipboard_enabled(self.osc52_clipboard_enabled)
            });
        }
        self.persist();
        cx.notify();
    }
}
