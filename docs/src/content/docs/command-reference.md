---
title: CLI and MCP reference
description: Commands, arguments, limits, and retry semantics.
order: 20
category: Reference
---

## CLI

```text
slingring setup [--profile NAME] [--default-effort LEVEL]
slingring profile list [--json]
slingring profile use NAME
slingring browser install|status
slingring new-task [PROMPT] [--stdin | --prompt-file FILE]
  [--reasoning-effort LEVEL] [--image PATH]... [--request-id ID] [--json]
slingring config show [--json]
slingring config set KEY VALUE
slingring status [--json]
slingring jobs [--json]
slingring service start|stop|restart|install|uninstall|status
slingring remote configure --url wss://HOST/device --token-stdin [--mcp-url URL]
slingring remote on|off|status|url
slingring mcp
slingring daemon [--ephemeral]
slingring upgrade [--check]
slingring auto-update on|off|status
```

`new_task` aliases `new-task`; `queue` aliases `jobs`; `config get` aliases `config show`. `--prompt-file -` reads stdin. Use each command's `--help` for full syntax.

`profile use` accepts an exact Chrome profile directory or a unique display name. It refuses changes while tasks are active. `browser install` installs the pinned, verified fork privately; it does not overwrite a separately installed browser CLI. An explicit absolute `browser-binary` override remains authoritative.

## Configuration

The default state directory is `~/.config/slingring`. `SLINGRING_HOME` can select another absolute directory. Supported `config set` keys include `profile`, `browser-binary`, `default-effort`, `timeout-secs`, and `heartbeat-secs`.

The default task-start timeout is 180 seconds. Local queue waiting does not consume that per-task budget. Remote calls have an additional end-to-end deadline. Status and configuration output redact credentials. Metadata discovery and profile listing do not open a browser.

## One MCP tool

```json
{
  "name": "new_task",
  "arguments": {
    "prompt": "Review this plan.",
    "reasoning_effort": "High"
  }
}
```

Only `prompt` and optional `reasoning_effort` are accepted; unknown properties are rejected. The advertised hints are `readOnlyHint: true`, `openWorldHint: false`, `destructiveHint: false`, and `idempotentHint: false`. These are client-facing metadata, not authorization controls: **invoking the tool sends the prompt to ChatGPT and creates a conversation**. Grant access accordingly.

The server supports initialization, ping, tool discovery, and tool calls. Supported negotiated protocol versions are 2025-03-26, 2025-06-18, and 2025-11-25. Streamable HTTP uses POST JSON responses; GET and DELETE are not implemented. Local `slingring mcp` provides the same text-only surface over stdio.

## Limits

Prompts are limited to 128 KiB; remote JSON bodies to 256 KiB. The queue permits 32 active or queued tasks. Local images are limited to 16 files, each up to 20 MiB. PNG, JPEG, WebP, and GIF extensions are accepted; the ChatGPT UI must successfully accept the attachment too.

## Idempotency and uncertainty

Use `--request-id` locally, or `Idempotency-Key` on remote HTTP requests, to recover an identical request. IDs are bound to the prompt, effort, selected profile, and image content. A different payload using an existing ID is rejected.

A successful result contains `job_id`, `url`, `reasoning_effort`, and `evidence`. `started` means genuine activity was observed, not that the response is complete. `uncertain` means submission may have happened. Never automatically create a replacement after an uncertain outcome.
