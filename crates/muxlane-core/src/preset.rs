//! Agent preset 模型，保持最小而可扩展。
use crate::model::AgentType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// Check the process PATH. Kept for callers without project context.
    pub fn installed(&self) -> bool {
        if self.agent_type == AgentType::Shell {
            return true;
        }
        which(&self.program).is_some()
    }

    /// Resolve the executable visible from a project, including project-local bins.
    pub fn executable_in(&self, cwd: &Path) -> Option<PathBuf> {
        if self.agent_type == AgentType::Shell {
            return Some(PathBuf::from(&self.program));
        }
        project_binary(cwd, &self.program).or_else(|| which(&self.program))
    }

    /// Check project-local binary directories before falling back to PATH.
    pub fn installed_in(&self, cwd: &Path) -> bool {
        self.executable_in(cwd).is_some()
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
            id: "agy".into(),
            label: "Agy".into(),
            agent_type: AgentType::Agy,
            program: "agy".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
        AgentPreset {
            id: "qwen".into(),
            label: "Qwen Code".into(),
            agent_type: AgentType::Qwen,
            program: "qwen".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
        AgentPreset {
            id: "kimi".into(),
            label: "Kimi CLI".into(),
            agent_type: AgentType::Kimi,
            program: "kimi".into(),
            args: vec![],
            env: empty(),
            require_installed: true,
        },
    ]
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn project_binary(cwd: &Path, program: &str) -> Option<PathBuf> {
    let program = Path::new(program);
    if program.components().count() > 1 {
        let candidate = if program.is_absolute() {
            program.to_path_buf()
        } else {
            cwd.join(program)
        };
        return is_executable(&candidate).then_some(candidate);
    }

    ["node_modules/.bin", ".venv/bin", ".local/bin"]
        .into_iter()
        .map(|dir| cwd.join(dir).join(program))
        .find(|candidate| is_executable(candidate))
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(program);
    if p.components().count() > 1 && p.is_file() {
        return is_executable(p).then_some(p.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(program))
            .find(|candidate| is_executable(candidate))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_include_core_agents() {
        let ps = builtin_presets("/bin/zsh");
        assert_eq!(ps[0].program, "/bin/zsh");
        for id in [
            "shell", "claude", "codex", "opencode", "pi", "agy", "qwen", "kimi",
        ] {
            assert!(ps.iter().any(|p| p.id == id));
        }
        assert!(!ps.iter().any(|p| p.id == "gemini"));
        assert_eq!(
            ps.iter()
                .filter(|p| p.agent_type != AgentType::Shell)
                .count(),
            7
        );
    }
    #[test]
    fn shell_always_visible() {
        assert!(builtin_presets("/not/actually/installed")[0].installed());
    }

    #[test]
    fn project_local_bins_are_installed() {
        let temp = tempfile::tempdir().unwrap();
        let preset = builtin_presets("/bin/sh")
            .into_iter()
            .find(|p| p.id == "qwen")
            .unwrap();

        for dir in ["node_modules/.bin", ".venv/bin", ".local/bin"] {
            let project = temp.path().join(dir.replace('/', "_"));
            let bin_dir = project.join(dir);
            std::fs::create_dir_all(&bin_dir).unwrap();
            std::fs::write(bin_dir.join("qwen"), "").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    bin_dir.join("qwen"),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
            assert!(preset.installed_in(&project), "local bin under {dir}");
        }
    }
}
