use engine::{Discoverability, DiscoveryConfigArg, RelayConfigArg};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Everything the user can change that isn't owned by the engine (device
/// name, paired devices and trust live in the engine's own stores).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub download_dir: Option<PathBuf>,
    pub discoverability: Discoverability,
    /// `None` uses the public n0 relays.
    pub relay: Option<RelayConfigArg>,
    /// `None` uses n0 discovery.
    pub discovery: Option<DiscoveryConfigArg>,
}

impl Settings {
    pub fn load(data_dir: &Path) -> Self {
        std::fs::read(data_dir.join("settings.json"))
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) {
        let path = data_dir.join("settings.json");
        let result = serde_json::to_vec_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|raw| std::fs::write(&path, raw));
        if let Err(error) = result {
            tracing::warn!(%error, "failed to save settings");
        }
    }

    pub fn download_dir(&self) -> PathBuf {
        self.download_dir
            .clone()
            .or_else(dirs::download_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
