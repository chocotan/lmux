//! lmux-term：PTY 会话 + 终端模拟（与 GPUI 解耦）
mod replay;
mod session;
mod vterm;

pub use replay::ReplayBuffer;
pub use session::{default_shell_program, LaunchCfg, PtySession, SessionEvent};
pub use vterm::{RenderCursor, VTerm, VTermModes};

use base64::Engine;

/// base64 编解码快捷方式（wire 协议用）
pub fn b64_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub fn b64_decode(s: &str) -> anyhow::Result<Vec<u8>> {
    Ok(base64::engine::general_purpose::STANDARD.decode(s)?)
}

/// 从原始字节流提取 OSC 标题（终端标题变化序列 `ESC ]0;title BEL`）
pub fn extract_osc_title(buf: &[u8]) -> Option<String> {
    const OSC_START: &[u8] = b"\x1b]";
    const BEL: u8 = 0x07;
    const ST: &[u8] = b"\x1b\\";
    let mut rest = buf;
    let mut last = None;
    while let Some(i) = find(rest, OSC_START) {
        let after = &rest[i + OSC_START.len()..];
        let Some(semi) = after.iter().position(|&b| b == b';') else {
            break;
        };
        let valid = matches!(after.first(), Some(b'0' | b'2'));
        let end_bel = after.iter().position(|&b| b == BEL);
        let end_st = find(after, ST);
        let Some(end) = (match (end_bel, end_st) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }) else {
            break;
        };
        if valid && semi < end {
            last = Some(String::from_utf8_lossy(&after[semi + 1..end]).into_owned());
        }
        let terminator_len = if end_bel == Some(end) { 1 } else { ST.len() };
        rest = &after[(end + terminator_len).min(after.len())..];
    }
    last
}
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 去除 ANSI 转义序列（粗略），返回纯文本行（屏幕检测用）
pub fn strip_ansi(buf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(buf);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                chars.next();
                // OSC: 直到 BEL 或 ESC \
                while let Some(&c2) = chars.peek() {
                    if c2 == '\x07' {
                        chars.next();
                        break;
                    }
                    if c2 == '\x1b' {
                        chars.next();
                        chars.next();
                        break;
                    }
                    chars.next();
                }
            }
            continue;
        }
        out.push(c);
    }
    out.lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc_title_extraction() {
        let data = b"\x1b]0;my title\x07some output\x1b]2;second\x1b\\more";
        assert_eq!(extract_osc_title(data).as_deref(), Some("second"));
    }

    #[test]
    fn strip_ansi_basic() {
        let data = b"\x1b[31mHello\x1b[0m world\r\n\x1b]0;title\x07\x1b[1;32mnext line\x1b[0m";
        let lines = strip_ansi(data);
        assert_eq!(lines, vec!["Hello world", "next line"]);
    }

    #[test]
    fn b64_roundtrip() {
        let s = b"hello \xe4\xbd\xa0\xe5\xa5\xbd";
        assert_eq!(b64_decode(&b64_encode(s)).unwrap(), s);
    }
}
