---
title: Installation reference
description: Dependencies, release access, checksums, and updates.
order: 23
category: Reference
---

## Supported setup

The release supports **Apple Silicon macOS** and **x86_64 Linux with glibc 2.39 or newer**, such as Ubuntu 24.04. There are no Intel macOS, Linux ARM64, Windows or Linux menu/tray archives. Google Chrome must be installed and the chosen profile signed into ChatGPT. Linux uses system-owned Google Chrome Stable at `/opt/google/chrome/chrome`, not an automatically downloaded browser fallback. It refuses root, CI and container environments where the helper would disable Chrome's sandbox; it never changes system policy to make a browser start.

On macOS, the installer downloads the paired Rust CLI and Swift app, verifies their checksum and signature, and invokes the embedded browser installer before replacing a working installation. The macOS helper remains pinned to `agent-browser-browse` 0.41.1 and verified by release checksum, architecture, signature, and reported version. Linux installs only the checksummed x86_64 CLI; its helper is separately pinned to 0.43.0 with fixed archive and native-binary SHA-256 digests, source/patch provenance, ELF architecture and version checks. Both live under `~/.config/slingring/browser`, separate from any globally installed agent-browser.

The companion extension is embedded in the Rust binary and loaded on demand. No manual unpacked-extension installation, Homebrew, npm, or separate upstream CLI is required.

## Release access

The installer downloads public GitHub release assets anonymously over HTTPS. You do not need a GitHub account or an authenticated `gh` session.

```bash
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

The CLI is installed in `~/.local/bin`, beside `Slingring.app` on macOS. Set `CLI_INSTALL_DIR` to another absolute directory if required. On macOS, `CLI_NO_LAUNCH=1` keeps the optional menu bar closed; `CLI_LAUNCH_MENU_BAR=0` disables automatic opening unless it was already running. Ad-hoc signing is not Developer ID notarization. No security settings are disabled and quarantine is not stripped.

On macOS, both menu bar and background service login startup are enabled on fresh installs, independently configurable from Configure or `slingring login ...`. `CLI_NO_LAUNCH` affects only the immediate opening during installation, **not** future macOS login startup. A macOS Login Items denial may require explicit approval in System Settings; the installer reports registration errors without removing a verified installation.

Linux installation never registers or starts a service. Optional `slingring service install` registers a systemd user unit for the default state directory. `slingring login service on|off` changes future login startup without stopping active work. `slingring service start|stop` controls the current process. Alternate `SLINGRING_HOME` instances use direct private daemons and never operate the default user unit. No root access, lingering, system service or desktop autostart entry is configured.

## Updates

```bash
slingring upgrade --check
slingring upgrade
```

Linux upgrades require `python3` to verify exact idle status before replacement. They preserve the helper's platform pin and resume a previously running service without changing login preferences. Avoid submitting new work while upgrading. Unknown or busy status refuses replacement; interrupted submissions are never replayed. Linux has no supported automatic update scheduler in this release.

On macOS, automatic updates remain available:

```bash
slingring auto-update on
slingring auto-update status
slingring auto-update off
```

macOS updates refuse to replace an installation while a task is active. They update the CLI and app together, restart a previously running service, and reopen a previously running menu bar. Automatic updates are opt-in and use a daily OS scheduler, not a polling app.

The installer migrates legacy LaunchAgent presence as the service login preference (a missing old plist is treated as an opt-out), initializes the menu bar login item where possible, and preserves subsequent explicit opt-outs through updates. It never resets those choices during automatic updates.

### Optional local signing identity

Public release bundles are ad-hoc signed. Their code hash changes with each build, so macOS can invalidate previous privacy consent after an update even though the signing identifier is unchanged. Starting in 0.6.2, the temporary task browser no longer needs App Management access for Chrome's app-cloning operation. Slingring's own installation and upgrade are separate app-modification operations.

If you maintain a dedicated certificate-backed code-signing identity in your Mac's Keychain, you can explicitly opt into signing the verified CLI and menu bar app locally. Download and inspect the installer, then supply the certificate's full 40-character SHA-1 fingerprint:

```bash
CLI_CODESIGN_IDENTITY=YOUR_CERTIFICATE_SHA1 sh install.sh upgrade
```

Later upgrades discover the installed certificate and preserve the CLI and app's designated requirements automatically. If the matching private key is unavailable, signing fails, or either requirement changes, the installer stops before replacing the working installation. It never silently falls back to ad-hoc signing. The private key stays on your Mac; this does not sign public GitHub artifacts, create a Developer ID identity, or notarize the app.

The installer does not create certificates, change Keychain trust or key-access permissions, or alter macOS privacy settings. Keychain may require your approval to use the selected key. Switching from ad-hoc to certificate-backed signing is a one-time identity change, so macOS may require fresh consent for any permissions you actually use. Permission retention is controlled by macOS and must be verified on your installation.

## Source builds

```bash
git clone https://github.com/amxv/slingring.git
cd slingring
cargo install --locked --path .
slingring browser install
slingring profile list
slingring profile use "YOUR PROFILE"
```

Source builds use the pinned Rust toolchain. Release workflows use cargo-dist and publish directly to GitHub Releases; there is no npm package or crates.io publication. The maintainer release procedure is in `.agents/skills/release/SKILL.md`.
