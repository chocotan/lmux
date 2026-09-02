//! HookInjector：启动 agent 时注入 hook 配置（幂等 + 旧值备份，pocket-studio 模式）
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// hook 上报脚本会被安装到该目录（由 app 启动时落盘）
pub fn hook_scripts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("hooks")
}

/// 生成 claude settings.json 的 hooks 片段（Stop + Notification）
/// hook 命令约定：node <scripts_dir>/report.mjs <socket_path> <token> <agent_id> <event>
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

fn value_has_report_command(value: &serde_json::Value) -> bool {
    value
        .get("command")
        .and_then(|item| item.as_str())
        .is_some_and(|command| command.contains("report.mjs"))
        || value
            .get("hooks")
            .and_then(|hooks| hooks.as_array())
            .is_some_and(|hooks| hooks.iter().any(value_has_report_command))
}

fn remove_report_commands(value: &mut serde_json::Value) -> bool {
    if value_has_report_command(value) && value.get("hooks").is_none() {
        return false;
    }
    if let Some(hooks) = value
        .get_mut("hooks")
        .and_then(|hooks| hooks.as_array_mut())
    {
        hooks.retain_mut(remove_report_commands);
        return !hooks.is_empty();
    }
    true
}

/// 幂等写文件：内容不同才写，旧内容备份到 <path>.lmux-bak
pub fn write_file_if_changed(path: &Path, new_content: &str) -> Result<bool> {
    if path.exists() {
        let old = std::fs::read_to_string(path).unwrap_or_default();
        if old == new_content {
            return Ok(false);
        }
        let backup = path.with_extension("lmux-bak");
        if !backup.exists() {
            std::fs::write(backup, old).ok();
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create parent dir")?;
    }
    std::fs::write(path, new_content).context("write file")?;
    Ok(true)
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
                    let duplicate = dest_array.iter().any(|existing| {
                        existing == item
                            || (value_has_report_command(item)
                                && value_has_report_command(existing))
                    });
                    if !duplicate {
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

/// 卸载 Claude hook：只删除 lmux 的 report.mjs 条目，保留用户 hook。
pub fn uninstall_claude_hooks(settings_path: &Path) -> Result<()> {
    if !settings_path.exists() {
        return Ok(());
    }
    let mut root: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings_path)?)?;
    if let Some(hooks) = root.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for value in hooks.values_mut() {
            if let Some(items) = value.as_array_mut() {
                items.retain_mut(remove_report_commands);
            }
        }
    }
    std::fs::write(settings_path, serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

/// codex config.toml notify 链：保存旧值到 <config>.lmux-notify-prev.json
pub fn inject_codex_notify(config_path: &Path, scripts_dir: &Path, _socket: &Path) -> Result<()> {
    let prev_backup = config_path.with_extension("lmux-notify-prev.json");
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
    if let Some(t) = doc.as_table_mut() {
        t.insert("notify".into(), notify);
    }
    write_file_if_changed(config_path, &toml::to_string_pretty(&doc)?)?;
    Ok(())
}

/// 卸载：还原 codex notify（读取备份链）
pub fn uninstall_codex_notify(config_path: &Path) -> Result<()> {
    let prev_backup = config_path.with_extension("lmux-notify-prev.json");
    if prev_backup.exists() {
        let old: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prev_backup)?)?;
        let toml_old: toml::Value = serde_json::from_value(old)?;
        let doc: toml::Value = toml::from_str(&std::fs::read_to_string(config_path)?)?;
        let mut doc = doc;
        if let Some(t) = doc.as_table_mut() {
            t.insert("notify".into(), toml_old);
        }
        std::fs::write(config_path, toml::to_string_pretty(&doc)?)?;
        std::fs::remove_file(&prev_backup).ok();
    }
    Ok(())
}

/// 上报脚本本体（node，零依赖：用 stdin 之外直接命令行参数 + socket 写 JSON）
pub const REPORT_SCRIPT: &str = r#"#!/usr/bin/env node
// lmux hook reporter (ESM, zero dependencies).
// 用法：node report.mjs done|blocked|working
// 身份和密钥来自 PTY env：LMUX_SOCKET / LMUX_AGENT_ID / LMUX_HOOK_TOKEN。
import net from 'node:net';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

function lmuxEnv(name) {
  if (process.env[name]) return process.env[name];
  if (!process.env.TMUX || !process.env.TMUX_PANE) return undefined;
  try {
    const line = execFileSync('tmux', ['show-environment', '-t', process.env.TMUX_PANE, name], { encoding: 'utf8' }).trim();
    return line.startsWith(name + '=') ? line.slice(name.length + 1) : undefined;
  } catch { return undefined; }
}

function parseJson(value) {
  const text = String(value || '').trim();
  if (!text) return null;
  try { return JSON.parse(text); } catch { return null; }
}

function payload() {
  for (let index = process.argv.length - 1; index >= 3; index--) {
    const parsed = parseJson(process.argv[index]);
    if (parsed && typeof parsed === 'object') return parsed;
  }
  if (!process.stdin.isTTY) {
    try {
      const parsed = parseJson(readFileSync(0, 'utf8'));
      if (parsed && typeof parsed === 'object') return parsed;
    } catch {}
  }
  return {};
}

function messageFrom(value) {
  const candidates = [
    value['last-assistant-message'], value.last_assistant_message,
    value.message, value.summary, value.title, value.notification, value.reason
  ];
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) return candidate.trim();
  }
  return null;
}

const event = process.argv[2] || 'done';
const socketPath = lmuxEnv('LMUX_SOCKET');
const agent = lmuxEnv('LMUX_AGENT_ID');
const token = lmuxEnv('LMUX_HOOK_TOKEN');
if (!socketPath || !agent || !token) process.exit(0);

const message = messageFrom(payload());
const line = JSON.stringify({
  id: Date.now(),
  method: 'agent.report',
  params: { token, agent, event, message },
}) + '\n';
const client = net.createConnection(socketPath);
client.setTimeout(1200);
client.on('connect', () => client.end(line));
client.on('error', () => process.exit(0));
client.on('timeout', () => { client.destroy(); process.exit(0); });
client.on('close', () => process.exit(0));
setTimeout(() => process.exit(0), 1500);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_hooks_merges_without_destroying() {
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
            Path::new("/tmp/lmux.sock"),
        );
        inject_claude_hooks(&settings, hooks).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(after["theme"], "dark"); // 用户的其它键保留
        assert!(after["hooks"]["PreToolUse"].is_array()); // 用户的 hook 保留
        assert!(after["hooks"]["Stop"].is_array()); // 新 hook 加入
        assert_eq!(after["hooks"]["Stop"][0]["matcher"], "");
        assert!(after["hooks"]["Stop"][0]["hooks"].is_array());
    }

    #[test]
    fn claude_same_event_hooks_coexist_and_uninstall_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{"hooks":{"Stop":[{"type":"command","command":"my-stop"}],"Notification":[{"type":"command","command":"my-note"}]}}"#,
        )
        .unwrap();
        let hooks = claude_hooks_value(dir.path(), "", Path::new("/tmp/lmux.sock"));
        inject_claude_hooks(&settings, hooks.clone()).unwrap();
        inject_claude_hooks(&settings, hooks).unwrap(); // 幂等，不重复 lmux
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(v["hooks"]["Notification"].as_array().unwrap().len(), 2);
        uninstall_claude_hooks(&settings).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["Stop"][0]["command"], "my-stop");
    }

    #[test]
    fn write_if_changed_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "old").unwrap();
        let changed = write_file_if_changed(&p, "new").unwrap();
        assert!(changed);
        let bak = p.with_extension("lmux-bak");
        assert_eq!(std::fs::read_to_string(bak).unwrap(), "old");
        let same = write_file_if_changed(&p, "new").unwrap();
        assert!(!same); // 幂等
    }

    #[test]
    fn codex_notify_chain() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "model = \"gpt\"\nnotify = [\"my-old-notify\"]\n").unwrap();
        inject_codex_notify(&cfg, Path::new("/tmp/hooks"), Path::new("/tmp/lmux.sock")).unwrap();
        let after = std::fs::read_to_string(&cfg).unwrap();
        let parsed: toml::Value = toml::from_str(&after).unwrap();
        let notify = parsed["notify"].as_array().unwrap();
        assert_eq!(
            notify,
            &vec![
                toml::Value::String("node".into()),
                toml::Value::String("/tmp/hooks/report.mjs".into()),
                toml::Value::String("done".into()),
            ]
        );
        // 备份链存在
        let prev = cfg.with_extension("lmux-notify-prev.json");
        assert!(prev.exists());
        // 还原
        uninstall_codex_notify(&cfg).unwrap();
        let restored = std::fs::read_to_string(&cfg).unwrap();
        assert!(restored.contains("my-old-notify"));
    }
}

/// OpenCode plugin：`session.idle` 权威上报 done。
pub const OPENCODE_PLUGIN: &str = r#"import net from "node:net"
import { execFileSync } from "node:child_process"

function lmuxEnv(name: string) {
  if (process.env[name]) return process.env[name]
  if (!process.env.TMUX || !process.env.TMUX_PANE) return undefined
  try {
    const line = execFileSync("tmux", ["show-environment", "-t", process.env.TMUX_PANE, name], { encoding: "utf8" }).trim()
    return line.startsWith(name + "=") ? line.slice(name.length + 1) : undefined
  } catch { return undefined }
}

function report(event: string, message: string) {
  const socket = lmuxEnv("LMUX_SOCKET")
  const agent = lmuxEnv("LMUX_AGENT_ID")
  const token = lmuxEnv("LMUX_HOOK_TOKEN")
  if (!socket || !agent || !token) return Promise.resolve()
  return new Promise<void>((resolve) => {
    const client = net.createConnection(socket)
    client.setTimeout(1200)
    client.on("connect", () => client.end(JSON.stringify({
      id: Date.now(), method: "agent.report",
      params: { token, agent, event, message }
    }) + "\n"))
    client.on("error", () => resolve())
    client.on("timeout", () => { client.destroy(); resolve() })
    client.on("close", () => resolve())
  })
}

function openCodeAssistantText(messages: any[]) {
  for (let index = (messages || []).length - 1; index >= 0; index--) {
    const item = messages[index]
    const info = item?.info || item?.message || item
    if (info?.role !== "assistant") continue
    const parts = item?.parts || info?.parts || []
    const text = parts
      .filter((part: any) => part?.type === "text" && typeof part.text === "string")
      .map((part: any) => part.text)
      .join("\n")
      .trim()
    if (text) return text
  }
  return ""
}

export const Lmux = async ({ client }: any) => ({
  event: async ({ event }: any) => {
    if (event.type === "session.idle") {
      let message = ""
      const sessionID = event?.properties?.sessionID
      if (sessionID && client?.session?.messages) {
        try {
          const response = await client.session.messages({ path: { id: sessionID } })
          message = openCodeAssistantText(response?.data || response || [])
        } catch {}
      }
      await report("done", message || "任务已完成")
    } else if (event.type === "session.error") {
      const err = event?.error?.message || event?.error || "执行出错"
      await report("done", `任务异常: ${err}`)
    } else if (event.type === "permission.ask" || event.type === "tool.confirm" || event.type === "prompt.ask") {
      const question = event?.properties?.question || event?.properties?.message || "等待用户确认授权"
      await report("blocked", question)
    } else if (event.type === "subagent.completed") {
      const summary = event?.properties?.summary || "Subagent 任务已完成"
      await report("working", summary)
    } else if (event.type === "subagent.failed" || event.type === "subagent.error") {
      const err = event?.properties?.error || "Subagent 执行异常"
      await report("done", `Subagent 异常: ${err}`)
    } else if (event.type === "session.prompt" || event.type === "message.created" || event.type === "tool.call") {
      await report("working", "")
    }
  }
})
"#;

/// Pi extension：基于 pi-wechat-notifier 模式支持待确认(ask_user)、Subagent 状态区分及异常捕获。
pub const PI_EXTENSION: &str = r#"import net from "node:net"
import { execFileSync } from "node:child_process"

function lmuxEnv(name: string) {
  if (process.env[name]) return process.env[name]
  if (!process.env.TMUX || !process.env.TMUX_PANE) return undefined
  try {
    const line = execFileSync("tmux", ["show-environment", "-t", process.env.TMUX_PANE, name], { encoding: "utf8" }).trim()
    return line.startsWith(name + "=") ? line.slice(name.length + 1) : undefined
  } catch { return undefined }
}

function report(event: string, message: string) {
  const socket = lmuxEnv("LMUX_SOCKET")
  const agent = lmuxEnv("LMUX_AGENT_ID")
  const token = lmuxEnv("LMUX_HOOK_TOKEN")
  if (!socket || !agent || !token) return Promise.resolve()
  return new Promise<void>((resolve) => {
    const client = net.createConnection(socket)
    client.setTimeout(1200)
    client.on("connect", () => client.end(JSON.stringify({
      id: Date.now(), method: "agent.report",
      params: { token, agent, event, message }
    }) + "\n"))
    client.on("error", () => resolve())
    client.on("timeout", () => { client.destroy(); resolve() })
    client.on("close", () => resolve())
  })
}

function assistantText(messages: any[]) {
  for (let index = (messages || []).length - 1; index >= 0; index--) {
    const message = messages[index]
    if (message?.role !== "assistant") continue
    if (typeof message.content === "string" && message.content.trim()) return message.content.trim()
    if (Array.isArray(message.content)) {
      const text = message.content
        .filter((part: any) => part?.type === "text" && typeof part.text === "string")
        .map((part: any) => part.text)
        .join("\n")
        .trim()
      if (text) return text
    }
  }
  return ""
}

function shortText(value: any, max = 160) {
  if (typeof value !== "string") return ""
  const t = value.replace(/\s+/g, " ").trim()
  return t.length <= max ? t : t.slice(0, max - 1) + "…"
}

export default function (pi: any) {
  let latestAssistant = ""
  const seenAskUserCalls = new Set<string>()

  pi.on("session_start", async () => {
    latestAssistant = ""
    seenAskUserCalls.clear()
  })
  pi.on("turn_start", async () => {
    await report("working", "")
  })
  pi.on("agent_start", async () => {
    await report("working", "")
  })

  // 待确认：捕获 ask_user, confirm, prompt_user 等工具
  pi.on("tool_execution_start", async (event: any) => {
    const toolName = event?.toolName || ""
    if (toolName === "ask_user" || toolName === "confirm" || toolName === "prompt_user" || toolName === "user_input") {
      const callId = event?.toolCallId || String(Date.now())
      if (seenAskUserCalls.has(callId)) return
      seenAskUserCalls.add(callId)
      const args = event?.args || {}
      const question = shortText(args.question || args.message || args.prompt || "等待用户确认")
      await report("blocked", question)
      return
    }
    // Subagent 派发
    if (toolName.includes("subagent") || toolName.includes("delegate") || toolName.includes("agent_call")) {
      const subName = event?.args?.agent || event?.args?.role || "Subagent"
      await report("working", `正在运行: ${subName}`)
    }
  })

  // Subagent 执行结束状态区分（成功 / 错误 / 异常）
  pi.on("tool_execution_end", async (event: any) => {
    const toolName = event?.toolName || ""
    if (toolName.includes("subagent") || toolName.includes("delegate") || toolName.includes("agent_call")) {
      const subName = event?.args?.agent || event?.args?.role || "Subagent"
      if (event?.isError || event?.error || event?.exception) {
        const err = shortText(event.error || event.exception || "执行异常")
        await report("done", `${subName} 异常: ${err}`)
      } else {
        await report("working", `${subName} 完成`)
      }
    }
  })

  pi.on("message_end", (event: any) => {
    const message = event?.message
    if (message?.role === "assistant") {
      const text = typeof message.content === "string" ? message.content : assistantText([message])
      if (text) latestAssistant = shortText(text, 180)
    }
  })

  pi.on("agent_end", async (event: any) => {
    const text = assistantText(event?.messages || [])
    if (text) latestAssistant = shortText(text, 180)
  })

  // 任务结算（完成或异常）
  pi.on("agent_settled", async (event: any, ctx: any) => {
    if (ctx?.isIdle && !ctx.isIdle()) return
    let msg = latestAssistant
    if (event?.error || ctx?.error) {
      const err = shortText(event?.error || ctx?.error || "执行出错")
      msg = `任务异常: ${err}`
    }
    await report("done", msg || "任务已完成")
    latestAssistant = ""
  })
}
"#;

/// 安装 OpenCode/Pi 插件（幂等，不覆盖其他插件）。
pub fn install_agent_plugins(home: &Path) -> Result<()> {
    let opencode = home.join(".config/opencode/plugins/lmux.ts");
    write_file_if_changed(&opencode, OPENCODE_PLUGIN)?;
    let pi = home.join(".pi/agent/extensions/lmux.ts");
    write_file_if_changed(&pi, PI_EXTENSION)?;
    Ok(())
}

#[cfg(test)]
mod plugin_tests {
    use super::*;
    #[test]
    fn installs_opencode_and_pi_plugins_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        install_agent_plugins(dir.path()).unwrap();
        install_agent_plugins(dir.path()).unwrap();
        let oc =
            std::fs::read_to_string(dir.path().join(".config/opencode/plugins/lmux.ts")).unwrap();
        let pi = std::fs::read_to_string(dir.path().join(".pi/agent/extensions/lmux.ts")).unwrap();
        assert!(oc.contains("client.session.messages"));
        assert!(pi.contains("agent_settled"));
        assert!(pi.contains("assistantText"));
    }
}
