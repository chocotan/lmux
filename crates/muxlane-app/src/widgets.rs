use crate::i18n::{self, Language};
use crate::theme::Theme;
use gpui::{div, prelude::*, px, rgba, Context, Pixels, Point, Render, SharedString, Window};

pub(crate) struct DragGhost {
    pub(crate) label: SharedString,
    pub(crate) offset: Point<Pixels>,
    pub(crate) theme: Theme,
}
impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.offset.x.max(px(0.0)))
            .pt(self.offset.y.max(px(0.0)))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .bg(rgba(self.theme.bg2))
                    .border_1()
                    .border_color(rgba(self.theme.line))
                    .text_size(px(11.))
                    .text_color(rgba(self.theme.fg0))
                    .child(self.label.clone()),
            )
    }
}

pub(crate) struct DividerDragGhost;

impl Render for DividerDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(1.)).h(px(1.))
    }
}

pub(crate) fn format_relative_time(then: u64, lang: Language) -> String {
    let now = muxlane_core::model::now_secs();
    let diff = now.saturating_sub(then);
    if diff < 10 {
        i18n::text(lang, "relative.just_now").to_string()
    } else if diff < 60 {
        i18n::text(lang, "relative.seconds_ago").replace("{count}", &diff.to_string())
    } else if diff < 3600 {
        i18n::text(lang, "relative.minutes_ago").replace("{count}", &(diff / 60).to_string())
    } else if diff < 86400 {
        i18n::text(lang, "relative.hours_ago").replace("{count}", &(diff / 3600).to_string())
    } else {
        i18n::text(lang, "relative.days_ago").replace("{count}", &(diff / 86400).to_string())
    }
}

fn render_pi_loading_spinner(frame: usize, theme: Theme) -> gpui::Div {
    let empty_index = frame % 8;
    // 顺时针 8 点阵索引：左列 [0, 7, 6, 5]，右列 [1, 2, 3, 4]
    let left_indices = [0, 7, 6, 5];
    let right_indices = [1, 2, 3, 4];

    let render_col = |indices: [usize; 4]| {
        let mut col = div().flex().flex_col().gap(px(1.5));
        for idx in indices {
            let is_filled = idx != empty_index;
            col = col.child(
                div()
                    .w(px(2.5))
                    .h(px(2.5))
                    .rounded_full()
                    .when(is_filled, |el| el.bg(rgba(theme.accent)))
                    .when(!is_filled, |el| {
                        el.bg(rgba(Theme::with_alpha(theme.accent, 0x25)))
                    }),
            );
        }
        col
    };

    div()
        .w(px(14.))
        .h(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(2.5))
                .items_center()
                .justify_center()
                .child(render_col(left_indices))
                .child(render_col(right_indices)),
        )
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AttentionStyle {
    pub(crate) bg_color: Option<u32>,
    pub(crate) text_color: Option<u32>,
    pub(crate) border_color: Option<u32>,
    pub(crate) is_alerting: bool,
}

pub(crate) fn compute_attention_style(
    status: muxlane_core::model::AgentStatus,
    seen: bool,
    is_error: bool,
    pulse_phase: usize,
    theme: Theme,
) -> AttentionStyle {
    // 36 步采样的平滑余弦缓动（10 FPS，3.6 秒一周期）。
    let pulse = (1.0 - (pulse_phase as f32 * std::f32::consts::TAU / 36.0).cos()) * 0.5;
    match status {
        muxlane_core::model::AgentStatus::Blocked => {
            let base_color = theme.yellow;
            let alpha = (0x0e as f32 + pulse * 0x28 as f32) as u32;
            let border_alpha = (0x40 as f32 + pulse * 0x80 as f32) as u32;
            AttentionStyle {
                bg_color: Some(Theme::with_alpha(base_color, alpha as u8)),
                text_color: Some(base_color),
                border_color: Some(Theme::with_alpha(base_color, border_alpha as u8)),
                is_alerting: true,
            }
        }
        muxlane_core::model::AgentStatus::Done if !seen => {
            let base_color = if is_error { theme.red } else { theme.green };
            let alpha = (0x0c as f32 + pulse * 0x24 as f32) as u32;
            let border_alpha = (0x35 as f32 + pulse * 0x75 as f32) as u32;
            AttentionStyle {
                bg_color: Some(Theme::with_alpha(base_color, alpha as u8)),
                text_color: Some(base_color),
                border_color: Some(Theme::with_alpha(base_color, border_alpha as u8)),
                is_alerting: true,
            }
        }
        _ => AttentionStyle::default(),
    }
}

pub(crate) fn render_status_indicator(
    status: muxlane_core::model::AgentStatus,
    is_error: bool,
    spinner_frame: usize,
    theme: Theme,
) -> gpui::Div {
    let container = div()
        .w(px(14.))
        .h(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();

    match status {
        muxlane_core::model::AgentStatus::Working => {
            render_pi_loading_spinner(spinner_frame, theme)
        }
        muxlane_core::model::AgentStatus::Blocked => container.child(
            div()
                .w(px(6.))
                .h(px(6.))
                .rounded_full()
                .bg(rgba(theme.yellow)),
        ),
        muxlane_core::model::AgentStatus::Done if is_error => {
            container.child(div().w(px(6.)).h(px(6.)).rounded_full().bg(rgba(theme.red)))
        }
        muxlane_core::model::AgentStatus::Done => container.child(
            div()
                .w(px(6.))
                .h(px(6.))
                .rounded_full()
                .bg(rgba(theme.green)),
        ),
        muxlane_core::model::AgentStatus::Idle | muxlane_core::model::AgentStatus::Unknown => {
            container.child(div().w(px(5.)).h(px(5.)).rounded_full().bg(rgba(theme.fg2)))
        }
    }
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        let t: String = s.chars().take(n).collect();
        format!("{t}…")
    } else {
        s.to_string()
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 上传阶段进度文本：如「上传二进制 12.3 MB / 45.6 MB (27%)」；
/// 无字节数时退化为「上传二进制 27%」/「上传二进制…」
pub(crate) fn format_upload_phase(
    progress: &muxlane_client::BootstrapProgress,
    language: Language,
) -> String {
    let label = i18n::text(
        language,
        match progress.phase {
            muxlane_client::BootstrapPhase::Upload => "bootstrap.phase.upload",
            muxlane_client::BootstrapPhase::Install => "bootstrap.phase.install",
            muxlane_client::BootstrapPhase::Restart => "bootstrap.phase.restart",
        },
    );
    match (progress.done_bytes, progress.total_bytes, progress.percent) {
        (Some(done), Some(total), Some(percent)) if total > 0 => {
            format!(
                "{label} {} / {} ({percent}%)",
                format_bytes(done),
                format_bytes(total)
            )
        }
        (_, _, Some(percent)) => format!("{label} {percent}%"),
        _ => format!("{label}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_progress_text_shows_bytes_and_percent() {
        use muxlane_client::BootstrapPhase;
        let progress = muxlane_client::BootstrapProgress {
            phase: BootstrapPhase::Upload,
            percent: Some(27),
            done_bytes: Some(12 * 1024 * 1024 + 300 * 1024),
            total_bytes: Some(45 * 1024 * 1024),
        };
        let text = format_upload_phase(&progress, Language::English);
        assert!(text.contains("12.3 MB"), "{text}");
        assert!(text.contains("45.0 MB"), "{text}");
        assert!(text.contains("27%"), "{text}");
        // 无字节数时退化为纯百分比
        let text = format_upload_phase(
            &muxlane_client::BootstrapProgress {
                phase: BootstrapPhase::Install,
                percent: Some(50),
                done_bytes: None,
                total_bytes: None,
            },
            Language::English,
        );
        assert_eq!(text, "Installing 50%");
        // 无细分进度
        let text = format_upload_phase(
            &muxlane_client::BootstrapProgress {
                phase: BootstrapPhase::Restart,
                percent: None,
                done_bytes: None,
                total_bytes: None,
            },
            Language::English,
        );
        assert_eq!(text, "Restarting Service…");
    }
}
