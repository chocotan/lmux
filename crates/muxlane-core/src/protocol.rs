//! muxlane wire 协议 v1：newline-JSON 帧
use crate::model::{AgentId, AgentStatus, ProjectId};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_FRAME: usize = 1024 * 1024; // 1 MiB
pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Response {
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn err(id: u64, code: &str, message: impl Into<String>) -> Self {
        Response {
            id,
            result: None,
            error: Some(RpcError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMsg {
    pub event: String,
    pub params: Value,
}

impl EventMsg {
    pub fn new(event: &str, params: Value) -> Self {
        EventMsg {
            event: event.into(),
            params,
        }
    }
}

/// 事件名常量
pub mod events {
    pub const STATE_CHANGED: &str = "state.changed";
    pub const AGENT_STATUS: &str = "agent.status_changed";
    pub const TERM_DATA: &str = "term.data";
    pub const TERM_RESYNC: &str = "term.resync";
    pub const TERM_EXIT: &str = "term.exit";
}

/// 特性名常量
pub mod features {
    pub const PROJECT_ADD: &str = "project.add";
    pub const AGENT_SPAWN: &str = "agent.spawn";
    pub const TERM_INPUT: &str = "term.input";
    pub const TERM_RESIZE: &str = "term.resize";
}
/// 方法名常量
pub mod methods {
    pub const STATE_LIST: &str = "state.list";
    pub const SYSTEM_HELLO: &str = "system.hello";
    pub const TERM_SUBSCRIBE: &str = "term.subscribe";
    pub const TERM_UNSUBSCRIBE: &str = "term.unsubscribe";
    pub const TERM_INPUT: &str = "term.input";
    pub const TERM_RESIZE: &str = "term.resize";
    pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";
    pub const AGENT_REPORT: &str = "agent.report";
    pub const AGENT_SPAWN: &str = "agent.spawn";
    pub const AGENT_DELETE: &str = "agent.delete";
    pub const PROJECT_ADD: &str = "project.add";
    pub const PROJECT_DELETE: &str = "project.delete";
    pub const PAIR_BEGIN: &str = "pair.begin";
}

// ── 方法参数/结果 ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloResult {
    pub version: String,
    pub protocol: u32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermSubscribeParams {
    pub agent: AgentId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermSubscribeResult {
    pub sub_id: String,
    /// base64 的回放快照（历史字节，客户端喂入 Term 快进）
    pub replay_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermInputParams {
    pub agent: AgentId,
    pub data_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermResizeParams {
    pub agent: AgentId,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermDataEvent {
    pub agent: AgentId,
    /// base64 PTY 字节增量
    pub data_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermResyncEvent {
    pub agent: AgentId,
    pub replay_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusEvent {
    pub agent: AgentId,
    pub from: AgentStatus,
    pub to: AgentStatus,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDeleteParams {
    pub agent: AgentId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpawnParams {
    pub project: ProjectId,
    #[serde(default)]
    pub agent_type: Option<crate::model::AgentType>,
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<Vec<(String, String)>>,
    #[serde(default)]
    pub preset_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectAddParams {
    pub path: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDeleteParams {
    pub project: ProjectId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteScopeResult {
    pub destroyed_agents: Vec<AgentId>,
    #[serde(default)]
    pub failed_agents: Vec<AgentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReportParams {
    pub token: String,
    pub agent: AgentId,
    /// "done" | "blocked" | "working"
    pub event: String,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermExitEvent {
    pub agent: AgentId,
}

// ── newline-JSON 帧编解码 ─────────────────────

/// 从 AsyncRead 读一行 JSON 帧（自带缓冲累积，兼容裸 stream）
pub async fn read_frame<R>(reader: &mut R) -> Result<Value>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut line = Vec::with_capacity(256);
    let mut one = [0u8; 1];
    loop {
        let n = reader.read(&mut one).await?;
        if n == 0 {
            return Err(Error::Eof);
        }
        if one[0] == b'\n' {
            break;
        }
        line.push(one[0]);
        if line.len() > MAX_FRAME {
            return Err(Error::FrameTooLarge {
                size: line.len(),
                max: MAX_FRAME,
            });
        }
    }
    if line.is_empty() {
        // 空行跳过由 caller 处理；这里视为协议错误
        return Err(Error::Json(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "empty frame",
        ))));
    }
    Ok(serde_json::from_slice(&line)?)
}

/// 写一行 JSON 帧（自动追加 \n）
pub async fn write_frame<W>(writer: &mut W, value: &impl Serialize) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let mut buf = serde_json::to_vec(value)?;
    if buf.len() > MAX_FRAME {
        return Err(Error::FrameTooLarge {
            size: buf.len(),
            max: MAX_FRAME,
        });
    }
    buf.push(b'\n');
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

/// 同步版本（供 hook 脚本协议测试等）
pub fn encode_line(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut buf = serde_json::to_vec(value)?;
    buf.push(b'\n');
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Snapshot;

    #[tokio::test]
    async fn frame_roundtrip() {
        let mut cursor = std::io::Cursor::new(b"{\"a\":1}\n{\"b\":[2,3]}\n".to_vec());
        let v1 = read_frame(&mut cursor).await.unwrap();
        assert_eq!(v1["a"], 1);
        let v2 = read_frame(&mut cursor).await.unwrap();
        assert_eq!(v2["b"][1], 3);
        let eof = read_frame(&mut cursor).await;
        assert!(matches!(eof, Err(Error::Eof)));
    }

    #[tokio::test]
    async fn request_response_roundtrip() {
        let req = Request {
            id: 7,
            method: methods::STATE_LIST.into(),
            params: Value::Null,
        };
        let mut buf = Vec::new();
        {
            let mut w = tokio::io::BufWriter::new(&mut buf);
            write_frame(&mut w, &req).await.unwrap();
        }
        let mut r = std::io::Cursor::new(buf);
        let v = read_frame(&mut r).await.unwrap();
        let back: Request = serde_json::from_value(v).unwrap();
        assert_eq!(back.id, 7);
        assert_eq!(back.method, "state.list");
    }

    #[tokio::test]
    async fn snapshot_in_response() {
        let snap = Snapshot::default();
        let resp = Response::ok(1, serde_json::to_value(&snap).unwrap());
        let line = encode_line(&resp).unwrap();
        let mut r = std::io::Cursor::new(line);
        let v = read_frame(&mut r).await.unwrap();
        let back: Response = serde_json::from_value(v).unwrap();
        let snap2: Snapshot = serde_json::from_value(back.result.unwrap()).unwrap();
        assert_eq!(snap2, Snapshot::default());
    }

    #[tokio::test]
    async fn oversized_write_rejected() {
        let mut output = Vec::new();
        let error = write_frame(&mut output, &"x".repeat(MAX_FRAME))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::FrameTooLarge { size, max } if size > max && max == MAX_FRAME
        ));
    }

    #[tokio::test]
    async fn oversized_frame_rejected() {
        let big = "x".repeat(MAX_FRAME + 10);
        let data = format!("\"{}\"\n", big);
        let mut r = std::io::Cursor::new(data.into_bytes());
        let err = read_frame(&mut r).await.unwrap_err();
        assert!(matches!(err, Error::FrameTooLarge { .. }));
    }
}
