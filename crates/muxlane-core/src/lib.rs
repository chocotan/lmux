//! muxlane-core：协议、数据模型、状态检测、hook 注入
pub mod auth;
pub mod detect;
mod error;
pub mod hook;
pub mod model;
pub mod pane;
pub mod preset;
pub mod protocol;

pub use error::{Error, Result};

pub use detect::{DetectionEngine, HookEvent, ScreenStatusUpdate, StatusUpdate};
pub use model::{
    AgentId, AgentInstance, AgentStatus, AgentType, MachineId, MachineInfo, Project, ProjectId,
    Snapshot,
};

pub use auth::AuthSecret;
pub use pane::{PaneId, PaneNode, SplitAxis, TabGroup};
pub use preset::{builtin_presets, AgentPreset};
