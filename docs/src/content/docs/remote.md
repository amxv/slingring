---
title: Remote agents
description: Reach your Mac through a small authenticated relay, without an inbound port.
order: 4
category: Start
summary: One Streamable HTTP tool, one outbound connection, and no Redis.
---

## How remote access works

The Mac opens one outbound TLS WebSocket to your Cloudflare Worker. A hibernating Durable Object connects incoming Streamable HTTP requests to that socket. The relay does not run a browser, schedule work, interpret prompts, or store a task queue. Rust owns all task behavior.

The Mac must be awake, logged into its desktop session, and online. Sleeping, offline, or disconnected Macs return an error rather than retaining prompts for surprise execution later. No homebox, Redis, inbound firewall rule, or `cloudflared` process is required.

## Deploy a relay

Deploy the repository's `relay/` directory to your own Cloudflare account and hostname. Set **different random secrets** for `DEVICE_TOKEN` and `CLIENT_TOKEN`. Change the account ID and custom domain in `wrangler.jsonc` for your installation. Observability is off so capability URLs and prompts are not written to Worker logs.

```bash
cd relay
bun install --frozen-lockfile
bunx wrangler deploy
# Pipe tokens from a secret manager; do not put secrets in command arguments.
printf '%s' "$DEVICE_TOKEN" | bunx wrangler secret put DEVICE_TOKEN
printf '%s' "$CLIENT_TOKEN" | bunx wrangler secret put CLIENT_TOKEN
```

Configure the Mac through stdin. The credential JSON contains `device_token` and optionally the private `mcp_url` for the menu bar's copy action.

```bash
cat /path/to/private-credentials.json | slingring remote configure \
  --url wss://YOUR-RELAY/device --token-stdin
slingring remote on
slingring service install
slingring status --json
```

Remove the temporary credential file after securely configuring it. Configuration is saved under private local permissions; secrets do not appear in ordinary status output.

## Connect an assistant

For clients supporting headers, use `https://YOUR-RELAY/mcp` with `Authorization: Bearer CLIENT_TOKEN`. URL-only clients can use `https://YOUR-RELAY/mcp/CLIENT_TOKEN` and choose **No authentication** in the connector UI: the unguessable URL itself is the credential.

The locally configured URL is available through `slingring remote url` or **Copy Remote MCP URL** in the menu bar. Do not paste it into issues, public logs, or screenshots. Query-string keys are disabled by default.

```text
new_task(prompt: string, reasoning_effort?: string)
```

Blank or omitted effort uses the configured default. The tool returns a chat URL after real assistant activity starts, not the completed answer. `image_paths`, `images`, attachments, profile overrides, and every unknown argument are rejected. Local images are available only on the CLI.

## Disable or stop

```bash
slingring remote off
slingring service stop
slingring service uninstall
```

`remote off` closes the outbound connection; the idle local process exits after work drains. `service stop` stops current processing, so inspect any interrupted task before retrying. `service uninstall` additionally removes login registration. None of these deletes browser history or ChatGPT conversations.

The relay's deadlines are bounded. A transport error after submission may be uncertain; do not automatically resubmit. A client able to set `Idempotency-Key` can use the same key with identical input to recover a retained result without creating another chat.
