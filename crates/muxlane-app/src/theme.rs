//! muxlane 主题系统：参考 pocket-studio 的浅色、暖色与深色主题。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Paper,
    Sky,
    Jade,
    Sakura,
    Dark,
    Synthwave,
    OneDark,
}

impl ThemeMode {
    pub const ALL: [Self; 8] = [
        Self::Light,
        Self::Paper,
        Self::Sky,
        Self::Jade,
        Self::Sakura,
        Self::Dark,
        Self::Synthwave,
        Self::OneDark,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Paper => "paper",
            Self::Sky => "sky",
            Self::Jade => "jade",
            Self::Sakura => "sakura",
            Self::Dark => "dark",
            Self::Synthwave => "synthwave",
            Self::OneDark => "onedark",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "雾白瓷",
            Self::Paper => "纸暖",
            Self::Sky => "霁蓝",
            Self::Jade => "竹青",
            Self::Sakura => "樱粉",
            Self::Dark => "墨渊",
            Self::Synthwave => "夜霓",
            Self::OneDark => "代码墨",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Light => "Porcelain",
            Self::Paper => "Paper Warm",
            Self::Sky => "Clear Sky",
            Self::Jade => "Jade",
            Self::Sakura => "Sakura",
            Self::Dark => "Ink",
            Self::Synthwave => "Synthwave",
            Self::OneDark => "One Dark",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark | Self::Synthwave | Self::OneDark)
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Theme {
    pub mode: ThemeMode,
    pub bg0: u32,
    pub bg1: u32,
    pub bg2: u32,
    pub bg3: u32,
    pub line: u32,
    pub fg0: u32,
    pub fg1: u32,
    pub fg2: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub green: u32,
    pub yellow: u32,
    pub red: u32,
    pub cyan: u32,
}

impl Theme {
    pub fn with_alpha(color: u32, alpha: u8) -> u32 {
        (color & 0xffffff00) | u32::from(alpha)
    }

    pub fn overlay(self) -> u32 {
        Self::with_alpha(0x00000000, 0x44)
    }

    pub fn selection(self) -> u32 {
        Self::with_alpha(self.accent, 0x55)
    }

    pub fn cursor(self) -> u32 {
        Self::with_alpha(self.accent, 0xc0)
    }

    pub fn scrollbar_track(self) -> u32 {
        Self::with_alpha(self.bg3, 0x4d)
    }

    pub fn scrollbar_thumb(self) -> u32 {
        Self::with_alpha(self.fg1, 0xaa)
    }

    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self {
                mode,
                bg0: 0xfaf9f6ff,
                bg1: 0xece8e1ff,
                bg2: 0xdbd7cfff,
                bg3: 0xd0ccc3ff,
                line: 0xd0ccc3ff,
                fg0: 0x242831ff,
                fg1: 0x5c6370ff,
                fg2: 0x8a909cff,
                accent: 0x3d6cd8ff,
                on_accent: 0xffffffff,
                green: 0x529633ff,
                yellow: 0xb88226ff,
                red: 0xd13e50ff,
                cyan: 0x2a92b0ff,
            },
            ThemeMode::Paper => Self {
                mode,
                bg0: 0xf4ecdfff,
                bg1: 0xeadfceff,
                bg2: 0xdfd0bcff,
                bg3: 0xd2c0a8ff,
                line: 0xd2c0a8ff,
                fg0: 0x3d3027ff,
                fg1: 0x786858ff,
                fg2: 0x9d8a77ff,
                accent: 0xb35f2aff,
                on_accent: 0xffffffff,
                green: 0x4e8c62ff,
                yellow: 0xb8872eff,
                red: 0xc94b4bff,
                cyan: 0x368ca1ff,
            },
            ThemeMode::Sky => Self {
                mode,
                bg0: 0xf2f7ffff,
                bg1: 0xe3edfcff,
                bg2: 0xd2e0f4ff,
                bg3: 0xc1d3ecff,
                line: 0xc1d3ecff,
                fg0: 0x1e293bff,
                fg1: 0x64748bff,
                fg2: 0x94a3b8ff,
                accent: 0x2563ebff,
                on_accent: 0xffffffff,
                green: 0x16805cff,
                yellow: 0xb7791fff,
                red: 0xdc3f51ff,
                cyan: 0x1689a8ff,
            },
            ThemeMode::Jade => Self {
                mode,
                bg0: 0xeff8f2ff,
                bg1: 0xe0f0e6ff,
                bg2: 0xcfe4d8ff,
                bg3: 0xbfd8caff,
                line: 0xbfd8caff,
                fg0: 0x1f3529ff,
                fg1: 0x5f7869ff,
                fg2: 0x8aa394ff,
                accent: 0x059669ff,
                on_accent: 0xffffffff,
                green: 0x2f855aff,
                yellow: 0xb7791fff,
                red: 0xc94b4bff,
                cyan: 0x1689a8ff,
            },
            ThemeMode::Sakura => Self {
                mode,
                bg0: 0xfff2f5ff,
                bg1: 0xf9e3eaff,
                bg2: 0xf1d0dcff,
                bg3: 0xe8bdcdff,
                line: 0xe8bdcdff,
                fg0: 0x452530ff,
                fg1: 0x855568ff,
                fg2: 0xb18798ff,
                accent: 0xe11d48ff,
                on_accent: 0xffffffff,
                green: 0x378557ff,
                yellow: 0xb7791fff,
                red: 0xc2415aff,
                cyan: 0x1689a8ff,
            },
            ThemeMode::Dark => Self {
                mode,
                bg0: 0x181a1fff,
                bg1: 0x21252bff,
                bg2: 0x282c34ff,
                bg3: 0x3b4048ff,
                line: 0x333842ff,
                fg0: 0xd7dae0ff,
                fg1: 0x828997ff,
                fg2: 0x5c6370ff,
                accent: 0x528bffff,
                on_accent: 0x0f1419ff,
                green: 0x98c379ff,
                yellow: 0xe5c07bff,
                red: 0xe06c75ff,
                cyan: 0x56b6c2ff,
            },
            ThemeMode::Synthwave => Self {
                mode,
                bg0: 0x1b1029ff,
                bg1: 0x28163bff,
                bg2: 0x38204dff,
                bg3: 0x4b2861ff,
                line: 0x4b2861ff,
                fg0: 0xf9eaffff,
                fg1: 0xc6a9d8ff,
                fg2: 0x9875adff,
                accent: 0xe879f9ff,
                on_accent: 0x1b1029ff,
                green: 0x7ee2b8ff,
                yellow: 0xf5ca7aff,
                red: 0xfb7185ff,
                cyan: 0x67e8f9ff,
            },
            ThemeMode::OneDark => Self {
                mode,
                bg0: 0x282c34ff,
                bg1: 0x21252bff,
                bg2: 0x313640ff,
                bg3: 0x3e4451ff,
                line: 0x3e4451ff,
                fg0: 0xabb2bfff,
                fg1: 0x7f848eff,
                fg2: 0x5c6370ff,
                accent: 0x61afefff,
                on_accent: 0x0f1419ff,
                green: 0x98c379ff,
                yellow: 0xe5c07bff,
                red: 0xe06c75ff,
                cyan: 0x56b6c2ff,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_ids_round_trip() {
        for mode in ThemeMode::ALL {
            assert_eq!(ThemeMode::from_id(mode.id()), Some(mode));
            assert_ne!(Theme::for_mode(mode).bg0, 0);
        }
    }

    #[test]
    fn dark_theme_ids_are_marked_dark() {
        assert!(ThemeMode::Dark.is_dark());
        assert!(ThemeMode::Synthwave.is_dark());
        assert!(!ThemeMode::Paper.is_dark());
    }

    fn luminance(color: u32) -> u32 {
        let r = (color >> 24) & 0xff;
        let g = (color >> 16) & 0xff;
        let b = (color >> 8) & 0xff;
        r + g + b
    }

    #[test]
    fn light_secondary_text_is_lighter_than_primary_muted() {
        let theme = Theme::for_mode(ThemeMode::Light);
        assert!(luminance(theme.fg2) > luminance(theme.fg1));
        assert!(luminance(theme.fg1) > luminance(theme.fg0));
    }

    #[test]
    fn synthwave_on_accent_is_not_white() {
        let theme = Theme::for_mode(ThemeMode::Synthwave);
        assert_ne!(theme.on_accent, 0xffffffff);
        assert_eq!(Theme::with_alpha(theme.accent, 0x44) & 0xff, 0x44);
    }
}
