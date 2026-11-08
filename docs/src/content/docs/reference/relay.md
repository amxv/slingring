---
title: Relay setup
description: One-time deployment for remote assistants.
order: 21
category: Reference
---

Local CLI use does not need a relay. For remote assistants, deploy the `relay/` workspace to your Cloudflare account. It uses a Worker and SQLite-backed Durable Object; no Redis, homebox, or tunnel process is needed.

## Deploy

Clone the repository and authenticate Wrangler to **your** Cloudflare account. The checked-in `relay/wrangler.jsonc` deliberately has no account ID, zone ID or custom domain. For a new installation, the default config deploys a Worker to your own account; choose a different Worker name in your own checkout if needed. Do not reuse another person's Worker or deployment hostname.

```bash
cd relay
bun install --frozen-lockfile
bunx wrangler login
bunx wrangler deploy
```

For an **existing Worker or a custom domain**, do **not** deploy with the unbound default config. Keep the existing Worker name and Durable Object migration unchanged; supply your own nonsecret account, zone and hostname identifiers to the guarded command below. This generates and removes a temporary ignored config without changing the checked-in template or touching the Worker secrets. `--dry-run` validates the configuration without deploying.

```bash
export CLOUDFLARE_ACCOUNT_ID=YOUR_32_CHARACTER_ACCOUNT_ID
export RELAY_ZONE_ID=YOUR_32_CHARACTER_ZONE_ID
export RELAY_HOSTNAME=mcp.example.com
bun run deploy:configured --dry-run
# After checking the intended Worker and route:
bun run deploy:configured
```

When using this repository's GitHub Actions automatic relay deployment (`DEPLOY_RELAY=true`), store the existing production values as GitHub Actions **secrets** `CLOUDFLARE_ACCOUNT_ID`, `RELAY_ZONE_ID` and `RELAY_HOSTNAME` **before merging a relay change**. Keep `CLOUDFLARE_DEPLOY_TOKEN` as the existing GitHub Actions secret; the `DEPLOY_RELAY` switch remains an Actions **variable**. Missing or invalid identifier secrets prevent deployment instead of silently targeting another account or removing a custom domain. No production deployment runs for pull requests. See [relay maintainer notes](https://github.com/amxv/slingring/blob/main/relay/README.md) for details.

Create two distinct random secrets with at least 32 bytes of entropy and store them in your password manager. Add `DEVICE_TOKEN` and `CLIENT_TOKEN` to the Worker using Wrangler's secret prompt or stdin. Do not put secret values in command arguments or commit them.

```bash
bunx wrangler secret put DEVICE_TOKEN
bunx wrangler secret put CLIENT_TOKEN
```

The `/device` endpoint uses the device token. MCP uses the client token. Do not use the same token for both. Browser-oriented challenges must not be applied to this authenticated machine endpoint. Keep any Cloudflare configuration exception scoped to the relay hostname, not the entire zone.

GET `/health` is public and returns only static service metadata. It does **not** reveal device connection status or prove the relay is ready; `/device` and `/mcp` still require their respective credentials.

## Connect your Mac

Pass credentials from your secret manager over stdin. The accepted JSON shape is:

```json
{
  "device_token": "YOUR_DEVICE_TOKEN",
  "mcp_url": "https://YOUR_RELAY/mcp/YOUR_CLIENT_TOKEN"
}
```

Pipe that JSON into:

```bash
slingring remote configure --url wss://YOUR_RELAY/device --token-stdin
slingring remote on
slingring service install
slingring status --json
```

When piping, connect the credential producer directly to the first command; never paste real secrets into a shell command. `remote configure` also accepts a raw device token on stdin, with optional `--mcp-url`, but credential JSON avoids exposing a secret-bearing URL in process arguments.

## Connect a client

Clients supporting headers may use `/mcp` with `Authorization: Bearer CLIENT_TOKEN`. URL-only clients use `/mcp/CLIENT_TOKEN` and select No authentication. The URL is a capability credential, not an unauthenticated endpoint. `slingring remote url | pbcopy` copies the saved connection URL on macOS.

## Transport behavior

The Mac initiates one outbound TLS WebSocket. A sparse heartbeat maintains connection health; it is not a job poll. The relay does not persist prompts. Offline requests fail rather than accumulating a backlog. Reconnecting never automatically resubmits a task.

The Worker authenticates both sides, checks browser Origins, bounds request size and outstanding requests, and strips URL credentials before forwarding to the Durable Object. Logs must not contain prompts or secret URLs. Disconnects and timeouts after submission are uncertain, not safe retry signals.

Slingring v0.4.0 and later return an accepted `in_progress` receipt after a five-second wait when a task is still preparing, then expose its outcome through `get_job_status`. The relay can forward these receipts without changing client URLs or credentials. Its 240-second HTTP safety limit remains for compatibility with older Macs; the updated daemon still applies a 230-second remote job deadline including queue wait.

Status/control requests have eight reserved relay slots separate from the 32 write slots, and status dispatch on the Mac does not wait for a submission. Read-only recovery therefore remains available when write capacity is full, subject to its own bound and device/network availability. Accepted task payloads remain only in Mac memory, so a crash never triggers a replay.
