//! lmux 持久化：原子写 + 版本字段 + 安全默认值。
use lmux_core::model::{AgentId, AgentType, Project, ProjectId};
use lmux_core::{PaneId, PaneNode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedApp {
    pub version: u32,
    #[serde(default)]
    pub initialized: bool,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub remotes: Vec<String>,
    #[serde(default)]
    pub remote_configs: Vec<PersistedRemote>,
    #[serde(default)]
    pub sessions: Vec<PersistedSession>,
    #[serde(default = "PaneNode::empty")]
    pub pane_tree: PaneNode,
    #[serde(default)]
    pub active_pane: Option<PaneId>,
    #[serde(default)]
    pub maximized_pane: Option<PaneId>,
    #[serde(default)]
    pub window: Option<WindowGeometry>,
}

impl Default for PersistedApp {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            initialized: false,
            projects: vec![],
            remotes: vec![],
            remote_configs: vec![],
            sessions: vec![],
            pane_tree: PaneNode::empty(),
            active_pane: None,
            maximized_pane: None,
            window: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "auth", rename_all = "snake_case")]
pub enum PersistedRemoteAuth {
    SshConfig,
    PublicKey {
        #[serde(default)]
        username: Option<String>,
        #[serde(default)]
        identity_file: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedRemote {
    pub target: String,
    pub auth: PersistedRemoteAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSession {
    pub agent_id: AgentId,
    pub project_id: ProjectId,
    pub agent_type: AgentType,
    pub title: String,
    pub tmux_session: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

pub fn load(path: &Path) -> anyhow::Result<PersistedApp> {
    if !path.exists() {
        return Ok(PersistedApp::default());
    }
    let bytes = std::fs::read(path)?;
    let mut app: PersistedApp = serde_json::from_slice(&bytes)?;
    migrate(&mut app)?;
    Ok(app)
}

pub fn save(path: &Path, app: &PersistedApp) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(app)?;
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn migrate(app: &mut PersistedApp) -> anyhow::Result<()> {
    if app.version > STORE_VERSION {
        anyhow::bail!(
            "state version {} is newer than supported {}",
            app.version,
            STORE_VERSION
        );
    }
    // v1 是首版；后续在此逐版本迁移。
    app.version = STORE_VERSION;
    Ok(())
}

pub fn default_path(data_dir: &Path) -> PathBuf {
    data_dir.join("state.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut app = PersistedApp::default();
        app.remotes.push("user@nuc:/tmp/lmux.sock".into());
        app.sessions.push(PersistedSession {
            agent_id: "a".into(),
            project_id: "p".into(),
            agent_type: AgentType::Shell,
            title: "zsh".into(),
            tmux_session: "lmux-a".into(),
        });
        app.window = Some(WindowGeometry {
            x: 10.0,
            y: 20.0,
            width: 1200.0,
            height: 800.0,
            maximized: true,
        });
        let root = app.pane_tree.first_pane_id();
        app.pane_tree.open_tab(&root, "a".into());
        let second = app
            .pane_tree
            .split(&root, lmux_core::SplitAxis::Horizontal, "b".into())
            .unwrap();
        app.active_pane = Some(second.clone());
        app.maximized_pane = Some(second);
        app.projects.push(Project {
            id: "p".into(),
            name: "repo".into(),
            path: "/tmp/repo".into(),
            branch: Some("main".into()),
            agents: vec![],
        });
        save(&p, &app).unwrap();
        let back = load(&p).unwrap();
        assert_eq!(back, app);
        assert!(!p.with_extension("json.tmp").exists());
    }
    #[test]
    fn missing_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let app = load(&dir.path().join("missing.json")).unwrap();
        assert_eq!(app.version, STORE_VERSION);
    }
    #[test]
    fn future_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        std::fs::write(&p, r#"{"version":999}"#).unwrap();
        assert!(load(&p).is_err());
    }

    #[test]
    fn corrupted_json_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        std::fs::write(&p, b"{truncated").unwrap();
        assert!(load(&p).is_err());
    }
}
