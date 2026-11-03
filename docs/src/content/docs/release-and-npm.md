---
title: Installation and updates
description: Direct macOS distribution, with an optional native menu bar.
order: 10
category: Distribution
summary: Private GitHub downloads, verified archives, and explicit update preferences.
---

Slingring uses direct GitHub Releases, not npm. Apple Silicon and Intel macOS archives include the Rust executable; paired bundles also include the Swift menu bar app. Their checksums and ad-hoc signatures are verified by the website installer. Ad-hoc signing is not Developer ID notarization, and no macOS security settings are disabled.

## Private repository access

While the repository is private, authenticate `gh` with access to `amxv/slingring`. The installer and upgrade check use that existing login for private release metadata and assets. Tokens are not copied into the command line or source tree. Anonymous GitHub URLs cannot download private artifacts.

```bash
gh auth status
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

The default destination is `~/.local/bin`. The app is included but need not run. Open `~/.local/bin/Slingring.app` explicitly, or set `CLI_LAUNCH_MENU_BAR=1` when installing. An already-running app is reopened after an upgrade.

## Manual and scheduled updates

```bash
slingring upgrade --check
slingring upgrade
slingring auto-update on
slingring auto-update status
slingring auto-update off
```

Daily update checks are off by default. Enabling them installs an independent OS scheduler, not a continuously polling process. A failed or offline check does not revoke your opt-in. Stop active task work before upgrading.

## Maintainers

The Cargo version and changelog are the release sources of truth. Run Rust formatting, strict Clippy, unit/integration tests, extension/relay tests, Swift bundle validation, and docs checks before tagging. `dist-workspace.toml` owns the Apple Silicon and Intel release targets. Regenerate the managed workflow with `dist generate` rather than editing its generated steps.

The tag's cargo-dist workflow publishes native artifacts. Its post-announce Swift job builds and signs the paired app bundles. The private repository remains private unless explicitly changed by its owner.
