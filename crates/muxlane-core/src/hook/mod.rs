//! HookInjector：启动 agent 时注入 hook 配置（幂等 + 旧值备份，pocket-studio 模式）
use crate::{Error, Result};
use std::path::{Path, PathBuf};

mod claude;
mod codex;
mod plugins;

pub use claude::{claude_hooks_value, inject_claude_hooks, uninstall_claude_hooks};
pub use codex::{inject_codex_notify, uninstall_codex_notify};
pub use plugins::{install_agent_plugins, OPENCODE_PLUGIN, PI_EXTENSION, REPORT_SCRIPT};

/// hook 上报脚本会被安装到该目录（由 app 启动时落盘）
pub fn hook_scripts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("hooks")
}

/// 幂等写文件：内容不同才写，旧内容备份到 <path>.muxlane-bak
pub fn write_file_if_changed(path: &Path, new_content: &str) -> Result<bool> {
    if path.exists() {
        let old = std::fs::read_to_string(path)?;
        if old == new_content {
            return Ok(false);
        }
        let backup = path.with_extension("muxlane-bak");
        if !backup.exists() {
            std::fs::write(backup, old).ok();
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::IoContext {
            context: "create parent dir",
            source,
        })?;
    }
    std::fs::write(path, new_content).map_err(|source| Error::IoContext {
        context: "write file",
        source,
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_if_changed_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "old").unwrap();
        assert!(write_file_if_changed(&path, "new").unwrap());
        assert_eq!(
            std::fs::read_to_string(path.with_extension("muxlane-bak")).unwrap(),
            "old"
        );
        assert!(!write_file_if_changed(&path, "new").unwrap());
    }
}
