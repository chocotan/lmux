//! 状态检测引擎：hook 权威上报 + manifest 屏幕规则兜底 + 防抖
pub mod manifest;

use crate::model::{AgentId, AgentStatus};
use manifest::CompiledManifest;
use std::collections::HashMap;
use std::time::Instant;

/// 一次屏幕采样的输入
#[derive(Debug, Clone, Default)]
pub struct ScreenInput {
    /// 终端屏幕底部可见行（含 prompt 区域），已按行切分
    pub bottom_lines: Vec<String>,
    /// OSC 标题（若有）
    pub osc_title: Option<String>,
    /// 最近一次输出距现在多少秒（输出静默时长）
    pub secs_since_output: Option<f64>,
    /// 最近一个采样周期内是否响铃
    pub bell: bool,
}

/// hook 上报事件（对应 wire 协议 agent.report 的 event 字段）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    Done,
    Blocked,
    Working,
}

impl HookEvent {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "done" => Some(HookEvent::Done),
            "blocked" => Some(HookEvent::Blocked),
            "working" => Some(HookEvent::Working),
            _ => None,
        }
    }
    pub fn to_status(self) -> AgentStatus {
        match self {
            HookEvent::Done => AgentStatus::Done,
            HookEvent::Blocked => AgentStatus::Blocked,
            HookEvent::Working => AgentStatus::Working,
        }
    }
}

#[derive(Debug, Clone)]
struct AgentDetectState {
    status: AgentStatus,
    /// 当前权威源
    authority: Authority,
    /// hook 最近一次上报时刻
    hook_last_seen: Option<Instant>,
    /// 防抖：候选状态与已连续次数
    pending: Option<(AgentStatus, u32)>,
    /// 状态序号，单调递增，乱序/回退丢弃
    seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Authority {
    Screen,
    Hook,
}

/// 引擎产出的状态更新（None = 无变化）
#[derive(Debug, Clone, PartialEq)]
pub struct StatusUpdate {
    pub agent: AgentId,
    pub to: AgentStatus,
    pub seq: u64,
}

pub struct DetectionEngine {
    manifests: HashMap<String, CompiledManifest>,
    states: HashMap<AgentId, AgentDetectState>,
    /// 防抖需要连续一致的采样次数
    debounce_ticks: u32,
}

/// hook 权威窗口：hook 上报后该时长内 hook 独占状态判定
const HOOK_AUTHORITY_WINDOW: std::time::Duration = std::time::Duration::from_secs(5 * 60);

impl DetectionEngine {
    pub fn new() -> Self {
        DetectionEngine {
            manifests: HashMap::new(),
            states: HashMap::new(),
            debounce_ticks: 2,
        }
    }

    /// 注册/覆盖某 agent 类型的 manifest（内置或 ~/.config/lmux/manifests）
    pub fn register_manifest(&mut self, m: CompiledManifest) {
        self.manifests.insert(m.agent_type.clone(), m);
    }

    pub fn has_hook_authority(&self, agent: &AgentId) -> bool {
        self.states
            .get(agent)
            .map(|s| matches!(s.authority, Authority::Hook))
            .unwrap_or(false)
    }

    fn state_mut(&mut self, agent: &AgentId, current: AgentStatus) -> &mut AgentDetectState {
        self.states
            .entry(agent.clone())
            .or_insert_with(|| AgentDetectState {
                status: current,
                authority: Authority::Screen,
                hook_last_seen: None,
                pending: None,
                seq: 0,
            })
    }

    pub fn forget(&mut self, agent: &AgentId) {
        self.states.remove(agent);
    }

    /// hook 上报入口（服务端线程投递）。权威窗口内独占。
    pub fn report(
        &mut self,
        agent: &AgentId,
        current: AgentStatus,
        ev: HookEvent,
    ) -> Option<StatusUpdate> {
        let st = self.state_mut(agent, current);
        st.hook_last_seen = Some(Instant::now());
        st.authority = Authority::Hook;
        st.seq += 1;
        let seq = st.seq;
        if st.status != ev.to_status() {
            st.status = ev.to_status();
            st.pending = None;
            Some(StatusUpdate {
                agent: agent.clone(),
                to: ev.to_status(),
                seq,
            })
        } else {
            st.pending = None;
            None
        }
    }

    /// 每 tick 屏幕采样。返回状态变化（已经过防抖）。
    pub fn observe(
        &mut self,
        agent: &AgentId,
        current: AgentStatus,
        input: &ScreenInput,
    ) -> Option<StatusUpdate> {
        // 1) hook 权威窗口内不采信屏幕规则
        if let Some(st) = self.states.get(agent) {
            if matches!(st.authority, Authority::Hook) {
                let fresh = st
                    .hook_last_seen
                    .map(|t| t.elapsed() < HOOK_AUTHORITY_WINDOW)
                    .unwrap_or(false);
                if fresh {
                    return None;
                }
                // 窗口过期 → 权威回落到屏幕
                let st = self.state_mut(agent, current);
                st.authority = Authority::Screen;
            }
        }

        // 2) 屏幕规则推导候选状态
        let candidate = self.screen_candidate(agent, input);
        let need = self.debounce_ticks;

        let st = self.state_mut(agent, current);
        let Some(candidate) = candidate else {
            st.pending = None;
            return None;
        };
        if candidate == st.status {
            st.pending = None;
            return None;
        }

        // 3) 防抖：连续 debounce_ticks 次一致才提交
        match &mut st.pending {
            Some((pend, n)) if *pend == candidate => {
                *n += 1;
                if *n >= need {
                    st.pending = None;
                    st.status = candidate;
                    st.seq += 1;
                    return Some(StatusUpdate {
                        agent: agent.clone(),
                        to: candidate,
                        seq: st.seq,
                    });
                }
                None
            }
            _ => {
                st.pending = Some((candidate, 1));
                None
            }
        }
    }

    /// 屏幕规则 → AgentStatus
    fn screen_candidate(&self, agent: &AgentId, input: &ScreenInput) -> Option<AgentStatus> {
        // agent.id 形如 "agent_<type>_<ulid>"，类型段解析不到就用全局兜底规则
        let agent_type = agent_type_of(agent);
        let m = self.manifests.get(agent_type)?;
        m.evaluate(input)
    }
}

impl Default for DetectionEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// AgentId 约定: "<agent_type>_<ulid>"；找不到则 "shell"
fn agent_type_of(agent: &AgentId) -> &str {
    agent.split('_').next().unwrap_or("shell")
}

/// 内置 manifest TOML（编译期嵌入）
pub fn builtin_manifests() -> Vec<CompiledManifest> {
    let sources = [
        ("claude", include_str!("manifests/claude.toml")),
        ("codex", include_str!("manifests/codex.toml")),
        ("opencode", include_str!("manifests/opencode.toml")),
        ("pi", include_str!("manifests/pi.toml")),
        ("shell", include_str!("manifests/shell.toml")),
    ];
    sources
        .into_iter()
        .filter_map(|(name, src)| match CompiledManifest::parse(name, src) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!(manifest = name, error = %e, "builtin manifest parse failed");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> DetectionEngine {
        let mut e = DetectionEngine::new();
        for m in builtin_manifests() {
            e.register_manifest(m);
        }
        e
    }

    fn input<'a>(lines: impl IntoIterator<Item = &'a str>) -> ScreenInput {
        ScreenInput {
            bottom_lines: lines.into_iter().map(|s| s.to_string()).collect(),
            osc_title: None,
            secs_since_output: Some(0.1),
            bell: false,
        }
    }

    #[test]
    fn claude_blocked_by_permission_prompt() {
        let mut e = engine();
        let a: AgentId = "claude_A01".into();
        assert_eq!(
            e.observe(&a, AgentStatus::Working, &input(["❯ do it"])),
            None
        ); // 第一次进 pending
        let lines = [" Do you want to proceed?", "❯ 1. Yes", "  2. No"];
        assert_eq!(e.observe(&a, AgentStatus::Working, &input(lines)), None);
        let upd = e.observe(&a, AgentStatus::Working, &input(lines)).unwrap();
        assert_eq!(upd.to, AgentStatus::Blocked);
    }

    #[test]
    fn debounce_rejects_flicker() {
        let mut e = engine();
        let a: AgentId = "claude_A01".into();
        // 单次出现 blocked 特征然后消失 → 不提交
        let lines = [" Do you want to proceed?"];
        e.observe(&a, AgentStatus::Working, &input(lines));
        e.observe(&a, AgentStatus::Working, &input(["working hard..."]));
        let r = e.observe(&a, AgentStatus::Working, &input(["working hard..."]));
        assert_eq!(r, None);
        assert!(!e.has_hook_authority(&a));
    }

    #[test]
    fn hook_report_is_authoritative_and_immediate() {
        let mut e = engine();
        let a: AgentId = "codex_B02".into();
        let upd = e.report(&a, AgentStatus::Working, HookEvent::Done).unwrap();
        assert_eq!(upd.to, AgentStatus::Done);
        // hook 窗口内屏幕规则不生效
        let r = e.observe(&a, AgentStatus::Done, &input(["some output"]));
        assert_eq!(r, None);
    }

    #[test]
    fn hook_report_same_status_no_event() {
        let mut e = engine();
        let a: AgentId = "pi_C03".into();
        assert_eq!(e.report(&a, AgentStatus::Done, HookEvent::Done), None);
    }

    #[test]
    fn unknown_type_uses_shell_rules_or_none() {
        let mut e = engine();
        let a: AgentId = "shell_D04".into();
        // 空闲 bash 无输出 → shell manifest 可判 idle；没有匹配特征则 None
        let _ = e.observe(&a, AgentStatus::Working, &input(["$ "]));
    }

    #[test]
    fn seq_monotonic() {
        let mut e = engine();
        let a: AgentId = "claude_E05".into();
        e.report(&a, AgentStatus::Working, HookEvent::Done);
        let lines = ["Do you want to proceed?"];
        e.observe(&a, AgentStatus::Done, &input(lines)); // hook 权威窗口内，屏幕不生效
        let s1 = e.report(&a, AgentStatus::Done, HookEvent::Working).unwrap();
        let s2 = e
            .report(&a, AgentStatus::Working, HookEvent::Blocked)
            .unwrap();
        assert!(s2.seq > s1.seq);
        assert_eq!(s2.to, AgentStatus::Blocked);
    }
}
