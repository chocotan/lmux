//! muxlane 持久化：原子写 + 版本字段 + 安全默认值。
use muxlane_core::model::{AgentId, AgentType, Project, ProjectId, Snapshot};
use muxlane_core::{PaneId, PaneNode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
const SECRETS_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedSecrets {
    version: u32,
    #[serde(default)]
    remote_passwords: BTreeMap<String, String>,
}

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
    pub sessions: Vec<PersistedAgent>,
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
    pub osc52_clipboard_enabled: Option<bool>,
    #[serde(default)]
    pub language: Option<String>,
}

impl PersistedApp {
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        let mut projects = snapshot.projects.clone();
        for project in &mut projects {
            project.agents.clear();
        }
        Self {
            initialized: true,
            projects,
            sessions: snapshot
                .agents
                .iter()
                .filter_map(|agent| {
                    Some(PersistedAgent {
                        agent_id: agent.id.clone(),
                        project_id: agent.project.clone(),
                        agent_type: agent.agent_type,
                        title: agent.title.clone(),
                        tmux_session: agent.tmux_session.clone()?,
                    })
                })
                .collect(),
            ..Self::default()
        }
    }

    pub fn with_ui_prefs_from(mut self, previous: &Self) -> Self {
        self.remotes = previous.remotes.clone();
        self.remote_configs = previous.remote_configs.clone();
        self.pane_tree = previous.pane_tree.clone();
        self.active_pane = previous.active_pane.clone();
        self.window = previous.window;
        self.dark_mode = previous.dark_mode;
        self.theme = previous.theme.clone();
        self.font_family = previous.font_family.clone();
        self.sound_enabled = previous.sound_enabled;
        self.osc52_clipboard_enabled = previous.osc52_clipboard_enabled;
        self.language = previous.language.clone();
        self
    }
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
            osc52_clipboard_enabled: None,
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
pub struct PersistedAgent {
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
        let secrets_path = secrets_path(path);
        let mut secrets = load_secrets(&secrets_path)?;
        let mut migrated_passwords = false;
        for remote in &mut app.remote_configs {
            if let PersistedRemoteAuth::Password { password, .. } = &mut remote.auth {
                if let Some(inline) = password.take() {
                    secrets
                        .remote_passwords
                        .insert(remote.target.clone(), inline);
                    migrated_passwords = true;
                }
            }
        }
        if migrated_passwords {
            write_secrets(&secrets_path, &secrets)?;
            write_state(path, &app)?;
        }
        restore_passwords(&mut app, &secrets);
        Ok(app)
    })();
    if result.is_err() {
        cleanup_temporary_files(path);
    }
    result
}

pub fn save(path: &Path, app: &PersistedApp) -> anyhow::Result<()> {
    let mut state = app.clone();
    let mut secrets = PersistedSecrets {
        version: SECRETS_VERSION,
        ..Default::default()
    };
    for remote in &mut state.remote_configs {
        if let PersistedRemoteAuth::Password { password, .. } = &mut remote.auth {
            if let Some(password) = password.take() {
                secrets
                    .remote_passwords
                    .insert(remote.target.clone(), password);
            }
        }
    }
    write_secrets(&secrets_path(path), &secrets)?;
    write_state(path, &state)
}

fn secrets_path(state_path: &Path) -> PathBuf {
    state_path.with_file_name("secrets.json")
}

fn load_secrets(path: &Path) -> anyhow::Result<PersistedSecrets> {
    if !path.exists() {
        return Ok(PersistedSecrets {
            version: SECRETS_VERSION,
            ..Default::default()
        });
    }
    let secrets: PersistedSecrets = serde_json::from_slice(&std::fs::read(path)?)?;
    if secrets.version > SECRETS_VERSION {
        anyhow::bail!(
            "secrets version {} is newer than supported {}",
            secrets.version,
            SECRETS_VERSION
        );
    }
    Ok(secrets)
}

fn restore_passwords(app: &mut PersistedApp, secrets: &PersistedSecrets) {
    for remote in &mut app.remote_configs {
        if let PersistedRemoteAuth::Password { password, .. } = &mut remote.auth {
            *password = secrets.remote_passwords.get(&remote.target).cloned();
        }
    }
}

fn write_state(path: &Path, app: &PersistedApp) -> anyhow::Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(app)?, None)
}

fn write_secrets(path: &Path, secrets: &PersistedSecrets) -> anyhow::Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(secrets)?, Some(0o600))
}

fn write_atomic(path: &Path, data: &[u8], mode: Option<u32>) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temporary_path(path);
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        let mut file = options.open(&tmp)?;
        use std::io::Write;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        }
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
    fn from_snapshot_keeps_only_persistable_runtime_state() {
        let snapshot = Snapshot {
            machine: None,
            projects: vec![Project {
                id: "p".into(),
                name: "repo".into(),
                path: "/tmp/repo".into(),
                branch: Some("main".into()),
                agents: vec!["tmux-agent".into(), "plain-agent".into()],
            }],
            agents: vec![
                muxlane_core::model::AgentInstance {
                    id: "tmux-agent".into(),
                    project: "p".into(),
                    agent_type: AgentType::Claude,
                    title: "work".into(),
                    status: muxlane_core::model::AgentStatus::Working,
                    status_since: 1,
                    seen: false,
                    tmux_session: Some("muxlane-tmux-agent".into()),
                },
                muxlane_core::model::AgentInstance {
                    id: "plain-agent".into(),
                    project: "p".into(),
                    agent_type: AgentType::Shell,
                    title: "shell".into(),
                    status: muxlane_core::model::AgentStatus::Idle,
                    status_since: 2,
                    seen: true,
                    tmux_session: None,
                },
            ],
        };

        let mut previous = PersistedApp::default();
        let pane = previous.pane_tree.first_pane_id();
        previous.pane_tree.open_tab(&pane, "tmux-agent".into());
        previous.theme = Some("nord".into());
        previous.osc52_clipboard_enabled = Some(true);
        previous.maximized_pane = Some(pane);
        let app = PersistedApp::from_snapshot(&snapshot).with_ui_prefs_from(&previous);

        assert!(app.initialized);
        assert!(app.projects[0].agents.is_empty());
        assert_eq!(app.projects[0].branch.as_deref(), Some("main"));
        assert_eq!(
            app.sessions,
            vec![PersistedAgent {
                agent_id: "tmux-agent".into(),
                project_id: "p".into(),
                agent_type: AgentType::Claude,
                title: "work".into(),
                tmux_session: "muxlane-tmux-agent".into(),
            }]
        );
        assert_eq!(app.theme.as_deref(), Some("nord"));
        assert_eq!(app.osc52_clipboard_enabled, Some(true));
        assert_eq!(app.pane_tree, previous.pane_tree);
        assert!(app.maximized_pane.is_none());
    }

    #[test]
    fn from_empty_snapshot_has_empty_projects_and_sessions() {
        let app = PersistedApp::from_snapshot(&Snapshot::default());
        assert!(app.initialized);
        assert!(app.projects.is_empty());
        assert!(app.sessions.is_empty());
    }

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
        app.sessions.push(PersistedAgent {
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
        let json = std::fs::read_to_string(&p).unwrap();
        assert!(json.contains("\"sessions\""));
        let back = load(&p).unwrap();
        assert_eq!(back, app);
        assert!(std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .all(|entry| entry.file_name() != "state.json.tmp"));
    }

    #[test]
    fn passwords_are_stored_separately_and_restored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut app = PersistedApp::default();
        app.remote_configs.push(PersistedRemote {
            target: "alice@nuc".into(),
            auth: PersistedRemoteAuth::Password {
                username: "alice".into(),
                password: Some("correct horse battery staple".into()),
            },
        });

        save(&path, &app).unwrap();

        let state = std::fs::read_to_string(&path).unwrap();
        assert!(!state.contains("correct horse battery staple"));
        assert_eq!(load(&path).unwrap(), app);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.path().join("secrets.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn inline_password_is_migrated_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            r#"{"version":1,"remote_configs":[{"target":"alice@nuc","auth":{"auth":"password","username":"alice","password":"legacy-secret"}}]}"#,
        )
        .unwrap();

        let first = load(&path).unwrap();
        let second = load(&path).unwrap();

        assert_eq!(first, second);
        assert!(matches!(
            &first.remote_configs[0].auth,
            PersistedRemoteAuth::Password { password: Some(password), .. }
                if password == "legacy-secret"
        ));
        assert!(!std::fs::read_to_string(path)
            .unwrap()
            .contains("legacy-secret"));
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
