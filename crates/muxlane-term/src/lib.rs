//! muxlane-term：PTY 会话 + 终端模拟（与 GPUI 解耦）
mod replay;
mod session;
mod vterm;

pub use replay::ReplayBuffer;
pub use session::{default_shell_program, LaunchCfg, PtySession, SessionEvent};
pub use vterm::{RenderCursor, VTerm, VTermModes};
