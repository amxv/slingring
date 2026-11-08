# Slingring transport relay

Self-hosted Cloudflare Worker and one SQLite-backed Durable Object. The Mac makes one outbound WebSocket connection to `/device`. Authenticated POSTs to `/mcp` are correlated and forwarded to Rust, which owns all MCP methods, tool schemas, argument validation, image rejection, and JSON-RPC responses. The relay adds no tools and never interprets tool arguments. JSON values are preserved; whitespace and object serialization may change.

## Local validation

Use Bun (the same toolchain used by CI):

```sh
cd relay
bun install --frozen-lockfile
bun run check
bun run test
CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV=false bun run dry-run
```

The committed lockfile pins Wrangler 4. The dry run bundles locally, without deployment. Tests mock Durable Object state, hibernating sockets, timers and the Workers constant-time primitive. They do not contact a Mac or exercise deployed Cloudflare infrastructure. No Redis, database tables, polling loop or background task worker is used.

## Configuration

The checked-in `wrangler.jsonc` is a **public, account-neutral template**. It defines the `slingring-relay` Worker, the `RELAY` Durable Object binding and its SQLite migration `v1`, but has no account ID, zone ID or custom domain. A first-time user can run `bunx wrangler deploy` with their own Wrangler login to publish a Worker on their own account's Workers route. Change the Worker name in your own checkout if desired; never point at another person's Worker or bind an unrelated Durable Object namespace.

For an **existing custom-domain deployment**, use `bun run deploy:configured` with three nonsecret environment variables. It generates a temporary, gitignored Wrangler config using the checked-in Worker name and all existing bindings, adds your account ID, zone ID and custom-domain hostname, then removes the temporary file. The config generator rejects missing or malformed identifiers before Wrangler starts. **Keep the exact same Worker name, account, zone, hostname, and migration when updating an existing deployment** to avoid moving traffic or losing access to its Durable Object state. It passes Wrangler's `--keep-vars` to preserve any dashboard-managed Worker variables; existing Worker secrets are also retained. No credentials are written into the generated file.

```sh
export CLOUDFLARE_ACCOUNT_ID=YOUR_32_CHARACTER_ACCOUNT_ID
export RELAY_ZONE_ID=YOUR_32_CHARACTER_ZONE_ID
export RELAY_HOSTNAME=mcp.example.com
bun run deploy:configured --dry-run
# Only when ready to deploy to your existing Worker:
bun run deploy:configured
```

For this repository's optional production deploy Action (`DEPLOY_RELAY=true`), store the **existing** production identifiers in GitHub Actions **secrets** named `CLOUDFLARE_ACCOUNT_ID`, `RELAY_ZONE_ID`, and `RELAY_HOSTNAME` before merging changes to `relay/`. The existing `CLOUDFLARE_DEPLOY_TOKEN` remains a GitHub **secret**, unchanged. `DEPLOY_RELAY` itself stays an Actions **variable**. The workflow fails before deploying if any identifier secret is missing or invalid; do not replace them with examples. No automatic deployment happens on pull requests. If your Worker has a different name, preserve it in your own `wrangler.jsonc` before deploying.

Required secrets:

- `CLIENT_TOKEN`: authenticates the `/mcp` endpoint and `/mcp/CLIENT_TOKEN` capability URLs.
- `DEVICE_TOKEN`: distinct secret used only for `/device`.

Missing or equal secrets fail closed for authenticated endpoints. Requests to `/mcp` and `/device`, including upgrades, invalid methods and Durable Object requests, are authenticated using SHA-256 digests and the Workers `crypto.subtle.timingSafeEqual` primitive. Authentication headers take precedence over URL keys. **GET `/health` is intentionally public**: it returns static service metadata only, never a client or device connection status, even if credentials have not been configured.

Nonsecret Wrangler variables:

- `ALLOW_QUERY_KEY`: `false` by default. The `/mcp/CLIENT_TOKEN` capability path works without it; setting the variable to the exact string `true` additionally permits `/mcp?key=CLIENT_TOKEN`. URL keys never authenticate `/device` or `/health`. Prefer headers; URL credentials can enter client, proxy and platform access logs. Disable URL/request logging on every upstream service before using URL-key auth. The relay strips query and path credentials before forwarding to its DO and emits no application logs.
- `ALLOWED_ORIGINS`: optional comma-separated list of exact origins. When unset, allow `https://chatgpt.com` and the incoming request's own origin. When set, it replaces the defaults. No wildcards, suffix matching or `null` origin default. Requests without Origin are allowed for server clients. No cross-origin browser preflight/CORS support is advertised.

## Manual deployment

No deployment is performed by this repository's validation commands (including `dry-run`). Authenticate Wrangler using your own Cloudflare account, then deploy using the public default config or the configured custom-domain command above. For a **new** Worker, generate two independent 256-bit secrets in private temporary files and add them to that same Worker. For an **existing** Worker, retain the provisioned secrets and do **not** regenerate them. Run the following only for a new installation, with shell tracing disabled:

```sh
set +x
umask 077
secret_dir=$(mktemp -d)
openssl rand -hex 32 > "$secret_dir/client"
openssl rand -hex 32 > "$secret_dir/device"
bunx wrangler deploy
bunx wrangler secret put CLIENT_TOKEN < "$secret_dir/client"
bunx wrangler secret put DEVICE_TOKEN < "$secret_dir/device"
```

Transfer the client token into your MCP client's private credential configuration without printing it. On the Mac, feed the device token through stdin:

```sh
slingring remote configure --url wss://YOUR-WORKER.workers.dev/device --token-stdin < "$secret_dir/device"
```

Keep these files only until both sides are configured, then remove them and the directory. Never commit tokens, paste them into command arguments, enable shell tracing, or log request bodies, headers, URLs or frames. `.dev.vars*` is ignored for local testing; create such files privately only if needed. Disable platform request logging and third-party proxy logging if they could retain credentials. Secret rotation requires reconnecting the device; deploy/restart the DO as part of rotation to close existing authenticated sockets. Socket authentication occurs at upgrade time, while every HTTP request authenticates independently.

## HTTP and bridge contract

All HTTP responses use `Cache-Control: no-store`; non-upgrade responses use `Content-Type: application/json`. Public GET `/health` returns `{ "service": "slingring-relay", "ok": true }` without credentials. This is **not** a readiness, connection, or task-success check. The Durable Object's internal `/health` implementation reports socket presence, but that response is not exposed by the public Worker route. Authenticated GET `/mcp` returns 405 with `Allow: POST`. POST requires `Content-Type: application/json` (charset parameters allowed) and Accept permitting JSON, such as `application/json` or `application/json, text/event-stream`. SSE and session storage are unnecessary because Rust returns JSON responses. Request cancellation and MCP session semantics remain Rust's responsibility.

POST bodies must be JSON objects, bounded to 512 KiB even without Content-Length. Transport rejects malformed JSON and arrays. It forwards arbitrary fields and arguments without policy filtering. Serialized bridge frames, including their envelope, are also bounded to 512 KiB. Optional `Idempotency-Key` is forwarded unchanged and restricted to 1..100 ASCII letters, digits, hyphens or underscores. Rust owns deduplication; the relay never replays a request.

Worker to Mac:

```json
{"type":"rpc","id":"generated-uuid","request":{"jsonrpc":"2.0","id":1,"method":"ping"},"idempotency_key":"optional"}
```

Mac to Worker:

```json
{"type":"rpc_result","id":"same-uuid","response":{"jsonrpc":"2.0","id":1,"result":{}}}
```

For a notification (request has no `id`), Rust confirms delivery with `response: null`; only then does HTTP return an empty 202. A normal JSON response, including any Rust JSON-RPC error, returns HTTP 200 unchanged. A JSON object with `id: null` is treated as expecting a response. Malformed or mismatched bridge envelopes close the socket and fail active calls. Late/unknown correlation IDs are discarded and cannot resolve a different request.

Only one device socket remains active: a newly authenticated device connection replaces the previous socket. `acceptWebSocket` enables hibernation, and automatic `ping`/`pong` string replies permit a low-frequency daemon heartbeat without waking the object. No timers or intervals run while idle. Active HTTP calls keep their in-memory correlations until completion, disconnect, or the 240-second deadline; pending calls are not persisted or retried across restarts. Capacity is 32 pending writes plus eight reserved control/status requests; overflow receives 429. Offline and disconnected calls receive 503, deadline expiration 504. Disconnects/timeouts explicitly report an uncertain outcome: the Mac might already have submitted a task. Clients must not automatically retry writes. The Rust ledger and an explicitly reused idempotency key are the recovery mechanism.
