use super::write_file_if_changed;
use crate::Result;
use std::path::Path;

/// 生成 claude settings.json 的 hooks 片段（Stop + Notification）
/// hook 命令约定：node <scripts_dir>/report.mjs <event>
pub fn claude_hooks_value(
    scripts_dir: &Path,
    _agent_id: &str,
    _socket: &Path,
) -> serde_json::Value {
    let mk = |event: &str| {
        serde_json::json!({
            "matcher": "",
            "hooks": [{
                "type": "command",
                // 身份/鉴权完全来自每个 PTY 注入的 env；全局 settings 不绑定具体 pane。
                "command": format!(
                    "node {} {}",
                    scripts_dir.join("report.mjs").display(),
                    event,
                ),
            }]
        })
    };
    serde_json::json!({
        "UserPromptSubmit": [mk("working")],
        "PreToolUse": [mk("working")],
        "Stop": [mk("done")],
        "Notification": [mk("blocked")]
    })
}

fn remove_where_command(value: &mut serde_json::Value, pred: &dyn Fn(&str) -> bool) -> bool {
    if value
        .get("command")
        .and_then(|item| item.as_str())
        .is_some_and(pred)
    {
        return false;
    }
    if let Some(hooks) = value
        .get_mut("hooks")
        .and_then(|hooks| hooks.as_array_mut())
    {
        hooks.retain_mut(|value| remove_where_command(value, pred));
        return !hooks.is_empty();
    }
    true
}

fn remove_report_commands(value: &mut serde_json::Value) -> bool {
    remove_where_command(value, &|command| command.contains("report.mjs"))
}

/// 向 JSON 文件的指定键合并 hooks 配置（claude settings.json）
pub fn inject_claude_hooks(settings_path: &Path, hooks_value: serde_json::Value) -> Result<()> {
    let existing: serde_json::Value = if settings_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(settings_path)?)?
    } else {
        serde_json::json!({})
    };
    let mut merged = existing.clone();
    // 只合并 "hooks" 键；用户的其它配置原样保留
    if let (Some(obj), Some(new_hooks)) = (merged.as_object_mut(), hooks_value.as_object()) {
        // 先清掉旧 lmux 时代的条目（改名的遗留路径），否则下面会误判重复、新 hook 写不进去
        if let Some(hook_obj) = obj.get_mut("hooks").and_then(|value| value.as_object_mut()) {
            for value in hook_obj.values_mut() {
                if let Some(items) = value.as_array_mut() {
                    items.retain_mut(|value| {
                        remove_where_command(value, &|command| command.contains("/lmux/hooks/"))
                    });
                }
            }
        }
        let hooks_entry = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
        if let Some(hook_obj) = hooks_entry.as_object_mut() {
            for (event, value) in new_hooks {
                let dest = hook_obj
                    .entry(event.clone())
                    .or_insert_with(|| serde_json::json!([]));
                let Some(dest_array) = dest.as_array_mut() else {
                    continue;
                };
                let Some(new_array) = value.as_array() else {
                    continue;
                };
                for item in new_array {
                    if !dest_array.iter().any(|existing| existing == item) {
                        dest_array.push(item.clone());
                    }
                }
            }
        }
    }
    if merged != existing {
        write_file_if_changed(settings_path, &serde_json::to_string_pretty(&merged)?)?;
    }
    Ok(())
}

/// 卸载 Claude hook：只删除 muxlane 的 report.mjs 条目，保留用户 hook。
pub fn uninstall_claude_hooks(settings_path: &Path) -> Result<()> {
    if !settings_path.exists() {
        return Ok(());
    }
    let mut root: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings_path)?)?;
    if let Some(hooks) = root
        .get_mut("hooks")
        .and_then(|value| value.as_object_mut())
    {
        for value in hooks.values_mut() {
            if let Some(items) = value.as_array_mut() {
                items.retain_mut(remove_report_commands);
            }
        }
    }
    std::fs::write(settings_path, serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn merges_without_destroying_user_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{"theme": "dark", "hooks": {"PreToolUse": [{"type":"command","command":"my-own"}]}}"#,
        )
        .unwrap();
        let hooks = claude_hooks_value(
            Path::new("/tmp/hooks"),
            "agent_x",
            Path::new("/tmp/muxlane.sock"),
        );
        inject_claude_hooks(&settings, hooks).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(after["theme"], "dark");
        assert!(after["hooks"]["PreToolUse"].is_array());
        assert!(after["hooks"]["Stop"].is_array());
        assert_eq!(after["hooks"]["Stop"][0]["matcher"], "");
        assert!(after["hooks"]["Stop"][0]["hooks"].is_array());
    }

    #[test]
    fn same_event_hooks_coexist_and_uninstall_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{"hooks":{"Stop":[{"type":"command","command":"my-stop"}],"Notification":[{"type":"command","command":"my-note"}]}}"#,
        )
        .unwrap();
        let hooks = claude_hooks_value(dir.path(), "", Path::new("/tmp/muxlane.sock"));
        inject_claude_hooks(&settings, hooks.clone()).unwrap();
        inject_claude_hooks(&settings, hooks).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(value["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(value["hooks"]["Notification"].as_array().unwrap().len(), 2);
        uninstall_claude_hooks(&settings).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(value["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(value["hooks"]["Stop"][0]["command"], "my-stop");
    }

    #[test]
    fn legacy_lmux_entries_are_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"node /home/choco/.local/share/lmux/hooks/report.mjs done"}]}]}}"#,
        )
        .unwrap();
        let hooks = claude_hooks_value(dir.path(), "", Path::new("/tmp/muxlane.sock"));
        inject_claude_hooks(&settings, hooks).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let stop = value["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert!(stop[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("node "));
        assert!(!serde_json::to_string(&value)
            .unwrap()
            .contains("lmux/hooks"));
    }

    #[test]
    fn malformed_settings_returns_json_error() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, "{").unwrap();
        let error = inject_claude_hooks(&settings, serde_json::json!({})).unwrap_err();
        assert!(matches!(error, Error::Json(_)));
    }
}
