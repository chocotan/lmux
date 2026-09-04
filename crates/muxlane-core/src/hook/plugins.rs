use super::write_file_if_changed;
use crate::Result;
use std::path::Path;

/// 上报脚本本体（node，零依赖：用 stdin 之外直接命令行参数 + socket 写 JSON）
pub const REPORT_SCRIPT: &str = concat!(
    "#!/usr/bin/env node\n",
    include_str!("../../hooks/shared-prelude.js"),
    include_str!("../../hooks/report.js")
);

/// OpenCode plugin：`session.idle` 权威上报 done。
pub const OPENCODE_PLUGIN: &str = concat!(
    include_str!("../../hooks/shared-prelude.js"),
    include_str!("../../hooks/opencode-plugin.js")
);

/// Pi extension：基于 pi-wechat-notifier 模式支持待确认(ask_user)、Subagent 状态区分及异常捕获。
pub const PI_EXTENSION: &str = concat!(
    include_str!("../../hooks/shared-prelude.js"),
    include_str!("../../hooks/pi-extension.js")
);

/// 安装 OpenCode/Pi 插件（幂等，不覆盖其他插件）。
/// 同时清掉旧 lmux 时代的插件文件（改名的遗留），避免与新版并存重复上报。
pub fn install_agent_plugins(home: &Path) -> Result<()> {
    let opencode = home.join(".config/opencode/plugins/muxlane.ts");
    write_file_if_changed(&opencode, OPENCODE_PLUGIN)?;
    let pi = home.join(".pi/agent/extensions/muxlane.ts");
    write_file_if_changed(&pi, PI_EXTENSION)?;
    for legacy in [
        ".config/opencode/plugins/lmux.ts",
        ".pi/agent/extensions/lmux.ts",
    ] {
        std::fs::remove_file(home.join(legacy)).ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_include_shared_prelude_once() {
        for script in [REPORT_SCRIPT, OPENCODE_PLUGIN, PI_EXTENSION] {
            assert_eq!(script.matches("function muxlaneEnv").count(), 1);
            assert_eq!(script.matches("function report").count(), 1);
        }
        assert!(REPORT_SCRIPT.starts_with("#!/usr/bin/env node\n"));
    }

    #[test]
    fn installs_opencode_and_pi_plugins_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        install_agent_plugins(dir.path()).unwrap();
        install_agent_plugins(dir.path()).unwrap();
        let opencode =
            std::fs::read_to_string(dir.path().join(".config/opencode/plugins/muxlane.ts"))
                .unwrap();
        let pi =
            std::fs::read_to_string(dir.path().join(".pi/agent/extensions/muxlane.ts")).unwrap();
        assert!(opencode.contains("client.session.messages"));
        assert!(pi.contains("agent_settled"));
        assert!(pi.contains("assistantText"));
        assert!(pi.contains("isSubagentProcess"));
        assert!(pi.contains("--no-session"));
    }

    #[test]
    fn legacy_lmux_plugins_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let old_opencode = dir.path().join(".config/opencode/plugins/lmux.ts");
        let old_pi = dir.path().join(".pi/agent/extensions/lmux.ts");
        std::fs::create_dir_all(old_opencode.parent().unwrap()).unwrap();
        std::fs::create_dir_all(old_pi.parent().unwrap()).unwrap();
        std::fs::write(&old_opencode, "old").unwrap();
        std::fs::write(&old_pi, "old").unwrap();
        install_agent_plugins(dir.path()).unwrap();
        assert!(!old_opencode.exists());
        assert!(!old_pi.exists());
    }
}
