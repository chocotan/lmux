// muxlane hook reporter (ESM, zero dependencies).
// 用法：node report.mjs done|failed|blocked|working
// 身份和密钥来自 PTY env：MUXLANE_SOCKET / MUXLANE_AGENT_ID / MUXLANE_HOOK_TOKEN。
import { readFileSync } from "node:fs"

const isSubagentProcess = false

function parseJson(value) {
  const text = String(value || "").trim()
  if (!text) return null
  try { return JSON.parse(text) } catch { return null }
}

function payload() {
  for (let index = process.argv.length - 1; index >= 3; index--) {
    const parsed = parseJson(process.argv[index])
    if (parsed && typeof parsed === "object") return parsed
  }
  if (!process.stdin.isTTY) {
    try {
      const parsed = parseJson(readFileSync(0, "utf8"))
      if (parsed && typeof parsed === "object") return parsed
    } catch {}
  }
  return {}
}

function messageFrom(value) {
  const candidates = [
    value["last-assistant-message"], value.last_assistant_message,
    value.message, value.summary, value.title, value.notification, value.reason
  ]
  for (const candidate of candidates) {
    if (typeof candidate === "string" && candidate.trim()) return candidate.trim()
  }
  return null
}

const event = process.argv[2] || "done"
const message = messageFrom(payload())
const exitTimer = setTimeout(() => process.exit(0), 1500)
await report(event, message)
clearTimeout(exitTimer)
