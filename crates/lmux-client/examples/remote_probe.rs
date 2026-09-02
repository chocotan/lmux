use lmux_client::{HostCfg, RemoteHost, RemoteState, SshAuth, Target};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = std::env::var("LMUX_TEST_HOST")?;
    let username = std::env::var("LMUX_TEST_USERNAME")?;
    let password = std::env::var("LMUX_TEST_PASSWORD")?;
    let (tx, _rx) = mpsc::channel(16);
    let remote = RemoteHost::new(
        HostCfg {
            name: host.clone(),
            target: Target::Ssh {
                host,
                socket: String::new(),
            },
            auth: SshAuth::Password { username, password },
            retry_base_ms: 200,
        },
        tx,
    );
    tokio::spawn(std::sync::Arc::clone(&remote).run_loop());
    let mut upgraded = false;
    for _ in 0..120 {
        match remote.state().await {
            RemoteState::Online(snapshot) => {
                println!(
                    "ONLINE {} {}",
                    remote.endpoint_now().unwrap_or_default(),
                    snapshot.projects.len()
                );
                remote.stop();
                return Ok(());
            }
            RemoteState::AuthenticationFailed(error) => {
                anyhow::bail!("authentication failed: {error}")
            }
            RemoteState::NeedsInstall { .. } => anyhow::bail!("remote lmux needs installation"),
            RemoteState::NeedsStart { .. } => anyhow::bail!("remote lmux needs start"),
            RemoteState::NeedsUpgrade { .. } => {
                if std::env::var("LMUX_TEST_UPGRADE").as_deref() == Ok("1") && !upgraded {
                    std::sync::Arc::clone(&remote).upgrade_and_retry().await?;
                    upgraded = true;
                } else if !upgraded {
                    anyhow::bail!("remote lmux needs upgrade");
                }
            }
            RemoteState::Offline(error) => anyhow::bail!("remote offline: {error}"),
            RemoteState::Connecting(_) => {}
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    anyhow::bail!("remote connection timeout")
}
