---
title: Remote access
description: One outbound push connection, one MCP tool, no polling queue.
order: 4
category: Start
summary: Connect an assistant through your authenticated Streamable HTTP relay.
---

## Connect the Mac

Remote mode adds one outbound WSS connection from the Rust service to a Cloudflare Worker. A hibernating Durable Object forwards requests to the connected Mac. There is no homebox, Redis, cloudflared process, inbound Mac port, or durable cloud prompt queue.

Deploy `relay/` into your own Cloudflare account first. Its Worker secrets must contain distinct, cryptographically random `DEVICE_TOKEN` and `CLIENT_TOKEN` values. Never reuse either token for another service.

```bash
# DEVICE_TOKEN comes from your secret manager, not a literal in shell history.
printf '%s' "$DEVICE_TOKEN" | slingring remote configure \
  --url wss://YOUR_RELAY/device --token-stdin
slingring remote on
slingring service install
slingring status --json
```

`remote on` enables the connection. `service install` starts the user LaunchAgent and arranges startup after login. The Mac must be awake, logged in, online, and able to use the configured ChatGPT session. Slingring does not prevent sleep or bypass login challenges.

## Add a remote assistant

Clients that support HTTP headers use `https://YOUR_RELAY/mcp` with `Authorization: Bearer CLIENT_TOKEN`.

For a client such as ChatGPT configured with **No authentication**, use the private URL `https://YOUR_RELAY/mcp/CLIENT_TOKEN`. The unguessable URL is the authentication credential. It is not a public, unauthenticated endpoint. Keep it out of screenshots, source control, support logs, and shared chats.

`remote configure` also accepts credential JSON on stdin with `device_token` and `mcp_url`. This lets a provisioning script save the complete private connection URL without exposing it in command arguments. The menu bar's **Copy Remote MCP URL** and `slingring remote url` reveal that URL only when explicitly requested.

## One tool

```json
{
  "name": "new_task",
  "arguments": {
    "prompt": "Review this plan and identify the biggest risks.",
    "reasoning_effort": "Extra High"
  }
}
```

Omit the effort or pass an empty string to use the local default. No remote argument accepts images, filesystem paths, arbitrary URLs, shell commands, or browser configuration. Unknown fields are rejected.

The server implements MCP initialization, ping, tool discovery, and this one write tool. Successful calls return task metadata and a canonical ChatGPT URL, not the generated answer. HTTP POST JSON responses are used; the optional MCP GET event stream is not exposed.

## Failure and reconnect behavior

Requests are bounded to 256 KiB and 32 outstanding calls. A disconnected Mac produces an explicit error. Transport reconnects use bounded exponential backoff; they never replay a task automatically. A sparse 60-second application heartbeat maintains and checks the connection. It is not a job poll.

Remote requests carry an end-to-end deadline. A queued request whose caller has gone away is cancelled before submission. If a request may already have reached ChatGPT, the result is uncertain instead of retried. Check the local journal before requesting another task. Clients with custom headers may use a unique `Idempotency-Key` to recover the same operation safely; reuse with different arguments is rejected.

```bash
slingring remote off
slingring service stop
```

Disabling remote access preserves its credentials. Stopping the service does not delete configuration or chat history.
