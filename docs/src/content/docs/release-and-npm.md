---
title: Installation reference
description: Dependencies, release access, checksums, and updates.
order: 23
category: Reference
---

## Supported setup

The release supports **Apple Silicon macOS only**. Neither Intel CLI nor menu bar archives are produced. Google Chrome must be installed and the chosen profile signed into ChatGPT.

The installer downloads the paired Rust CLI and Swift app, verifies their checksum and signature, and invokes the embedded browser installer before replacing a working installation. The helper is pinned to `agent-browser-browse` 0.41.1 and verified by release checksum, architecture, signature, and reported version. It lives under `~/.config/slingring/browser`, separate from any globally installed agent-browser.

The companion extension is embedded in the Rust binary and loaded on demand. No manual unpacked-extension installation, Homebrew, npm, or separate upstream CLI is required.

## Release access

When `amxv/slingring` is public, the installer downloads release assets anonymously over HTTPS. **Until the repository becomes public**, `gh` must be authenticated with repository access to download its private release assets. The installer automatically uses an existing authenticated `gh` session when available; it never prints or copies GitHub credentials. A public website does not make private GitHub releases anonymously accessible.

```bash
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

For private access only, first run `gh auth status` and confirm the account can access `amxv/slingring`. This extra step is unnecessary for public releases.

The CLI and `Slingring.app` are installed beside each other in `~/.local/bin`. Set `CLI_INSTALL_DIR` to another absolute directory if required. `CLI_NO_LAUNCH=1` keeps the optional menu bar closed; `CLI_LAUNCH_MENU_BAR=0` disables automatic opening unless it was already running. Ad-hoc signing is not Developer ID notarization. No security settings are disabled and quarantine is not stripped.

Both menu bar and background service login startup are enabled on fresh installs, independently configurable from Configure or `slingring login ...`. `CLI_NO_LAUNCH` affects only the immediate opening during installation, **not** future macOS login startup. A macOS Login Items denial may require explicit approval in System Settings; the installer reports registration errors without removing a verified installation.

## Updates

```bash
slingring upgrade --check
slingring upgrade
slingring auto-update on
slingring auto-update status
slingring auto-update off
```

Updates refuse to replace an installation while a task is active. They update the CLI and app together, restart a previously running service, and reopen a previously running menu bar. Automatic updates are opt-in and use a daily OS scheduler, not a polling app.

The installer migrates legacy LaunchAgent presence as the service login preference (a missing old plist is treated as an opt-out), initializes the menu bar login item where possible, and preserves subsequent explicit opt-outs through updates. It never resets those choices during automatic updates.

## Source builds

```bash
git clone https://github.com/amxv/slingring.git
cd slingring
cargo install --locked --path .
slingring browser install
slingring profile list
slingring profile use "YOUR PROFILE"
```

While the repository remains private, cloning also requires authorized GitHub access. Source builds use the pinned Rust toolchain. Release workflows use cargo-dist and publish directly to GitHub Releases; there is no npm package or crates.io publication. The maintainer release procedure is in `.agents/skills/release/SKILL.md`.
