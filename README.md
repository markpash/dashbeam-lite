# DashBeam Lite

A lightweight, opinionated fork of [DashBeam](https://github.com/tonyantony300/dashbeam): peer-to-peer file transfer for macOS, rewritten as a native [gpui-kit](https://gpui-kit.com) app.

DashBeam sends files and folders directly between devices, end-to-end encrypted, with no cloud storage, accounts or tracking. It's built on [iroh](https://www.iroh.computer), which handles NAT hole punching and falls back to encrypted relays when a direct connection isn't possible.

This fork keeps DashBeam's transfer engine and protocol, so it works with the regular DashBeam apps (desktop, Android, CLI). Everything around the engine has been replaced or removed.

## What's different

- **Native UI.** The Tauri shell and React frontend are gone. The UI is Rust on GPUI via gpui-kit, with no web view.
- **macOS only.** No Windows, Linux, Android or web builds.
- **Less of everything.** English only. Light and dark follow the system. One window with five tabs: Send, Receive, Devices, History, Settings.
- **No back-compat.** Settings and storage were simplified without preserving the original app's formats.
- **Built with AI agents.** The port was done largely by AI coding agents ([Claude Code](https://claude.com/claude-code)), with a human steering and reviewing.

## What it does

- **Send** files or folders by dropping them on the window or the Dock icon, choosing them, or using **Open With**, then copy the ticket.
- **Receive** by pasting a ticket. Receive links from other DashBeam apps work too.
- **Many at once**: any number of sends and downloads run in parallel, each with its own progress, and the tabs and menu bar show what's in flight.
- **Nearby**: find DashBeam devices on the local network and send to them directly. First contact shows a verification code on both screens.
- **Paired devices**: pair once with a code (works over the internet) or through Nearby, then send without tickets. Turn on **Auto-accept** for devices you trust.
- **History**: a local log of sent and received transfers.
- **Menu bar**: closing the window keeps DashBeam Lite running in the menu bar. It can also start at login.
- **Self-hosted relays and discovery**, configured in Settings.

Left out on purpose: QR codes, broadcast mode, themes, translations, debug-bundle export and thumbnails.

## Install

Every green build on `main` is published as a [release](https://github.com/markpash/dashbeam-lite/releases) named `build-<n>`, where n is the number of commits on `main`. Download the `.dmg` and drag DashBeam Lite to Applications. From then on the app checks for new builds every few hours and offers to install them (Settings also has **Check for updates**).

Builds are Apple Silicon only and signed ad hoc rather than notarized, so the first launch of a downloaded copy is blocked by Gatekeeper. Right-click the app and choose **Open**, or run `xattr -dr com.apple.quarantine "/Applications/DashBeam Lite.app"`. Updates installed by the app itself aren't affected.

## Build

Requires macOS 13+ and a stable Rust toolchain (pinned in `rust-toolchain.toml`).

```sh
cargo xtask bundle            # target/release/DashBeam Lite.app
cargo xtask dmg --build 42    # also dashbeam-lite-42.dmg and the updater's dashbeam-lite-42.zip
open "target/release/DashBeam Lite.app"
```

Local builds have build number 0 and never update themselves. Set `SIGNING_IDENTITY` to sign with a real certificate instead of ad hoc. `cargo run` works for development, but system notifications only work from the app bundle. You can start sharing from a terminal with `"DashBeam Lite.app/Contents/MacOS/dashbeam-lite" <files…>`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development setup, and [docs/troubleshooting.md](docs/troubleshooting.md) if something doesn't connect.

## Layout

| Path | What it is |
|------|------------|
| `app/` | The macOS app: gpui-kit UI, menu bar icon, settings, glue to the engine |
| `engine/` | DashBeam's P2P transfer engine (`protocol` and `native` crates) |
| `infra/` | Configs for self-hosting a relay and discovery server |
| `xtask/` | `cargo xtask bundle` / `dmg`: app bundle, dmg and updater zip |

## Credits

All the networking, pairing and transfer work comes from [DashBeam](https://github.com/tonyantony300/dashbeam) by Tony Antony and its contributors, built on [iroh](https://www.iroh.computer) by n0. The UI is built with [gpui-kit](https://gpui-kit.com) and [gpui-tray](https://github.com/domenkozar/gpui-tray). If you want the full-featured, cross-platform app, use upstream DashBeam.

## License

[AGPL-3.0](LICENSE), same as upstream. See also the [privacy policy](PRIVACY.md).
