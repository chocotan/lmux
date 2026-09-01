//! HMAC-SHA256 token（hook / 配对后的 RPC）。格式：v1:<expiry_unix>:<base64url(mac)>。
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::path::Path;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct AuthSecret(pub Vec<u8>);

impl AuthSecret {
    pub fn generate() -> Self {
        let bytes: [u8; 32] = rand::random();
        Self(bytes.to_vec())
    }

    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            let bytes = std::fs::read(path)?;
            if bytes.len() >= 32 {
                force_private(path)?;
                return Ok(Self(bytes));
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = Self::generate();
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;
            file.write_all(&s.0)?;
            file.sync_all()?;
        }
        #[cfg(not(unix))]
        std::fs::write(path, &s.0)?;
        force_private(path)?;
        Ok(s)
    }

    pub fn token(&self, subject: &str, ttl_secs: u64) -> String {
        let expiry = crate::model::now_secs().saturating_add(ttl_secs);
        self.token_at(subject, expiry)
    }

    pub fn token_at(&self, subject: &str, expiry: u64) -> String {
        let msg = format!("{subject}\n{expiry}");
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts any key");
        mac.update(msg.as_bytes());
        let sig =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        format!("v1:{expiry}:{sig}")
    }

    pub fn verify(&self, subject: &str, token: &str) -> bool {
        let mut parts = token.split(':');
        if parts.next() != Some("v1") {
            return false;
        }
        let Some(expiry) = parts.next().and_then(|v| v.parse::<u64>().ok()) else {
            return false;
        };
        let Some(sig) = parts.next() else {
            return false;
        };
        if parts.next().is_some() || expiry < crate::model::now_secs() {
            return false;
        }
        let Ok(sig) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sig) else {
            return false;
        };
        let msg = format!("{subject}\n{expiry}");
        let Ok(mut mac) = HmacSha256::new_from_slice(&self.0) else {
            return false;
        };
        mac.update(msg.as_bytes());
        mac.verify_slice(&sig).is_ok() // constant-time verify
    }
}

fn force_private(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_roundtrip_and_tamper() {
        let s = AuthSecret(vec![7; 32]);
        let t = s.token("agent_1", 60);
        assert!(s.verify("agent_1", &t));
        assert!(!s.verify("agent_2", &t));
        let tampered = format!("{}x", t);
        assert!(!s.verify("agent_1", &tampered));
    }
    #[test]
    fn expired_rejected() {
        let s = AuthSecret(vec![9; 32]);
        let t = s.token_at("a", crate::model::now_secs().saturating_sub(1));
        assert!(!s.verify("a", &t));
    }
    #[test]
    fn existing_secret_permissions_are_repaired() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secret");
        std::fs::write(&p, [3u8; 32]).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        AuthSecret::load_or_create(&p).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn secret_file_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secret");
        let a = AuthSecret::load_or_create(&p).unwrap();
        let b = AuthSecret::load_or_create(&p).unwrap();
        assert_eq!(a.0, b.0);
    }
}
