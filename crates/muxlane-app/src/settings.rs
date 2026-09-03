//! Settings page and appearance controls.
use crate::app::MuxlaneApp;
use crate::i18n::{self, Language};
use crate::theme::{Theme, ThemeMode};
use gpui::{deferred, div, prelude::*, px, rgba, Context, MouseButton, ParentElement, Styled};

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
        self.dismiss_settings_menus();
        self.persist();
        cx.notify();
    }

    pub(crate) fn render_settings(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::for_mode(self.theme_mode);
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
                cx.listener(|this, _event, _window, cx| {
                    this.settings_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("settings-page")
                    .relative()
                    .occlude()
                    .w(px(560.))
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
                    .on_key_down(
                        cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                            if event.keystroke.key.as_str() == "escape" {
                                this.settings_open = false;
                                cx.notify();
                            }
                        }),
                    )
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
                                    .child(i18n::text(self.language, "设置", "Settings")),
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
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.settings_open = false;
                                        cx.notify();
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
                            .child(i18n::text(self.language, "主题", "Theme")),
                    )
                    .child(
                        div()
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div().text_size(px(12.)).text_color(rgba(theme.fg0)).child(
                                    i18n::text(self.language, "界面主题", "Interface theme"),
                                ),
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
                                                    .child(if self.language == Language::English {
                                                        self.theme_mode.label_en()
                                                    } else {
                                                        self.theme_mode.label()
                                                    }),
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
                                                                .child(
                                                                    if language == Language::English
                                                                    {
                                                                        mode.label_en()
                                                                    } else {
                                                                        mode.label()
                                                                    },
                                                                ),
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
                            .child(i18n::text(self.language, "终端字体", "Terminal font")),
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
                                    .child(i18n::text(self.language, "语言", "Language")),
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
                                    i18n::text(self.language, "通知声音", "Notification sound"),
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
                                        i18n::text(self.language, "已开启", "Enabled")
                                    } else {
                                        i18n::text(self.language, "已关闭", "Disabled")
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
                            .child(div().text_size(px(12.)).text_color(rgba(theme.fg0)).child(
                                i18n::text(
                                    self.language,
                                    "允许终端写入剪贴板 (OSC52)",
                                    "Allow terminal clipboard writes (OSC52)",
                                ),
                            ))
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
                                        i18n::text(self.language, "已开启", "Enabled")
                                    } else {
                                        i18n::text(self.language, "已关闭", "Disabled")
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}
