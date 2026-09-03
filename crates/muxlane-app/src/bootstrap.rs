use std::path::{Path, PathBuf};

pub fn install(data_dir: &Path) {
    if std::env::var("MUXLANE_HOOKS").as_deref() == Ok("off") {
        return;
    }

    let scripts_dir = muxlane_core::hook::hook_scripts_dir(data_dir);
    std::fs::create_dir_all(&scripts_dir).ok();
    std::fs::write(
        scripts_dir.join("report.mjs"),
        muxlane_core::hook::REPORT_SCRIPT,
    )
    .ok();
    let socket_path = data_dir.join("muxlane.sock");
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());

    let claude_dir = PathBuf::from(&home).join(".claude");
    let _ = std::fs::create_dir_all(&claude_dir);
    let hooks = muxlane_core::hook::claude_hooks_value(&scripts_dir, "claude-hook", &socket_path);
    if let Err(error) =
        muxlane_core::hook::inject_claude_hooks(&claude_dir.join("settings.json"), hooks)
    {
        tracing::warn!(%error, "claude hooks 注入失败");
    }

    let codex_dir = PathBuf::from(&home).join(".codex");
    let _ = std::fs::create_dir_all(&codex_dir);
    if let Err(error) = muxlane_core::hook::inject_codex_notify(
        &codex_dir.join("config.toml"),
        &scripts_dir,
        &socket_path,
    ) {
        tracing::warn!(%error, "codex notify 注入失败");
    }
    if let Err(error) = muxlane_core::hook::install_agent_plugins(Path::new(&home)) {
        tracing::warn!(%error, "OpenCode/Pi plugins 安装失败");
    }
    tracing::info!("hook/plugin 注入完成（Claude/Codex/OpenCode/Pi）");
}
