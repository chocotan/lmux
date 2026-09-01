//! SSH 隧道：复用 ControlMaster，把远端 lmux.sock 转发到本地（P1 收尾接入 RemoteHost）
#![allow(dead_code)]
use crate::host::SshAuth;
use std::path::PathBuf;
use std::sync::LazyLock;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    #[error("SSH authentication failed: {0}")]
    Authentication(String),
    #[error("lmux is not installed; expected socket {remote_socket}")]
    NeedsInstall { remote_socket: String },
    #[error("lmux is installed but not running ({binary})")]
    NeedsStart {
        remote_socket: String,
        binary: String,
    },
    #[error("SSH/tunnel failed: {0}")]
    Other(String),
}

fn askpass_script() -> PathBuf {
    let path = data_dir().join("ssh/lmux-askpass.sh");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = "#!/bin/sh\nsecret=$(cat -- \"$LMUX_SSH_SECRET_FILE\" 2>/dev/null) || exit 1\nrm -f -- \"$LMUX_SSH_SECRET_FILE\"\nprintf '%s\\n' \"$secret\"\n";
    if std::fs::read_to_string(&path).ok().as_deref() != Some(content) {
        let _ = std::fs::write(&path, content);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
        }
    }
    path
}

fn ssh_command(auth: &SshAuth) -> Command {
    let mut command = match auth {
        SshAuth::Password { password, .. } => {
            let runtime = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("lmux");
            let _ = std::fs::create_dir_all(&runtime);
            let secret_file = runtime.join(format!(
                "ssh-secret-{}",
                lmux_core::model::new_id("password")
            ));
            let _ = std::fs::write(&secret_file, password);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&secret_file, std::fs::Permissions::from_mode(0o600));
            }
            let mut command = Command::new("setsid");
            command
                .arg("sh")
                .arg("-c")
                .arg("ssh \"$@\"; status=$?; rm -f -- \"$LMUX_SSH_SECRET_FILE\"; exit $status")
                .arg("lmux-ssh");
            command.env("SSH_ASKPASS", askpass_script());
            command.env("SSH_ASKPASS_REQUIRE", "force");
            command.env("LMUX_SSH_SECRET_FILE", secret_file);
            command.env(
                "DISPLAY",
                std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),
            );
            command
        }
        _ => Command::new("ssh"),
    };
    command.args(["-o", "ConnectTimeout=5", "-o", "NumberOfPasswordPrompts=1"]);
    match auth {
        SshAuth::SshConfig => {
            command.args(["-o", "BatchMode=yes"]);
        }
        SshAuth::PublicKey { identity_file, .. } => {
            command.args(["-o", "BatchMode=yes"]);
            if let Some(identity) = identity_file
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                let identity = if let Some(rest) = identity.strip_prefix("~/") {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_default()
                        .join(rest)
                        .display()
                        .to_string()
                } else {
                    identity.to_string()
                };
                command
                    .arg("-o")
                    .arg("IdentitiesOnly=yes")
                    .arg("-i")
                    .arg(identity);
            }
        }
        SshAuth::Password { .. } => {
            command.args([
                "-o",
                "BatchMode=no",
                "-o",
                "PreferredAuthentications=password,keyboard-interactive",
            ]);
        }
    }
    command
}

fn classify_failure(stderr: &[u8]) -> TunnelError {
    let message = String::from_utf8_lossy(stderr).trim().to_string();
    if message.contains("Permission denied") || message.contains("Authentication failed") {
        TunnelError::Authentication(message)
    } else {
        TunnelError::Other(if message.is_empty() {
            "unknown SSH failure".into()
        } else {
            message
        })
    }
}

#[derive(Debug, Clone)]
struct TunnelEntry {
    local: String,
    control_path: PathBuf,
    destination: String,
}

/// 隧道表：host → 精确 local/control/destination。
static TUNNELS: LazyLock<Mutex<Option<std::collections::HashMap<String, TunnelEntry>>>> =
    LazyLock::new(|| Mutex::new(None));

pub async fn release_tunnel(host: &str) {
    if let Some(entry) = TUNNELS
        .lock()
        .await
        .as_mut()
        .and_then(|tunnels| tunnels.remove(host))
    {
        let _ = std::fs::remove_file(entry.local);
        let _ = Command::new("ssh")
            .arg("-S")
            .arg(&entry.control_path)
            .args(["-O", "exit"])
            .arg(&entry.destination)
            .output()
            .await;
        let _ = std::fs::remove_file(entry.control_path);
    }
}

#[allow(dead_code)]
pub fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".local/share")
        })
        .join("lmux")
}

/// 确保到 host 的隧道，返回本地可连的 unix socket 路径。
pub async fn ensure_tunnel(
    host: &str,
    remote_socket: &str,
    auth: &SshAuth,
) -> Result<String, TunnelError> {
    let destination = auth.destination(host);
    let discovered;
    let remote_socket = if remote_socket.trim().is_empty() {
        discovered = match probe_remote(host, auth).await? {
            RemoteProbe::Ready { remote_socket } => remote_socket,
        };
        discovered.as_str()
    } else {
        remote_socket
    };
    let cached = {
        let guard = TUNNELS.lock().await;
        guard.as_ref().and_then(|map| map.get(host).cloned())
    };
    if let Some(entry) = cached {
        if wait_healthy(std::path::Path::new(&entry.local)).await {
            return Ok(entry.local);
        }
        release_tunnel(host).await;
    }
    let entry = start_tunnel(&destination, remote_socket, auth).await?;
    let local = entry.local.clone();
    TUNNELS
        .lock()
        .await
        .get_or_insert_with(Default::default)
        .insert(host.to_string(), entry);
    Ok(local)
}

#[derive(Debug, Clone)]
pub enum RemoteProbe {
    Ready { remote_socket: String },
}

fn parse_probe_output(line: &str) -> Result<RemoteProbe, TunnelError> {
    let fields: Vec<_> = line.trim().split('\t').collect();
    match fields.as_slice() {
        ["READY", socket] => Ok(RemoteProbe::Ready {
            remote_socket: (*socket).into(),
        }),
        ["STOPPED", socket, binary] => Err(TunnelError::NeedsStart {
            remote_socket: (*socket).into(),
            binary: (*binary).into(),
        }),
        ["MISSING", socket] => Err(TunnelError::NeedsInstall {
            remote_socket: (*socket).into(),
        }),
        _ => Err(TunnelError::Other(format!(
            "invalid remote probe: {}",
            line.trim()
        ))),
    }
}

/// SSH 成功后区分 ready / missing / stopped，不再全部折叠为 offline。
pub async fn probe_remote(host: &str, auth: &SshAuth) -> Result<RemoteProbe, TunnelError> {
    let destination = auth.destination(host);
    let script = r#"data=${XDG_DATA_HOME:-$HOME/.local/share}/lmux
socket=$data/lmux.sock
lock=$data/lmux.lock
managed=$data/bin/lmux
running=0
if [ -S "$socket" ] && command -v flock >/dev/null 2>&1 && ! flock -n "$lock" -c true 2>/dev/null; then running=1; fi
if [ "$running" -eq 1 ]; then printf 'READY\t%s\n' "$socket"
elif command -v lmux >/dev/null 2>&1; then printf 'STOPPED\t%s\t%s\n' "$socket" "$(command -v lmux)"
elif [ -x "$managed" ]; then printf 'STOPPED\t%s\t%s\n' "$socket" "$managed"
else printf 'MISSING\t%s\n' "$socket"
fi"#;
    let output = ssh_command(auth)
        .arg(&destination)
        .arg(script)
        .output()
        .await
        .map_err(|error| TunnelError::Other(error.to_string()))?;
    if !output.status.success() {
        return Err(classify_failure(&output.stderr));
    }
    let line = String::from_utf8_lossy(&output.stdout);
    parse_probe_output(&line)
}

async fn upload_binary(host: &str, auth: &SshAuth) -> Result<(), TunnelError> {
    let destination = auth.destination(host);
    let binary = if let Some(path) = std::env::var_os("LMUX_BOOTSTRAP_BINARY") {
        PathBuf::from(path)
    } else {
        let current =
            std::env::current_exe().map_err(|error| TunnelError::Other(error.to_string()))?;
        let release = current
            .parent()
            .and_then(|debug| debug.parent())
            .map(|target| target.join("release/lmux"));
        release.filter(|path| path.is_file()).unwrap_or(current)
    };
    let bytes = tokio::fs::read(binary)
        .await
        .map_err(|error| TunnelError::Other(error.to_string()))?;
    let script = r#"set -eu
data=${XDG_DATA_HOME:-$HOME/.local/share}/lmux
mkdir -p "$data/bin" "$data/logs"
tmp=$data/bin/.lmux-upload-$$
cat > "$tmp"
chmod 700 "$tmp"
"$tmp" --version >/dev/null
mv "$tmp" "$data/bin/lmux""#;
    let mut child = ssh_command(auth)
        .arg(&destination)
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| TunnelError::Other(error.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| TunnelError::Other("SSH upload stdin unavailable".into()))?;
    stdin
        .write_all(&bytes)
        .await
        .map_err(|error| TunnelError::Other(error.to_string()))?;
    drop(stdin);
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| TunnelError::Other(error.to_string()))?;
    if !output.status.success() {
        return Err(classify_failure(&output.stderr));
    }
    Ok(())
}

pub async fn install_and_start(host: &str, auth: &SshAuth) -> Result<(), TunnelError> {
    upload_binary(host, auth).await?;
    start_remote(host, auth, None).await
}

pub async fn install_and_restart(host: &str, auth: &SshAuth) -> Result<(), TunnelError> {
    upload_binary(host, auth).await?;
    let destination = auth.destination(host);
    let command = r#"data=${XDG_DATA_HOME:-$HOME/.local/share}/lmux
pkill -TERM -f '[l]mux.*--headless' 2>/dev/null || true
for i in 1 2 3 4 5 6 7 8 9 10; do
  if flock -n "$data/lmux.lock" -c true 2>/dev/null; then break; fi
  sleep .2
done
rm -f -- "$data/lmux.sock""#;
    let output = ssh_command(auth)
        .arg(&destination)
        .arg(command)
        .output()
        .await
        .map_err(|error| TunnelError::Other(error.to_string()))?;
    if !output.status.success() {
        return Err(classify_failure(&output.stderr));
    }
    start_remote(host, auth, None).await
}

pub async fn start_remote(
    host: &str,
    auth: &SshAuth,
    binary: Option<&str>,
) -> Result<(), TunnelError> {
    let destination = auth.destination(host);
    let explicit = binary.unwrap_or("");
    let command = format!(
        r#"set -eu
data=${{XDG_DATA_HOME:-$HOME/.local/share}}/lmux
socket=$data/lmux.sock
lock=$data/lmux.lock
bin={}
if [ -z "$bin" ]; then bin=$data/bin/lmux; fi
mkdir -p "$data/logs"
running=0
if [ -S "$socket" ] && command -v flock >/dev/null 2>&1 && ! flock -n "$lock" -c true 2>/dev/null; then running=1; fi
if [ "$running" -eq 0 ]; then
  rm -f -- "$socket"
  nohup "$bin" --headless </dev/null >>"$data/logs/headless.log" 2>&1 &
fi
for i in 1 2 3 4 5 6 7 8 9 10; do
  if [ -S "$socket" ] && ! flock -n "$lock" -c true 2>/dev/null; then exit 0; fi
  sleep .2
done
printf 'lmux headless did not create %s\n' "$socket" >&2
exit 1"#,
        sh_quote(explicit)
    );
    let output = ssh_command(auth)
        .arg(&destination)
        .arg(command)
        .output()
        .await
        .map_err(|error| TunnelError::Other(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(classify_failure(&output.stderr))
    }
}

/// 启动一条隧道：
///   ssh -M -S <ctl> -fN -L <local_sock>:<remote_sock> <host>   (OpenSSH ≥8.3 支持 unix 转发)
/// 失败则回退：远端 socat 转 TCP + 本地 socat TCP→unix
async fn start_tunnel(
    host: &str,
    remote_socket: &str,
    auth: &SshAuth,
) -> Result<TunnelEntry, TunnelError> {
    let dir = data_dir();
    std::fs::create_dir_all(dir.join("ssh")).ok();
    let id = lmux_core::model::new_id("tunnel");
    let ctl = dir.join("ssh").join(format!("{id}.ctl"));
    let local_sock = dir.join("ssh").join(format!("{id}-fwd.sock"));
    let _ = std::fs::remove_file(&local_sock);

    // 方案 A：OpenSSH 原生 StreamLocalForward: -L local_socket:remote_socket。
    let out = ssh_command(auth)
        .args(["-S"])
        .arg(&ctl)
        .args([
            "-fN",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPersist=600",
            "-o",
            "StreamLocalBindUnlink=yes",
            "-o",
            "ConnectTimeout=5",
        ])
        .arg("-L")
        .arg(streamlocal_spec(&local_sock, remote_socket))
        .arg(host)
        .output()
        .await;

    if out.as_ref().is_ok_and(|o| o.status.success()) && wait_healthy(&local_sock).await {
        return Ok(TunnelEntry {
            local: local_sock.display().to_string(),
            control_path: ctl,
            destination: host.to_string(),
        });
    }
    match out {
        Ok(output) => Err(classify_failure(&output.stderr)),
        Err(error) => Err(TunnelError::Other(error.to_string())),
    }
}

async fn wait_healthy(socket: &std::path::Path) -> bool {
    for _ in 0..30 {
        if tokio::net::UnixStream::connect(socket).await.is_ok() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

async fn start_tunnel_socat(
    host: &str,
    remote_socket: &str,
    local_sock: &std::path::Path,
    ctl: &std::path::Path,
    auth: &SshAuth,
) -> Result<String, TunnelError> {
    for offset in 0..20u16 {
        let remote_port = 43_700 + offset;
        let local_port = 44_700 + offset;
        let remote_socket_q = sh_quote(remote_socket);
        let probe = ssh_command(auth)
            .args(["-o", "ConnectTimeout=5"])
            .arg(host)
            .arg(format!(
                "if ! ss -ltn | grep -q ':{remote_port} '; then (socat TCP-LISTEN:{remote_port},bind=127.0.0.1,fork UNIX-CONNECT:{remote_socket_q} >/dev/null 2>&1 &) ; fi; sleep 0.2; ss -ltn | grep -q ':{remote_port} ' && echo OK"
            ))
            .output()
            .await
            .map_err(|error| TunnelError::Other(error.to_string()))?;
        if !probe.status.success() || !String::from_utf8_lossy(&probe.stdout).contains("OK") {
            continue;
        }

        // 本地 TCP 端口通过 SSH 送到远端 socat listener。
        let ssh = ssh_command(auth)
            .arg("-S")
            .arg(ctl)
            .args([
                "-fN",
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPersist=600",
                "-o",
                "BatchMode=yes",
                "-L",
            ])
            .arg(format!("{local_port}:127.0.0.1:{remote_port}"))
            .arg(host)
            .output()
            .await
            .map_err(|error| TunnelError::Other(error.to_string()))?;
        if !ssh.status.success() {
            continue;
        }

        let local = local_sock.display().to_string();
        let _ = std::fs::remove_file(local_sock);
        Command::new("socat")
            .args([
                format!("UNIX-LISTEN:{local},fork"),
                format!("TCP:127.0.0.1:{local_port}"),
            ])
            .spawn()
            .map_err(|error| TunnelError::Other(error.to_string()))?;
        if wait_healthy(local_sock).await {
            return Ok(local);
        }
    }
    Err(TunnelError::Other("unable to establish SSH tunnel".into()))
}

fn sh_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn streamlocal_spec(local: &std::path::Path, remote: &str) -> String {
    format!("{}:{}", local.display(), remote)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_probe_distinguishes_install_and_start() {
        assert!(matches!(
            parse_probe_output("READY\t/run/user/1000/lmux.sock\n"),
            Ok(RemoteProbe::Ready { .. })
        ));
        assert!(matches!(
            parse_probe_output("MISSING\t/home/u/.local/share/lmux/lmux.sock\n"),
            Err(TunnelError::NeedsInstall { .. })
        ));
        assert!(matches!(
            parse_probe_output("STOPPED\t/tmp/lmux.sock\t/usr/bin/lmux\n"),
            Err(TunnelError::NeedsStart { .. })
        ));
    }

    #[test]
    fn askpass_script_never_contains_a_password() {
        let script = std::fs::read_to_string(askpass_script()).unwrap();
        assert!(script.contains("LMUX_SSH_SECRET_FILE"));
        assert!(!script.contains("super-secret"));
    }

    #[test]
    fn shell_quote_handles_apostrophes() {
        assert_eq!(sh_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
    }

    #[test]
    fn shell_quote_blocks_injection() {
        assert_eq!(sh_quote("/tmp/a b;rm"), "'/tmp/a b;rm'");
    }

    #[test]
    fn native_streamlocal_spec_is_local_colon_remote() {
        assert_eq!(
            streamlocal_spec(std::path::Path::new("/tmp/local.sock"), "/run/remote.sock"),
            "/tmp/local.sock:/run/remote.sock"
        );
    }
}
