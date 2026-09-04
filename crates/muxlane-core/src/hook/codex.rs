use super::write_file_if_changed;
use crate::Result;
use std::path::Path;

/// codex config.toml notify 链：保存旧值到 <config>.muxlane-notify-prev.json
pub fn inject_codex_notify(config_path: &Path, scripts_dir: &Path, _socket: &Path) -> Result<()> {
    let prev_backup = config_path.with_extension("muxlane-notify-prev.json");
    let content = if config_path.exists() {
        std::fs::read_to_string(config_path)?
    } else {
        String::new()
    };
    let mut doc: toml::Value = if content.is_empty() {
        toml::Value::Table(Default::default())
    } else {
        toml::from_str(&content)?
    };
    let notify = toml::Value::Array(vec![
        toml::Value::String("node".into()),
        toml::Value::String(scripts_dir.join("report.mjs").display().to_string()),
        toml::Value::String("done".into()),
    ]);
    // 首次注入时备份旧 notify
    if !prev_backup.exists() {
        if let Some(old) = doc.get("notify") {
            std::fs::write(&prev_backup, serde_json::to_string(old)?)?;
        }
    }
    if let Some(table) = doc.as_table_mut() {
        table.insert("notify".into(), notify);
    }
    write_file_if_changed(config_path, &toml::to_string_pretty(&doc)?)?;
    Ok(())
}

/// 卸载：还原 codex notify（读取备份链）
pub fn uninstall_codex_notify(config_path: &Path) -> Result<()> {
    let prev_backup = config_path.with_extension("muxlane-notify-prev.json");
    if prev_backup.exists() {
        let old: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prev_backup)?)?;
        let toml_old: toml::Value = serde_json::from_value(old)?;
        let mut doc: toml::Value = toml::from_str(&std::fs::read_to_string(config_path)?)?;
        if let Some(table) = doc.as_table_mut() {
            table.insert("notify".into(), toml_old);
        }
        std::fs::write(config_path, toml::to_string_pretty(&doc)?)?;
        std::fs::remove_file(&prev_backup).ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_chain_restores_previous_value() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "model = \"gpt\"\nnotify = [\"my-old-notify\"]\n").unwrap();
        inject_codex_notify(
            &config,
            Path::new("/tmp/hooks"),
            Path::new("/tmp/muxlane.sock"),
        )
        .unwrap();
        let parsed: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(
            parsed["notify"].as_array().unwrap(),
            &vec![
                toml::Value::String("node".into()),
                toml::Value::String("/tmp/hooks/report.mjs".into()),
                toml::Value::String("done".into()),
            ]
        );
        assert!(config.with_extension("muxlane-notify-prev.json").exists());
        uninstall_codex_notify(&config).unwrap();
        assert!(std::fs::read_to_string(&config)
            .unwrap()
            .contains("my-old-notify"));
    }
}
