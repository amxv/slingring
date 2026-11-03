---
title: Command reference
description: The local command surface at a glance.
order: 8
category: Reference
summary: The local command surface at a glance.
---

```text
slingring new-task [PROMPT | --prompt PROMPT | --prompt-file PATH|- | --stdin]
  [--reasoning-effort VALUE] [--image PATH]... [--request-id ID] [--json]
slingring config show [--json]
slingring config set KEY VALUE
slingring setup [--profile NAME] [--default-effort VALUE]
slingring status [--json]
slingring jobs [--json]
slingring service start|stop|restart|install|uninstall
slingring daemon [--ephemeral]
slingring remote configure --url wss://HOST/device --token-stdin
slingring remote on|off|status|url
slingring mcp
slingring upgrade [--check]
slingring auto-update on|off|status
```

Use command-local `--help` for details. Supply exactly one prompt source. Status does not start a service. `daemon` is a foreground diagnostic mode. `service install` registers the Rust service at login. The menu bar is a separate optional app. `mcp` is an optional local stdio adapter exposing the same text-only MCP tool; image attachments remain CLI-only.

## Optional menu bar

The native AppKit app shows service and remote status, latest job metadata, all five default efforts, service start/stop, configuration, jobs, updates, and launch at login. It queries the CLI when the menu opens or an action is selected. There are no status polling timers or embedded browser views. Quit closes only the menu bar app.
