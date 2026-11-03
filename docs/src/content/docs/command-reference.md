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
slingring jobs [--request-id ID] [--json]
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

The default state directory is `~/.config/slingring`. `SLINGRING_HOME` can select another absolute directory. Supported `config set` keys include `profile`, `browser-binary`, `browser-mode`, `default-effort`, `timeout-secs`, and `heartbeat-secs`.

`browser-mode` accepts `headless` (the default) or `headed` (a visible window for debugging). It persists as the boolean `browser_headed` configuration field; `config set browser-headed true|false` is an equivalent spelling. Existing configuration files default to headless. Use Configure → Browser Mode in the menu bar or:

```bash
slingring config set browser-mode headed
slingring config set browser-mode headless
```

Local CLI, local MCP and remote MCP submissions all use the saved setting when they enter the queue. Active and already queued tasks keep their original setting; the owned browser is reopened between tasks when a setting change requires it. Both modes use the isolated profile, pinned browser helper, draft protection and the same compatibility flags. Headed mode only makes the automation visible and can have different site-detection behavior; it does not guarantee that a challenge will disappear.

`status` separates `active_job`, `starting_tasks` and `queued_tasks` from historical `latest_finished_job` and `latest_started_job`. A historical `started` record confirms submission and fresh activity, not a still-running task or a finished answer. If the service owns its lock but cannot answer a status request, `status_available` is false and the CLI exposes saved history without claiming that the queue is idle. The menu bar refreshes when opened; it has no background status timer.

The default task-start timeout is 180 seconds, including follow-up readiness. Local queue waiting does not consume that per-task budget. Local and remote MCP write calls wait up to five seconds after queue admission, then return an accepted receipt if the task is still in progress. Direct CLI write commands continue waiting for the confirmed result. Remote Cloudflare requests retain a 240-second HTTP safety limit with a 230-second Mac-side job deadline (including queue wait), and the configured browser-start budget is the tighter limit when applicable. Client and network timeouts can still happen before receipt delivery; they do **not** cancel accepted MCP jobs. Status and configuration output redact credentials. Metadata discovery and profile listing do not open a browser.

## MCP tools

`new_task` starts a conversation. It accepts `prompt`, optional `reasoning_effort`, and optional `request_id`; omitted or blank effort uses the configured default.

```json
{
  "name": "new_task",
  "arguments": {
    "prompt": "Review this plan.",
    "reasoning_effort": "High",
    "request_id": "review-001"
  }
}
```

`send_message` follows up on an existing conversation. It accepts two required strings and optional `request_id`, preserving the chat's reasoning effort:

```json
{
  "name": "send_message",
  "arguments": {
    "chatgpt_url": "https://chatgpt.com/c/01234567-89ab-cdef-0123-456789abcdef",
    "message": "Now compare the two strongest options.",
    "request_id": "followup-002"
  }
}
```

Use the exact `https://chatgpt.com/c/ID` URL, where `ID` is 1..128 ASCII letters, digits, or hyphens. Queries, fragments, trailing slashes, other origins, and other routes, including Work, share, project, and custom GPT routes, are unsupported. The chat must be accessible to the configured Chrome account. MCP tools have no image or effort overrides for follow-ups; the local `send-message` CLI additionally accepts repeatable `--image PATH` files, preserving the current chat's reasoning effort.

```bash
slingring send-message 'https://chatgpt.com/c/YOUR-CHAT-ID' 'Review this screenshot' --image ./screenshot.png
```

The tool establishes Chat mode on the home page before opening the target conversation. It waits for a visible voice button with no stop control before filling the message. It never interrupts an existing response. It checks the target again before sending and confirms submission only after fresh thinking, working, or response activity begins for the new turn. An earlier accepted receipt does not establish success, and neither do historical answers. If readiness is not reached within the startup budget, the follow-up fails without sending; uncertain results after submission authorization still require inspection. If the site replaces or virtualizes prior turns during submission, the conservative detector may leave the result uncertain even when the message was accepted.

Both write tools reject unknown properties. Their advertised hints are `readOnlyHint: true`, `openWorldHint: false`, `destructiveHint: false`, and `idempotentHint: false`. These are client-facing metadata, not authorization controls: **both tools send a message through your account, creating or modifying a conversation**. Grant access accordingly.

`get_job_status(request_id)` is a third, **read-only** MCP tool. It looks up one recorded job on the Mac and never submits, retries, or launches a browser. Its sole required parameter is a 1..100-character ID containing only ASCII letters, digits, underscores or hyphens. Use the same ID on the original write tool before sending, or set the equivalent `Idempotency-Key` HTTP header when using the relay directly. If both are supplied, they must match. IDs must be unique to a logical request; Slingring rejects a different payload using the same ID.

```json
{
  "name": "get_job_status",
  "arguments": { "request_id": "review-001" }
}
```

The tool returns journal metadata (status, phase, elapsed milliseconds, error code, URL if known) and a recovery classification: `in_progress`, `submitted`, `not_submitted`, `uncertain`, or `unknown`. `unknown` also covers records pruned from the bounded journal and is **not** permission to retry. After a daemon crash, queued records become `not_submitted` and interrupted starting records are conservatively `uncertain`. They are never left indefinitely in progress or replayed. Querying status does not keep the browser open. The Mac stores bounded, payload-free lifecycle events in `~/.config/slingring/events.log` (plus an optional previous rotated segment), with browser launch/retry categories and elapsed times. No raw browser stdout/stderr, prompts, generated answers, tokens or URLs are written to that diagnostics log.

The remote relay supports initialization, ping, tool discovery, and tool calls with negotiated protocol versions 2025-03-26, 2025-06-18, and 2025-11-25. Streamable HTTP uses POST JSON responses; GET and DELETE are not implemented. Local `slingring mcp` uses the official `rmcp` Rust SDK for stdio transport and protocol negotiation and exposes the same three text-only tools. The local stdio SDK requires the standard MCP initialization handshake.

## Limits

Prompts and follow-up messages must be nonblank and are limited to 128 KiB each; remote JSON bodies to 256 KiB. Both tools share a queue of 32 active or queued tasks. A follow-up waiting for a busy chat can delay later tasks; the queue does not exclude a person or another client from writing to the same chat. Local images for new tasks and follow-ups are limited to 16 files, each up to 20 MiB (100 MiB combined). PNG, JPEG, WebP, and GIF extensions are accepted; the ChatGPT UI must successfully accept the attachment too.

## Idempotency and uncertainty

Use `--request-id` with local CLI operations, or `request_id` in remote MCP write arguments (or `Idempotency-Key` for raw HTTP), to identify a request **before** submitting. IDs are bound to the operation and selected profile, plus the prompt/effort/image content for a new task or target URL/message/image content for a follow-up. A different payload or operation using an existing ID is rejected. The HTTP relay echoes `X-Slingring-Request-Id` if a response arrives, but that header is not available if the caller times out before receiving a response. The relay's own 504 contains the request ID, `outcome: unknown`, and the status tool name; it does not assert failure.

A confirmed result contains `job_id`, `url`, `reasoning_effort`, and `evidence`. A follow-up returns the same target URL. If that result is not available within the MCP five-second wait, the tool instead returns a non-error receipt with `request_id`, `job_id`, `accepted: true`, `outcome: in_progress`, `recovery_tool: get_job_status`, and current job metadata. Acceptance confirms admission to the Mac's in-memory queue, not submission to ChatGPT. Read status using the same ID rather than creating another request. Accepted MCP jobs continue after the call returns or disconnects; unstarted direct CLI jobs whose caller goes away are skipped.

`started` means genuine new activity was observed, not that the response is complete. No generated answer is returned. `uncertain` means submission may have happened. Never automatically repeat a message or create a replacement after an uncertain outcome.
