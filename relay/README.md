# Slingring transport relay

Private Cloudflare Worker and one SQLite-backed Durable Object. The Mac makes one outbound WebSocket connection to `/bridge`. Authenticated POSTs to `/mcp` are correlated and forwarded to Rust, which owns all MCP methods, tool schemas, argument validation, image rejection, and JSON-RPC responses. The relay adds no tools and never interprets tool arguments. JSON values are preserved; whitespace and object serialization may change.

## Local validation

Use Node 22 or newer and npm:

```sh
cd relay
npm ci
npm run check
npm test
CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV=false npm run dry-run
```

The committed lockfile pins Wrangler 4. The dry run bundles locally, without deployment. Tests mock Durable Object state, hibernating sockets, timers and the Workers constant-time primitive. They do not contact a Mac or exercise deployed Cloudflare infrastructure. No Redis, database tables, polling loop or background task worker is used.

## Configuration

`wrangler.jsonc` binds `RELAY` to `Relay` and creates its SQLite Durable Object namespace with migration `v1`. Retain that migration after deployment. Change the Worker name as needed. Do not bind any existing DO namespace containing unrelated data.

Required secrets:

- `CLIENT_TOKEN`: authenticates `/mcp` and `/health`.
- `DEVICE_TOKEN`: distinct secret used only for `/bridge`.

Missing or equal secrets fail closed. Each request, including health, upgrades, invalid methods and DO requests, is authenticated using SHA-256 digests and the Workers `crypto.subtle.timingSafeEqual` primitive. Authentication headers take precedence over query keys.

Nonsecret Wrangler variables:

- `ALLOW_QUERY_KEY`: `false` by default. Set to the exact string `true` to permit `/mcp?key=CLIENT_TOKEN` when a client cannot configure Authorization headers. Query auth never works for `/bridge` or `/health`. Prefer headers; URL credentials can enter client, proxy and platform access logs. Disable URL/request logging on every upstream service before using query auth. The relay strips the query before forwarding to its DO and emits no application logs.
- `ALLOWED_ORIGINS`: optional comma-separated list of exact origins. When unset, allow `https://chatgpt.com` and the incoming request's own origin. When set, it replaces the defaults. No wildcards, suffix matching or `null` origin default. Requests without Origin are allowed for server clients. No cross-origin browser preflight/CORS support is advertised.

## Manual deployment

No deployment is performed by this repository's validation commands. Authenticate Wrangler yourself using your Cloudflare account, then generate two independent 256-bit secrets in private temporary files. Run this in your own terminal, with shell tracing disabled. These commands do not print the secrets:

```sh
set +x
umask 077
secret_dir=$(mktemp -d)
openssl rand -hex 32 > "$secret_dir/client"
openssl rand -hex 32 > "$secret_dir/device"
npx wrangler secret put CLIENT_TOKEN < "$secret_dir/client"
npx wrangler secret put DEVICE_TOKEN < "$secret_dir/device"
npx wrangler deploy
```

Transfer the client token into your MCP client's private credential configuration without printing it. On the Mac, feed the device token through stdin:

```sh
slingring remote configure --url wss://YOUR-WORKER.workers.dev/bridge --token-stdin < "$secret_dir/device"
```

Keep these files only until both sides are configured, then remove them and the directory. Never commit tokens, paste them into command arguments, enable shell tracing, or log request bodies, headers, URLs or frames. `.dev.vars*` is ignored for local testing; create such files privately only if needed. Disable platform request logging and third-party proxy logging if they could retain credentials. Secret rotation requires reconnecting the device; deploy/restart the DO as part of rotation to close existing authenticated sockets. Socket authentication occurs at upgrade time, while every HTTP request authenticates independently.

## HTTP and bridge contract

All HTTP responses use `Cache-Control: no-store`; non-upgrade responses use `Content-Type: application/json`. `/health` accepts authenticated GET and returns only `{ "online": boolean }`. This indicates socket presence, not daemon responsiveness or task success. GET `/mcp` returns 405 with `Allow: POST`. POST requires `Content-Type: application/json` (charset parameters allowed) and Accept permitting JSON, such as `application/json` or `application/json, text/event-stream`. SSE and session storage are unnecessary because Rust returns JSON responses. Request cancellation and MCP session semantics remain Rust's responsibility.

POST bodies must be JSON objects, bounded to 512 KiB even without Content-Length. Transport rejects malformed JSON and arrays. It forwards arbitrary fields and arguments without policy filtering. Serialized bridge frames, including their envelope, are also bounded to 512 KiB. Optional `Idempotency-Key` is forwarded unchanged, capped at 1024 UTF-8 bytes. Rust owns deduplication; the relay never replays a request.

Worker to Mac:

```json
{"type":"rpc","id":"generated-uuid","request":{"jsonrpc":"2.0","id":1,"method":"ping"},"idempotency_key":"optional"}
```

Mac to Worker:

```json
{"type":"rpc_result","id":"same-uuid","response":{"jsonrpc":"2.0","id":1,"result":{}}}
```

For a notification (request has no `id`), Rust confirms delivery with `response: null`; only then does HTTP return an empty 202. A normal JSON response, including any Rust JSON-RPC error, returns HTTP 200 unchanged. A JSON object with `id: null` is treated as expecting a response. Malformed or mismatched bridge envelopes close the socket and fail active calls. Late/unknown correlation IDs are discarded and cannot resolve a different request.

Only one device socket is accepted; additional upgrades receive 409. `acceptWebSocket` enables hibernation, and automatic `ping`/`pong` string replies permit a low-frequency daemon heartbeat without waking the object. No timers or intervals run while idle. Active HTTP calls keep their in-memory correlations until completion, disconnect, or the 240-second deadline; pending calls are not persisted or retried across restarts. The cap is 32 pending requests including notifications; overflow receives 429. Offline and disconnected calls receive 503, deadline expiration 504. Disconnect/timeouts explicitly report an uncertain outcome: the Mac might already have submitted a task. Clients must not automatically retry writes. The Rust ledger and an explicitly reused idempotency key are the recovery mechanism.
