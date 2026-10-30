//! Self-update from GitHub releases. CI publishes every green build on main as
//! release `build-<n>` with a zipped DashBeam.app; the app swaps its own bundle
//! for a newer one and relaunches.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build number baked in by `cargo xtask`; 0 for local builds, which never
/// update themselves.
pub const BUILD: u64 = match option_env!("DASHBEAM_BUILD") {
    Some(build) => parse(build),
    None => 0,
};

const LATEST: &str = "https://api.github.com/repos/markpash/dashbeam-lite/releases/latest";

const fn parse(digits: &str) -> u64 {
    let bytes = digits.as_bytes();
    let mut value = 0;
    let mut i = 0;
    while i < bytes.len() {
        value = value * 10 + (bytes[i] - b'0') as u64;
        i += 1;
    }
    value
}

#[derive(Clone)]
pub struct Release {
    pub build: u64,
    pub zip_url: String,
    pub page_url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// The bundle this process runs from, if it runs from one.
pub fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let bundle = exe.ancestors().nth(3)?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// Whether this copy can update itself at all.
pub fn enabled() -> bool {
    BUILD > 0 && current_bundle().is_some()
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("DashBeam/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// The latest release, when it is newer than this build.
pub async fn check() -> Result<Option<Release>, String> {
    // Overridable to test updates against a local server.
    let url = std::env::var("DASHBEAM_UPDATE_URL").unwrap_or_else(|_| LATEST.into());
    let response = client()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    // GitHub answers 404 until the first release exists.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status().map_err(|e| e.to_string())?;
    let body = response.bytes().await.map_err(|e| e.to_string())?;
    let release: ApiRelease = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let Some(build) = release
        .tag_name
        .strip_prefix("build-")
        .and_then(|n| n.parse::<u64>().ok())
    else {
        return Ok(None);
    };
    let zip = release
        .assets
        .into_iter()
        .find(|a| a.name.ends_with(".zip"));
    Ok(zip.filter(|_| build > BUILD).map(|zip| Release {
        build,
        zip_url: zip.browser_download_url,
        page_url: release.html_url,
    }))
}

/// Downloads `release` and swaps it in for the running bundle. Returns the
/// bundle path to relaunch.
pub async fn install(release: Release) -> Result<PathBuf, String> {
    let bundle = current_bundle().ok_or("not running from an app bundle")?;
    let zip = client()?
        .get(&release.zip_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || swap(&bundle, &zip).map(|()| bundle))
        .await
        .map_err(|e| e.to_string())?
}

/// Unpacks next to the bundle (same volume, so the swap is two renames) and
/// replaces it, restoring the old one if the second rename fails.
fn swap(bundle: &Path, zip: &[u8]) -> Result<(), String> {
    let parent = bundle.parent().ok_or("bundle has no parent folder")?;
    let staging = parent.join(".DashBeam-update");
    let old = parent.join(".DashBeam-old.app");
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", parent.display()))?;

    let result = (|| {
        let archive = staging.join("update.zip");
        std::fs::write(&archive, zip).map_err(|e| e.to_string())?;
        let status = Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&archive)
            .arg(&staging)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("could not unpack the update".to_string());
        }
        let new = staging.join("DashBeam.app");
        if !new.join("Contents/MacOS/DashBeam").exists() {
            return Err("the update does not contain DashBeam.app".into());
        }
        std::fs::rename(bundle, &old).map_err(|e| e.to_string())?;
        if let Err(error) = std::fs::rename(&new, bundle) {
            let _ = std::fs::rename(&old, bundle);
            return Err(error.to_string());
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&old);
    result
}

/// Opens `bundle` once this process has exited.
pub fn relaunch(bundle: &Path) {
    let spawned = Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; /usr/bin/open "$2""#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(bundle)
        .spawn();
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not schedule relaunch");
    }
}
