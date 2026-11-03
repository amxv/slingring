---
title: Command reference
description: The CLI is the source of truth for every integration.
order: 8
category: Reference
summary: Task submission, configuration, service lifecycle, remote access, and upgrades.
---

```text
slingring setup [--profile NAME] [--default-effort VALUE]
slingring new-task [PROMPT] [--stdin | --prompt-file FILE]
  [--reasoning-effort VALUE] [--image PATH]... [--request-id ID] [--json]
slingring config show [--json]
slingring config set KEY VALUE
slingring status [--json]
slingring jobs [--json]
slingring service start|stop|restart|install|uninstall|status
slingring daemon [--ephemeral]
slingring remote configure --url wss://HOST/device --token-stdin [--mcp-url URL]
slingring remote on|off|status|url
slingring mcp
slingring upgrade [--check]
slingring auto-update on|off|status
```

Use command-local `--help` for the complete syntax. `new_task` aliases `new-task`, `queue` aliases `jobs`, `config get` aliases `config show`, and `remote disable` aliases `remote off`.

## Configuration

The default configuration directory is `~/.config/slingring`; `SLINGRING_HOME` selects an isolated instance for testing. `setup` resolves `agent-browser-browse` to an absolute executable path so a LaunchAgent can find the same runner.

`config set` supports `default-effort`, `profile`, `browser-binary`, `timeout-secs`, and `heartbeat-secs`. Use `site` as the default effort to preserve the currently selected ChatGPT setting. Blank effort on an individual task uses the configured default.

Status, configuration inspection, job inspection, and local MCP metadata discovery do not start a browser or daemon. Configuration and status output redact credentials.

## Optional local MCP adapter

The CLI is the preferred local surface, especially for image attachments. `slingring mcp` is an optional stdio adapter for clients that require MCP. It exposes the same text-only tool as the remote server; filesystem arguments remain exclusive to `new-task`.

## Menu bar

The native app wraps these CLI commands for service status, default reasoning, remote access, copying the private connection URL, opening the latest chat, updates, and service startup at login. Opening the menu refreshes its status; there is no idle polling timer or embedded browser. Quitting the menu bar leaves the CLI and any enabled remote service independent.
