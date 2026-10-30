# Contributing to DashBeam Lite

Thank you for your interest in contributing. This guide covers local setup and how to submit changes.

## Development setup

**Prerequisites:** macOS 13+, Xcode command line tools, and a stable Rust toolchain (pinned in `rust-toolchain.toml`). With Nix, `nix develop` (or direnv) provides the toolchain.

```bash
cargo run                      # debug build of the app
cargo xtask bundle             # release "DashBeam Lite.app" (needed for system notifications)
cargo xtask dmg --build N      # also the .dmg and updater .zip, as CI publishes them
```

Useful while developing:

- `RUST_LOG=debug cargo run` turns up logging (it goes to stderr).
- `HOME=/tmp/peer-b cargo run` starts a second, fully separate instance (its own identity, settings and Downloads folder), so you can test sending, pairing and Nearby on one Mac.
- `cargo run -- <files…>` starts sharing those files at launch.
- To test the updater, run a numbered build from a bundle with `DASHBEAM_LITE_UPDATE_URL` pointing at a JSON file shaped like GitHub's `releases/latest` response.

### Project layout

| Path | Purpose |
|------|---------|
| `app/` | The macOS app: gpui-kit UI, tray, settings, glue to the engine |
| `engine/` | Rust P2P transfer engine (`protocol` + `native` crates) |
| `infra/` | Self-hosted relay and discovery server configs |
| `xtask/` | `cargo xtask bundle` / `dmg`: app bundle, dmg and updater zip |

## Testing

```bash
cargo test -p sendme-protocol -p sendme-native   # unit tests
cargo test -p engine -- --test-threads=1          # engine end-to-end tests
cargo test -p engine --features nearby-tests -- --test-threads=1   # plus Nearby E2E (needs a LAN-friendly network)
```

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/). Format:

```text
type(scope): short description
```

Examples:

```text
fix(ui): wrap long invite text
feat(discovery): optional DNS origin support
chore(nix): add flake and .envrc
docs: update contributing guide
```

Common types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`. Scope is optional (`android`, `tls`, `ui`, …).

**PR titles use the same format.** PRs are squash-merged using the PR title as the commit on `main`, so the title is what lands in history and release notes.

## Pull requests

1. Search [existing issues](https://github.com/tonyantony300/dashbeam/issues) before opening a new one.
2. For bugs, use the [bug report template](.github/ISSUE_TEMPLATE/report-bug.md).
3. Use a conventional PR title, for example `fix(ui): wrap long invite text`.
4. Run checks before opening a PR:

```bash
cargo fmt --all
cargo clippy --workspace
```

1. Fill out the [pull request template](.github/PULL_REQUEST_TEMPLATE.md).
**Lockfiles:** Do not commit lockfile-only changes (`git checkout origin/main -- Cargo.lock` to drop them).

## Getting help

- [Discord](https://discord.gg/xwb7z22Eve)
- [GitHub Issues](https://github.com/tonyantony300/dashbeam/issues)

## Code of conduct

Participation is governed by our [Code of Conduct](CODE_OF_CONDUCT.md). By
contributing, you agree to uphold it.

Security vulnerabilities should be reported privately — see [SECURITY.md](SECURITY.md).
