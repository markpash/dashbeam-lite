//! Release tasks: `cargo xtask bundle [--build N]` builds DashBeam Lite.app,
//! `cargo xtask dmg [--build N]` also makes the .dmg and the updater's .zip.
//!
//! The build number is baked into the app (`DASHBEAM_LITE_BUILD`) and is what the
//! updater compares; 0 means a local build that never updates itself. Signs ad
//! hoc unless `SIGNING_IDENTITY` is set.

use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T = ()> = std::result::Result<T, String>;

const NAME: &str = "DashBeam Lite";
/// Crate, binary and release asset name.
const SLUG: &str = "dashbeam-lite";
/// Must match `APP_ID` in app/src/main.rs.
const APP_ID: &str = "me.markpash.dashbeam-lite";

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result {
    let mut args = std::env::args().skip(1);
    let task = args.next().unwrap_or_default();
    let mut build = std::env::var("DASHBEAM_LITE_BUILD").unwrap_or_else(|_| "0".into());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--build" => build = args.next().ok_or("--build needs a number")?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    build
        .parse::<u64>()
        .map_err(|_| format!("invalid build number {build}"))?;

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    match task.as_str() {
        "bundle" => {
            bundle(&root, &build)?;
        }
        "dmg" => {
            let app = bundle(&root, &build)?;
            dmg(&root, &app, &build)?;
        }
        _ => return Err("usage: cargo xtask <bundle|dmg> [--build N]".into()),
    }
    Ok(())
}

fn bundle(root: &Path, build: &str) -> Result<PathBuf> {
    let release = root.join("target/release");
    sh(Command::new("cargo")
        .args(["build", "--release", "-p", SLUG])
        .env("DASHBEAM_LITE_BUILD", build)
        .current_dir(root))?;

    let app = release.join(format!("{NAME}.app"));
    let _ = std::fs::remove_dir_all(&app);
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).map_err(io)?;
    std::fs::create_dir_all(contents.join("Resources")).map_err(io)?;
    std::fs::copy(release.join(SLUG), contents.join("MacOS").join(SLUG)).map_err(io)?;
    std::fs::copy(
        root.join("app/assets/icon.icns"),
        contents.join("Resources/AppIcon.icns"),
    )
    .map_err(io)?;
    std::fs::write(
        contents.join("Info.plist"),
        info_plist(&version(root)?, build),
    )
    .map_err(io)?;

    let identity = std::env::var("SIGNING_IDENTITY").unwrap_or_else(|_| "-".into());
    sh(Command::new("codesign")
        .args([
            "--force",
            "--deep",
            "--options",
            "runtime",
            "--sign",
            &identity,
        ])
        .arg(&app))?;
    println!("Built {}", app.display());
    Ok(app)
}

/// Writes `dashbeam-lite-<build>.dmg` for people and `dashbeam-lite-<build>.zip` for the
/// in-app updater next to the bundle.
fn dmg(root: &Path, app: &Path, build: &str) -> Result {
    let release = root.join("target/release");
    let staging = release.join("dmg");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(io)?;
    sh(Command::new("ditto")
        .arg(app)
        .arg(staging.join(format!("{NAME}.app"))))?;
    std::os::unix::fs::symlink("/Applications", staging.join("Applications")).map_err(io)?;

    let dmg = release.join(format!("{SLUG}-{build}.dmg"));
    sh(Command::new("hdiutil")
        .args([
            "create",
            "-volname",
            NAME,
            "-format",
            "UDZO",
            "-ov",
            "-srcfolder",
        ])
        .arg(&staging)
        .arg(&dmg))?;
    std::fs::remove_dir_all(&staging).map_err(io)?;

    let zip = release.join(format!("{SLUG}-{build}.zip"));
    sh(Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(app)
        .arg(&zip))?;
    println!("Built {}\nBuilt {}", dmg.display(), zip.display());
    Ok(())
}

fn version(root: &Path) -> Result<String> {
    let manifest = std::fs::read_to_string(root.join("app/Cargo.toml")).map_err(io)?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_string)
        .ok_or_else(|| "no version in app/Cargo.toml".into())
}

fn info_plist(version: &str, build: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{NAME}</string>
    <key>CFBundleDisplayName</key><string>{NAME}</string>
    <key>CFBundleIdentifier</key><string>{APP_ID}</string>
    <key>CFBundleExecutable</key><string>{SLUG}</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>{build}</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSLocalNetworkUsageDescription</key><string>{NAME} finds nearby devices on your local network.</string>
    <key>NSBonjourServices</key><array><string>_irohv1._udp</string></array>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>File or folder to share</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>LSItemContentTypes</key><array><string>public.item</string><string>public.folder</string></array>
        </dict>
    </array>
</dict>
</plist>
"#
    )
}

fn sh(command: &mut Command) -> Result {
    let status = command.status().map_err(io)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed with {status}"))
    }
}

fn io(error: std::io::Error) -> String {
    error.to_string()
}
