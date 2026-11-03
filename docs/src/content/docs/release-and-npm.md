---
title: Installation reference
description: Dependencies, private releases, checksums, and updates.
order: 23
category: Reference
---

## Supported setup

The complete one-command installation currently supports Apple Silicon macOS. Intel CLI archives may exist, but the published browsing fork is ARM64-only; do not assume an Intel archive provides a working browser dependency. Google Chrome must be installed and the chosen profile signed into ChatGPT.

The installer downloads the paired Rust CLI and Swift app, verifies their checksum and signature, and invokes the embedded browser installer before replacing a working installation. The helper is pinned to `agent-browser-browse` 0.41.1 and verified by release checksum, architecture, signature, and reported version. It lives under `~/.config/slingring/browser`, separate from any globally installed agent-browser.

The companion extension is embedded in the Rust binary and loaded on demand. No manual unpacked-extension installation, Homebrew, npm, or separate upstream CLI is required.

## Private release access

While `amxv/slingring` is private, `gh` must be authenticated with repository access. The installer uses that existing login to download private assets. It does not print or copy GitHub credentials. Public anonymous GitHub download URLs cannot access a private release.

```bash
gh auth status
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

The CLI and `Slingring.app` are installed beside each other in `~/.local/bin`. Set `CLI_INSTALL_DIR` to another absolute directory if required. `CLI_NO_LAUNCH=1` keeps the optional menu bar closed; `CLI_LAUNCH_MENU_BAR=0` disables automatic opening unless it was already running. Ad-hoc signing is not Developer ID notarization. No security settings are disabled and quarantine is not stripped.

## Updates

```bash
slingring upgrade --check
slingring upgrade
slingring auto-update on
slingring auto-update status
slingring auto-update off
```

Updates refuse to replace an installation while a task is active. They update the CLI and app together, restart a previously running service, and reopen a previously running menu bar. Automatic updates are opt-in and use a daily OS scheduler, not a polling app.

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
