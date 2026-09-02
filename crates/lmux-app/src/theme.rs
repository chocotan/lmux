//! lmux 主题系统：支持纯净浅色 (Light) 与 极简深色 (Dark)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
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
    pub green: u32,
    pub yellow: u32,
    pub red: u32,
    pub cyan: u32,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            bg0: 0xfaf9f6ff,
            bg1: 0xece8e1ff,
            bg2: 0xdbd7cfff,
            bg3: 0xd0ccc3ff,
            line: 0xd0ccc3ff,
            fg0: 0x242831ff,
            fg1: 0x727783ff,
            fg2: 0x8b90a0ff,
            accent: 0x3d6cd8ff,
            green: 0x529633ff,
            yellow: 0xb88226ff,
            red: 0xd13e50ff,
            cyan: 0x2a92b0ff,
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            bg0: 0x181a1fff,
            bg1: 0x21252bff,
            bg2: 0x282c34ff,
            bg3: 0x3b4048ff,
            line: 0x333842ff,
            fg0: 0xd7dae0ff,
            fg1: 0x828997ff,
            fg2: 0x5c6370ff,
            accent: 0x528bff,
            green: 0x98c379ff,
            yellow: 0xe5c07bff,
            red: 0xe06c75ff,
            cyan: 0x56b6c2ff,
        }
    }

    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }
}
