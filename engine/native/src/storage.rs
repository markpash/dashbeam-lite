//! Native filesystem blob store creation.

use crate::types::AutoCleanupDir;
use anyhow::Context;
use data_encoding::HEXLOWER;
use iroh_blobs::store::fs::FsStore;
use rand::RngExt;
use std::path::{Path, PathBuf};

/// Directory name prefix of every send-side blob store.
pub const SEND_DIR_PREFIX: &str = ".dashbeam-send-";
/// Directory name prefix of every partial-receive blob store.
pub const RECV_DIR_PREFIX: &str = ".dashbeam-recv-";

pub fn new_send_blobs_dir() -> PathBuf {
    let suffix = rand::rng().random::<[u8; 16]>();
    temp_dir().join(format!("{SEND_DIR_PREFIX}{}", HEXLOWER.encode(&suffix)))
}

pub async fn create_send_store(dir: &Path) -> anyhow::Result<FsStore> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("failed to create send store dir {}", dir.display()))?;
    FsStore::load(dir)
        .await
        .with_context(|| format!("failed to load send store at {}", dir.display()))
}

pub async fn create_recv_store(hash_hex: &str) -> anyhow::Result<(FsStore, PathBuf)> {
    let dir_name = format!("{RECV_DIR_PREFIX}{hash_hex}");
    let path = temp_dir().join(dir_name);
    let store = FsStore::load(&path)
        .await
        .with_context(|| format!("failed to load recv store at {}", path.display()))?;
    Ok((store, path))
}

pub fn recv_cleanup_guard(path: PathBuf) -> AutoCleanupDir {
    AutoCleanupDir::new(path)
}

/// Root for every blob store this crate creates.
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}
