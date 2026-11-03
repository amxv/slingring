---
title: Quickstart
description: Send your first local task and continue in ChatGPT.
order: 1
category: Start
summary: Send your first local task and continue in ChatGPT.
---

## Install

Slingring is initially private and macOS-only. Authenticate the GitHub CLI with repository access before downloading a release. The installer uses that existing login for private assets and verifies the release checksum and app signature.

```bash
gh auth status
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

The paired CLI and optional `Slingring.app` are installed under `~/.local/bin`. The menu bar does not need to run for either local or remote calls. Open it explicitly when useful:

```bash
open "$HOME/.local/bin/Slingring.app"
```

To build from source instead:

```bash
gh repo clone amxv/slingring
cd slingring
cargo install --locked --path .
slingring --help
```

Use the pinned Rust toolchain. Install the tested `agent-browser-browse` fork separately and prepare its authenticated Chrome `Profile 6`, isolated from everyday Chrome. A normal agent-browser installation is not a substitute for the tested fork. Browser setup and account access are prerequisites; Slingring does not log you in.

```bash
slingring setup --profile 'Profile 6' --default-effort 'Extra High'
```

Setup resolves the browser executable and installs private extension assets. It does not launch Chrome or modify your everyday browser's extensions.

## Hand off a task

```bash
slingring new-task "Review this migration plan" --reasoning-effort High --json
```

A local call uses the running daemon or starts an ephemeral daemon. The ephemeral daemon drains its queue and closes the browser before exiting. No persistent service is required for local use.

The result contains job metadata and, on success, a canonical ChatGPT `/c/...` URL. Open that URL to continue. `started` means real activity was observed after submission, not that the answer is finished. Slingring never returns generated assistant text.

## Inspect without starting a service

```bash
slingring status --json
slingring jobs --json
slingring config show --json
```

Next: [local input and images](/docs/local-input), [remote access](/docs/remote), or [reasoning modes](/docs/reasoning).
