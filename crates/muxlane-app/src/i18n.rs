use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Chinese,
    English,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::Chinese, Self::English];

    pub fn id(self) -> &'static str {
        match self {
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Chinese => "中文",
            Self::English => "English",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "zh" | "zh_cn" | "zh-CN" => Some(Self::Chinese),
            "en" | "en_us" | "en-US" => Some(Self::English),
            _ => None,
        }
    }

    pub fn detect() -> Self {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .find_map(|value| {
                let value = value.trim().to_ascii_lowercase();
                if value.starts_with("zh") {
                    Some(Self::Chinese)
                } else if value.starts_with("en") {
                    Some(Self::English)
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }
}

pub fn text<'a>(language: Language, chinese: &'a str, english: &'a str) -> &'a str {
    match language {
        Language::Chinese => chinese,
        Language::English => english,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_ids_and_text_are_stable() {
        assert_eq!(Language::from_id("zh"), Some(Language::Chinese));
        assert_eq!(Language::from_id("en-US"), Some(Language::English));
        assert_eq!(text(Language::English, "设置", "Settings"), "Settings");
    }
}
