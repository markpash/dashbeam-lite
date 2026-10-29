#!/usr/bin/env bash
# Builds target/release/DashBeam.app. Signs ad hoc unless SIGNING_IDENTITY is
# set (e.g. "Developer ID Application: …"). System notifications only work
# from inside a bundle, so run the app from here rather than `cargo run` when
# testing invites.
set -euo pipefail

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)
app=target/release/DashBeam.app

cargo build --release -p dashbeam

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/dashbeam "$app/Contents/MacOS/DashBeam"
cp app/assets/icon.icns "$app/Contents/Resources/DashBeam.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>DashBeam</string>
    <key>CFBundleDisplayName</key><string>DashBeam</string>
    <key>CFBundleIdentifier</key><string>com.dashbeam.desktop</string>
    <key>CFBundleExecutable</key><string>DashBeam</string>
    <key>CFBundleIconFile</key><string>DashBeam</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSLocalNetworkUsageDescription</key><string>DashBeam finds nearby devices on your local network.</string>
    <key>NSBonjourServices</key><array><string>_irohv1._udp</string></array>
</dict>
</plist>
EOF

codesign --force --deep --options runtime --sign "${SIGNING_IDENTITY:--}" "$app"
echo "Built $app ($(du -sh "$app" | cut -f1))"
