import net from "node:net"
import { execFileSync } from "node:child_process"

function muxlaneEnv(name = "") {
  if (process.env[name]) return process.env[name]
  if (!process.env.TMUX || !process.env.TMUX_PANE) return undefined
  try {
    const line = execFileSync("tmux", ["show-environment", "-t", process.env.TMUX_PANE, name], { encoding: "utf8" }).trim()
    return line.startsWith(name + "=") ? line.slice(name.length + 1) : undefined
  } catch { return undefined }
}

function report(event = "", message = "") {
  if ((typeof isLegacySubagentProcess !== "undefined" && isLegacySubagentProcess)
    || (typeof isSubagentProcess !== "undefined" && isSubagentProcess)) return Promise.resolve()
  const socket = muxlaneEnv("MUXLANE_SOCKET")
  const agent = muxlaneEnv("MUXLANE_AGENT_ID")
  const token = muxlaneEnv("MUXLANE_HOOK_TOKEN")
  if (!socket || !agent || !token) return Promise.resolve()
  return new Promise((resolve) => {
    const client = net.createConnection(socket)
    client.setTimeout(1200)
    client.on("connect", () => client.end(JSON.stringify({
      id: Date.now(), method: "agent.report",
      params: { token, agent, event, message }
    }) + "\n"))
    client.on("error", () => resolve(undefined))
    client.on("timeout", () => { client.destroy(); resolve(undefined) })
    client.on("close", () => resolve(undefined))
  })
}
