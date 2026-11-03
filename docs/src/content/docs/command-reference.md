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
slingring send-message CHATGPT_URL [MESSAGE] [--stdin | --message-file FILE]
  [--image PATH]... [--request-id ID] [--json]
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

`new_task` aliases `new-task`; `send_message` aliases `send-message`; `queue` aliases `jobs`; `config get` aliases `config show`. `--prompt-file -` and `--message-file -` read stdin. Use each command's `--help` for full syntax.

`profile use` accepts an exact Chrome profile directory or a unique display name. It refuses changes while tasks are active. `browser install` installs the pinned, verified fork privately; it does not overwrite a separately installed browser CLI. An explicit absolute `browser-binary` override remains authoritative.

## Configuration

The default state directory is `~/.config/slingring`. `SLINGRING_HOME` can select another absolute directory. Supported `config set` keys include `profile`, `browser-binary`, `default-effort`, `timeout-secs`, and `heartbeat-secs`.

The default task-start timeout is 180 seconds, including follow-up readiness. Local queue waiting does not consume that per-task budget. Remote calls have an additional end-to-end deadline that includes queue waiting. Status and configuration output redact credentials. Metadata discovery and profile listing do not open a browser.

## MCP tools

`new_task` starts a conversation. It accepts only `prompt` and optional `reasoning_effort`; omitted or blank effort uses the configured default.

```json
{
  "name": "new_task",
  "arguments": {
    "prompt": "Review this plan.",
    "reasoning_effort": "High"
  }
}
```

`send_message` follows up on an existing conversation. It accepts exactly two required strings and preserves the chat's reasoning effort:

```json
{
  "name": "send_message",
  "arguments": {
    "chatgpt_url": "https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef",
    "message": "Now compare the two strongest options."
  }
}
```

Use the exact `https://chatgpt.com/c/ID` URL, where `ID` is 1..128 ASCII letters, digits, or hyphens. Queries, fragments, trailing slashes, other origins, and other routes, including Work, share, project, and custom GPT routes, are unsupported. The chat must be accessible to the configured Chrome account. MCP tools have no image or effort overrides for follow-ups; the local `send-message` CLI additionally accepts repeatable `--image PATH` files, preserving the current chat's reasoning effort.

```bash
slingring send-message 'https://chatgpt.com/c/YOUR-CHAT-ID' 'Review this screenshot' --image ./screenshot.png
```

The tool establishes Chat mode on the home page before opening the target conversation. It waits for a visible voice button with no stop control before filling the message. It never interrupts an existing response. It checks the target again before sending and returns only after fresh thinking, working, or response activity begins for the new turn. Historical answers do not establish success. If readiness is not reached within the startup budget, the follow-up fails without sending; uncertain results after submission authorization still require inspection. If the site replaces or virtualizes prior turns during submission, the conservative detector may leave the result uncertain even when the message was accepted.

Both tools reject unknown properties. Their advertised hints are `readOnlyHint: true`, `openWorldHint: false`, `destructiveHint: false`, and `idempotentHint: false`. These are client-facing metadata, not authorization controls: **both tools send a message through your account, creating or modifying a conversation**. Grant access accordingly.

The remote relay supports initialization, ping, tool discovery, and tool calls with negotiated protocol versions 2025-03-26, 2025-06-18, and 2025-11-25. Streamable HTTP uses POST JSON responses; GET and DELETE are not implemented. Local `slingring mcp` uses the official `rmcp` Rust SDK for stdio transport and protocol negotiation and exposes the same two text-only tools. The local stdio SDK requires the standard MCP initialization handshake.

## Limits

Prompts and follow-up messages must be nonblank and are limited to 128 KiB each; remote JSON bodies to 256 KiB. Both tools share a queue of 32 active or queued tasks. A follow-up waiting for a busy chat can delay later tasks; the queue does not exclude a person or another client from writing to the same chat. Local images for new tasks and follow-ups are limited to 16 files, each up to 20 MiB (100 MiB combined). PNG, JPEG, WebP, and GIF extensions are accepted; the ChatGPT UI must successfully accept the attachment too.

## Idempotency and uncertainty

Use `--request-id` with the local new-task or send-message CLI, or `Idempotency-Key` on remote HTTP requests, to recover an identical request. IDs are bound to the operation and selected profile, plus the prompt/effort/image content for a new task or target URL/message/image content for a follow-up. A different payload or operation using an existing ID is rejected.

A successful result contains `job_id`, `url`, `reasoning_effort`, and `evidence`. A follow-up returns the same target URL. `started` means genuine activity was observed, not that the response is complete. No generated answer is returned. `uncertain` means submission may have happened. Never automatically repeat a message or create a replacement after an uncertain outcome.
