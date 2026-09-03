//! muxlane 持久化：原子写 + 版本字段 + 安全默认值。
use muxlane_core::model::{AgentId, AgentType, Project, ProjectId};
use muxlane_core::{PaneId, PaneNode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    path.with_file_name(format!(
        "{name}.tmp.{}",
        muxlane_core::model::new_id("write")
    ))
}

fn cleanup_temporary_files(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let prefix = format!("{name}.tmp.");
    let legacy = path.with_file_name(format!("{name}.tmp"));
    let _ = std::fs::remove_file(legacy);
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|candidate| candidate.starts_with(&prefix))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

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
    #[serde(default)]
    pub dark_mode: Option<bool>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub sound_enabled: Option<bool>,
    #[serde(default)]
    pub language: Option<String>,
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
            dark_mode: None,
            theme: None,
            font_family: None,
            sound_enabled: None,
            language: None,
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
    Password {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: Option<String>,
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
    let result = (|| {
        if !path.exists() {
            return Ok(PersistedApp::default());
        }
        let bytes = std::fs::read(path)?;
        let mut app: PersistedApp = serde_json::from_slice(&bytes)?;
        migrate(&mut app)?;
        Ok(app)
    })();
    if result.is_err() {
        cleanup_temporary_files(path);
    }
    result
}

pub fn save(path: &Path, app: &PersistedApp) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temporary_path(path);
    let result = (|| {
        let data = serde_json::to_vec_pretty(app)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)?;
        use std::io::Write;
        file.write_all(&data)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok::<_, anyhow::Error>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
    if app.remote_configs.is_empty() && !app.remotes.is_empty() {
        app.remote_configs = app
            .remotes
            .drain(..)
            .map(|target| PersistedRemote {
                target,
                auth: PersistedRemoteAuth::SshConfig,
            })
            .collect();
    }
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
    fn concurrent_saves_leave_valid_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("state.json"));
        let app = std::sync::Arc::new(PersistedApp::default());
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let path = std::sync::Arc::clone(&path);
                let app = std::sync::Arc::clone(&app);
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        save(&path, &app).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        load(&path).unwrap();
    }
    #[test]
    fn roundtrip_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut app = PersistedApp::default();
        app.remote_configs.push(PersistedRemote {
            target: "user@nuc:/tmp/muxlane.sock".into(),
            auth: PersistedRemoteAuth::SshConfig,
        });
        app.sessions.push(PersistedSession {
            agent_id: "a".into(),
            project_id: "p".into(),
            agent_type: AgentType::Shell,
            title: "zsh".into(),
            tmux_session: "muxlane-a".into(),
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
            .split(&root, muxlane_core::SplitAxis::Horizontal, "b".into())
            .unwrap();
        app.active_pane = Some(second.clone());
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
        assert!(std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .all(|entry| entry.file_name() != "state.json.tmp"));
    }
    #[test]
    fn legacy_remote_targets_migrate_to_remote_configs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        std::fs::write(
            &p,
            r#"{"version":1,"remotes":["user@nuc"],"projects":[],"sessions":[]}"#,
        )
        .unwrap();
        let app = load(&p).unwrap();
        assert!(app.remotes.is_empty());
        assert_eq!(app.remote_configs.len(), 1);
        assert_eq!(app.remote_configs[0].target, "user@nuc");
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
