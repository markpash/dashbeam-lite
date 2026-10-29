//! Launch at login through a per-user LaunchAgent. The plist's presence is the
//! source of truth, so removing it outside the app is reflected here.

use std::path::PathBuf;

const LABEL: &str = "com.dashbeam.desktop";

fn plist_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(format!("Library/LaunchAgents/{LABEL}.plist")))
}

pub fn is_enabled() -> bool {
    plist_path().is_some_and(|path| path.exists())
}

pub fn set(enabled: bool) -> std::io::Result<()> {
    let path = plist_path().ok_or_else(|| std::io::Error::other("no home directory"))?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    let exe = std::env::current_exe()?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array><string>{}</string><string>--hidden</string></array>
    <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        exe.display()
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, plist)
}
