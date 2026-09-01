//! Agent Presets：参考 muxel 的 Agent preset 模型，保持最小而可扩展。
use crate::model::AgentType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPreset {
    pub id: String,
    pub label: String,
    pub agent_type: AgentType,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 二进制不存在时 UI 隐藏（Shell 永远可用）
    #[serde(default = "default_true")]
    pub require_installed: bool,
}
fn default_true() -> bool {
    true
}

impl AgentPreset {
    pub fn installed(&self) -> bool {
        if self.agent_type == AgentType::Shell {
            return true;
        }
        which(&self.program).is_some()
    }
}

pub fn builtin_presets(shell: impl Into<String>) -> Vec<AgentPreset> {
    let empty = || BTreeMap::new();
    vec![
        AgentPreset {
            id: "shell".into(),
            label: "Shell".into(),
            agent_type: AgentType::Shell,
            program: shell.into(),
            args: vec![],
            env: empty(),
            require_installed: false,
        },
        AgentPreset {
            id: "claude".into(),
            label: "Claude Code".into(),
            agent_type: AgentType::Claude,
            program: "claude".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
        AgentPreset {
            id: "codex".into(),
            label: "Codex".into(),
            agent_type: AgentType::Codex,
            program: "codex".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
        AgentPreset {
            id: "opencode".into(),
            label: "OpenCode".into(),
            agent_type: AgentType::Opencode,
            program: "opencode".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
        AgentPreset {
            id: "pi".into(),
            label: "Pi".into(),
            agent_type: AgentType::Pi,
            program: "pi".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
        AgentPreset {
            id: "gemini".into(),
            label: "Gemini".into(),
            agent_type: AgentType::Gemini,
            program: "gemini".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
    ]
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(program);
    if p.components().count() > 1 && p.is_file() {
        return Some(p.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_include_core_agents() {
        let ps = builtin_presets("/bin/zsh");
        assert_eq!(ps[0].program, "/bin/zsh");
        for id in ["shell", "claude", "codex", "opencode", "pi", "gemini"] {
            assert!(ps.iter().any(|p| p.id == id));
        }
    }
    #[test]
    fn shell_always_visible() {
        assert!(builtin_presets("/not/actually/installed")[0].installed());
    }
}
