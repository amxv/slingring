---
title: Installation reference
description: Dependencies, private releases, checksums, and updates.
order: 23
category: Reference
---

## Supported setup

The release pipeline has macOS ARM64, Linux x86_64 and Windows x64 targets. The portable CLI includes the same extension and queue protocol; Linux uses an optional systemd user unit and Windows uses per-user startup registration. The Swift menu bar app is **macOS-only**. The browser helper must first ship a matching platform asset; adding target definitions to a PR does not make an unreleased installer usable. Intel macOS CLI archives may exist but are not a supported full installation yet. Google Chrome must be installed and the chosen profile signed into ChatGPT.

The installer downloads the Rust CLI (and Swift app on macOS), verifies checksums and architecture where applicable, then installs the embedded browser helper before replacing an existing CLI. The helper is pinned to `agent-browser-browse` 0.41.1 and verified by release SHA-256 and reported version. The macOS build additionally verifies code signature. It lives under `~/.config/slingring/browser` on macOS/Linux or `%APPDATA%\slingring\browser` on Windows, separate from any global agent-browser installation.

The companion extension is embedded in the Rust binary and loaded on demand. No manual unpacked-extension installation, Homebrew, npm, or separate upstream CLI is required.

## Private release access

While `amxv/slingring` is private, `gh` must be authenticated with repository access. The installer uses that existing login to download private assets. It does not print or copy GitHub credentials. Public anonymous GitHub download URLs cannot access a private release.

```bash
gh auth status
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

On macOS the CLI and `Slingring.app` are installed beside each other in `~/.local/bin`. On Linux only the CLI is installed there. Set `CLI_INSTALL_DIR` to another absolute directory if required. On Windows use `install.ps1`; the CLI installs under `%LOCALAPPDATA%\Programs\Slingring` with no tray UI. `CLI_NO_LAUNCH=1` keeps the macOS menu bar closed; `CLI_LAUNCH_MENU_BAR=0` disables automatic opening unless it was already running. Ad-hoc signing is not Developer ID notarization. No security settings are disabled and quarantine is not stripped.

## Updates

```bash
slingring upgrade --check
slingring upgrade
slingring auto-update on
slingring auto-update status
slingring auto-update off
```

Updates refuse to replace an installation while a task is active. On macOS they update the CLI and app together and reopen a previously running menu bar. Linux and Windows update the CLI only. Automatic updates are opt-in and use a daily OS scheduler, not a polling app.

## Source builds

```bash
gh repo clone amxv/slingring
cd slingring
cargo install --locked --path .
slingring browser install
slingring profile list
slingring profile use "YOUR PROFILE"
```

Source builds use the pinned Rust toolchain. Release workflows use cargo-dist; there is no npm package or crates.io publication. The release procedure is in `.agents/skills/release/SKILL.md`.

## Platform test limitations

Cross-platform compilation and offline browser smoke tests do not verify authenticated ChatGPT task starts. Linux Chrome 152 passed basic fingerprint checks, but the dedicated Cloudflare challenge remained and G2 did not yield usable content. Windows Chrome native-messaging registration uses a transient HKCU registry entry, unlike the per-profile Unix manifests. Test native messaging, copied profile login, extension startup and complete task lifecycle on each target OS before claiming feature parity; detector pages are not a guarantee of successful access to protected sites.
