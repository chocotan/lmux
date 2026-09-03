//! manifest：TOML 屏幕规则 → 编译后的匹配器（借鉴 herdr 的规则形态，精简）
use super::ScreenInput;
use crate::model::AgentStatus;
use crate::{Error, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ManifestFile {
    /// 规则列表，自上而下第一个命中的生效
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub status: AgentStatus,
    /// 匹配区域：bottom（底部可见行）| osc_title
    pub region: String,
    /// 全部任一包含即命中（子串）
    #[serde(default)]
    pub contains: Vec<String>,
    /// 正则（任一命中）
    #[serde(default)]
    pub regex: Vec<String>,
    /// 反向条件：全部不包含才算命中
    #[serde(default)]
    pub not_contains: Vec<String>,
}

#[derive(Debug)]
pub struct CompiledManifest {
    pub agent_type: String,
    rules: Vec<CompiledRule>,
}

#[derive(Debug)]
struct CompiledRule {
    status: AgentStatus,
    region: Region,
    contains: Vec<String>,
    regexes: Vec<regex::Regex>,
    not_contains: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum Region {
    Bottom,
    OscTitle,
}

impl CompiledManifest {
    pub fn parse(agent_type: &str, src: &str) -> Result<Self> {
        let file: ManifestFile = toml::from_str(src)?;
        let mut rules = Vec::new();
        for r in file.rules {
            let region = match r.region.as_str() {
                "bottom" => Region::Bottom,
                "osc_title" => Region::OscTitle,
                other => return Err(Error::UnknownRegion(other.into())),
            };
            let mut regexes = Vec::new();
            for pat in &r.regex {
                regexes.push(regex::Regex::new(pat)?);
            }
            rules.push(CompiledRule {
                status: r.status,
                region,
                contains: r.contains,
                regexes,
                not_contains: r.not_contains,
            });
        }
        Ok(CompiledManifest {
            agent_type: agent_type.into(),
            rules,
        })
    }

    pub fn evaluate(&self, input: &ScreenInput) -> Option<AgentStatus> {
        for rule in &self.rules {
            let hit = match rule.region {
                Region::Bottom => {
                    let joined = input.bottom_lines.join("\n");
                    text_hit(rule, &joined, &input.bottom_lines)
                }
                Region::OscTitle => {
                    let title = input.osc_title.as_deref().unwrap_or("");
                    if title.is_empty() {
                        false
                    } else {
                        text_hit(rule, title, &[title.to_string()])
                    }
                }
            };
            if hit {
                return Some(rule.status);
            }
        }
        None
    }
}

fn text_hit(rule: &CompiledRule, joined: &str, lines: &[String]) -> bool {
    if let Some(bad) = rule
        .not_contains
        .iter()
        .find(|n| joined.contains(n.as_str()))
    {
        let _ = bad;
        return false;
    }
    let contains_hit = rule.contains.iter().any(|c| joined.contains(c.as_str()));
    let regex_hit = rule
        .regexes
        .iter()
        .any(|re| lines.iter().any(|l| re.is_match(l)));
    contains_hit || regex_hit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_evaluate() {
        let m = CompiledManifest::parse(
            "claude",
            r#"
[[rules]]
status = "blocked"
region = "bottom"
contains = ["Do you want to proceed?"]
not_contains = ["(y/n) fake"]

[[rules]]
status = "working"
region = "osc_title"
regex = ["✔ .*"]
"#,
        )
        .unwrap();
        let blocked = m.evaluate(&ScreenInput {
            bottom_lines: vec!["Do you want to proceed? (1/3)".into()],
            osc_title: None,
            secs_since_output: None,
            bell: false,
        });
        assert_eq!(blocked, Some(AgentStatus::Blocked));

        let title_working = m.evaluate(&ScreenInput {
            bottom_lines: vec![],
            osc_title: Some("✔ editing".into()),
            secs_since_output: None,
            bell: false,
        });
        assert_eq!(title_working, Some(AgentStatus::Working));

        let none = m.evaluate(&ScreenInput {
            bottom_lines: vec!["$ ls".into()],
            osc_title: None,
            secs_since_output: None,
            bell: false,
        });
        assert_eq!(none, None);
    }

    #[test]
    fn unknown_region_is_typed() {
        let error = CompiledManifest::parse(
            "claude",
            r#"
[[rules]]
status = "working"
region = "middle"
"#,
        )
        .unwrap_err();

        assert!(matches!(error, Error::UnknownRegion(region) if region == "middle"));
    }

    #[test]
    fn builtin_manifests_parse() {
        let ms = crate::detect::builtin_manifests();
        assert!(ms.len() >= 4);
        assert!(ms.iter().any(|m| m.agent_type == "claude"));
    }
}
