//! Persistent Iroh node identity (`SecretKey`).
//!
//! Storage priority:
//! 1. `IROH_SECRET` env var (explicit override; not written to disk)
//! 2. `identity.key` in the app data directory (32 raw bytes)

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use data_encoding::HEXLOWER;
use iroh::SecretKey;

pub const IDENTITY_KEY_FILE: &str = "identity.key";
const SECRET_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSource {
    EnvVar,
    IdentityFile,
    Generated,
}

#[derive(Debug)]
pub struct SecretLoadOutcome {
    pub secret: SecretKey,
    pub source: SecretSource,
}

pub fn identity_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join(IDENTITY_KEY_FILE)
}

pub fn load_or_create_secret(data_dir: &Path) -> Result<SecretLoadOutcome> {
    std::fs::create_dir_all(data_dir).with_context(|| format!("create {}", data_dir.display()))?;

    if let Ok(hex) = std::env::var("IROH_SECRET") {
        let secret = parse_secret_hex(&hex)?;

        return Ok(SecretLoadOutcome {
            secret,
            source: SecretSource::EnvVar,
        });
    }

    let file_path = identity_key_path(data_dir);
    if let Some(secret) = read_identity_file(&file_path)? {
        return Ok(SecretLoadOutcome {
            secret,
            source: SecretSource::IdentityFile,
        });
    }
    let secret = SecretKey::generate();
    write_identity_file(&file_path, &secret)?;
    Ok(SecretLoadOutcome {
        secret,
        source: SecretSource::Generated,
    })
}

fn read_identity_file(path: &Path) -> Result<Option<SecretKey>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    match parse_secret_bytes(&bytes) {
        Ok(secret) => Ok(Some(secret)),
        Err(_err) => Ok(None),
    }
}

fn write_identity_file(path: &Path, secret: &SecretKey) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("key.tmp");
    std::fs::write(&tmp, secret.to_bytes()).with_context(|| format!("write {}", tmp.display()))?;
    restrict_permissions(&tmp)?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename {}", path.display()))?;
    restrict_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn parse_secret_bytes(bytes: &[u8]) -> Result<SecretKey> {
    anyhow::ensure!(
        bytes.len() == SECRET_LEN,
        "identity key must be {} bytes, got {}",
        SECRET_LEN,
        bytes.len()
    );
    let mut arr = [0u8; SECRET_LEN];
    arr.copy_from_slice(bytes);
    Ok(SecretKey::from_bytes(&arr))
}

fn parse_secret_hex(hex: &str) -> Result<SecretKey> {
    let bytes = HEXLOWER
        .decode(hex.trim().as_bytes())
        .context("invalid secret hex")?;
    parse_secret_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn identity_file_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let secret = SecretKey::generate();
        let path = identity_key_path(dir.path());
        write_identity_file(&path, &secret).expect("write");
        let loaded = read_identity_file(&path).expect("read").expect("some");
        assert_eq!(secret.to_bytes(), loaded.to_bytes());
    }

    #[test]
    fn generated_secret_is_stable_across_restarts() {
        let dir = TempDir::new().expect("tempdir");
        let first = load_or_create_secret(dir.path()).expect("first load");
        assert_eq!(first.source, SecretSource::Generated);
        let second = load_or_create_secret(dir.path()).expect("second load");
        assert_eq!(second.source, SecretSource::IdentityFile);
        assert_eq!(first.secret.to_bytes(), second.secret.to_bytes());
    }
}
