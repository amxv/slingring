---
title: Releases and updates
description: Authenticated private downloads and direct macOS updates.
order: 9
category: Distribution
summary: Checksummed CLI and menu bar bundles, without npm.
---

## Private distribution

Use the [quickstart installer](/docs/quickstart) with an authenticated GitHub CLI session. The installer prefers authenticated downloads and falls back to public downloads when available. With repository access, assets can also be downloaded directly:

```bash
gh release list --repo amxv/slingring
gh release download <TAG> --repo amxv/slingring --dir ./release-assets
```

Select the matching macOS architecture and verify the release checksum before extracting. Keep the companion CLI next to the optional `.app`. Asset availability depends on an actual published release. Do not assume a release exists yet.

While the repository is private, anonymous downloads do not work. `upgrade --check` uses authenticated GitHub metadata when available. Updates download matching checksums and verify the CLI/app bundle before replacing the installation. An active task defers installation. The app reopens only when it was already running or explicitly requested.

## Update controls

```bash
slingring upgrade --check
slingring upgrade
slingring auto-update status
slingring auto-update on
slingring auto-update off
```

Automatic updates are off by default. Opt-in daily checks use the OS scheduler independently of the menu bar, with no app polling timer. Enable them only once the installation's release access path works. Ad-hoc signing is not Developer ID notarization; downloaded apps may trigger Gatekeeper. Do not disable macOS security controls.

## Maintainers

`dist-workspace.toml` is the release source of truth. Regenerate the release workflow with `dist generate`, never hand-edit it. Follow `.agents/skills/release/SKILL.md` for a release. The optional app packaging builds Apple Silicon and Intel bundles with matching CLI assets and checksum sidecars. There is no npm distribution.
