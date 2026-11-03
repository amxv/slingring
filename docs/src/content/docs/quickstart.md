---
title: Quickstart
description: Send your first local task and continue in ChatGPT.
order: 1
category: Start
summary: Send your first local task and continue in ChatGPT.
---

## Install a private release

Slingring runs on macOS. The repository is initially private, so sign into `gh` with repository access. The installer uses that authenticated session to fetch the checksummed release.

```bash
curl --proto '=https' --tlsv1.2 -fsSL https://slingring.ashray.xyz/install.sh -o /tmp/slingring-install.sh
sh /tmp/slingring-install.sh
rm /tmp/slingring-install.sh
slingring setup --profile "Profile 6" --default-effort "Extra High"
```

Install the tested [agent-browser-browse fork](https://github.com/amxv/agent-browser-browse) separately. Sign into ChatGPT in the chosen Chrome profile once. Slingring reuses an isolated copy, not your everyday browser window. Upstream `agent-browser` is not a substitute for the tested fork. Slingring does not log you in or bypass an authentication challenge.

The installer includes the Swift menu bar app beside the CLI but does not start it automatically. It is optional; open `~/.local/bin/Slingring.app` for service controls and defaults.

For a source build, clone the private repository and run `cargo install --locked --path .` with the pinned toolchain, then run `slingring setup`.

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
