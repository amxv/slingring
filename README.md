# Slingring

Start a ChatGPT task from another agent. Get the conversation link back. Let ChatGPT do the work.

Slingring is a macOS Rust CLI with an optional Swift menu bar app and a small Chrome extension. Local agents use the CLI. Remote agents use one authenticated Streamable HTTP MCP tool. Both share one queue and one browser runner.

```bash
slingring setup --profile 'Profile 6' --default-effort 'Extra High'
slingring new-task 'Review this migration plan' --json
slingring new-task --prompt-file brief.md --image mockup.png --reasoning-effort High
```

The successful result contains a canonical `https://chatgpt.com/c/...` URL. It means genuine thinking, working, or response streaming was observed. A loading dot, Stop button, or new URL alone is not enough. Slingring does not retrieve or return the generated answer.

## Local when possible. Reachable when needed.

In local-only mode, the CLI starts a short-lived Rust service on demand. It exits after the queue drains and its browser closes. With remote access enabled, the same service holds one outbound TLS WebSocket to a Cloudflare Worker relay. It sleeps between pushed requests and sparse connection heartbeats. No browser stays running at idle, and there is no homebox, Redis, polling queue, or tunnel daemon.

The optional native menu bar invokes CLI commands. It refreshes only when opened or used, not on a status-polling timer.

## Browser prerequisites

Install the tested [agent-browser-browse](https://github.com/amxv/agent-browser-browse) fork and stable Google Chrome. Sign into ChatGPT in the Chrome profile you configure. Slingring uses the fork's isolated temporary profile copy, not your everyday Chrome windows. It keeps the fork's browser launch defaults unchanged and never falls back silently to upstream agent-browser.

The extension is bundled into the Rust executable and loaded into the owned browser on demand. Native Messaging registration is confined to that temporary browser data directory. Setup does not install an extension into your everyday Chrome profile.

Browser compatibility is measured, not guaranteed. The upstream browsing-fork experiments passed the tested fingerprint checks and Cloudflare challenge, but did not defeat every protection system. ChatGPT UI, authentication, or service changes can still require an adapter update.

## Reasoning and images

Choose `Instant`, `Medium`, `High`, `Extra High`, or `Pro`. Omission or an empty value uses the configured default. Extra High and Pro require an account that exposes them. Unsupported or ambiguous selections fail before sending; no silent downgrade occurs.

```bash
slingring config set default-effort 'Extra High'
slingring config set default-effort site  # preserve ChatGPT's current setting
```

Repeated `--image` flags attach local PNG, JPEG, GIF, or WebP files. Images and local file paths are CLI-only. The remote tool accepts exactly `prompt` and optional `reasoning_effort`.

## Remote access

Deploy `relay/` to your Cloudflare account. Provision distinct device and client credentials. The relay forwards requests in memory and uses a hibernating Durable Object for the Mac's WebSocket. It does not durably queue prompts while your Mac is offline.

```bash
# Supply the device token from your secret manager, not as a command argument.
printf '%s' "$DEVICE_TOKEN" | slingring remote configure \
  --url wss://YOUR_RELAY/device --token-stdin
slingring remote on
slingring service install
```

Use `https://YOUR_RELAY/mcp` with the client Bearer token. For a client that cannot set headers, use the private connection URL `https://YOUR_RELAY/mcp/CLIENT_TOKEN` and select **No authentication** in the client. The URL itself is the credential. Never share it or place it in documentation. `slingring remote url` prints a configured private URL only when explicitly invoked.

The Mac must be awake, logged in, online, and able to use its ChatGPT session. Remote calls fail explicitly when the device is offline.

## Installation and development

The repository is initially private. Release downloads require GitHub access; anonymous GitHub download URLs will not work. With an authenticated `gh` CLI, use the website installer after the first release, or build from source:

```bash
gh repo clone amxv/slingring
cd slingring
cargo install --locked --path .
slingring setup --profile 'Profile 6'
```

Direct macOS releases include Apple Silicon and Intel CLI archives and paired, ad-hoc-signed menu bar bundles. There is no npm package. The app is optional, and no notarization or Gatekeeper bypass is claimed.

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cd relay && bun install --frozen-lockfile && bun run check && bun run test
```

The docs retain Astro/ZueDocs. Their path-filtered GitHub workflow deploys to Vercel; Rust-only changes do not request a docs deployment.

## Failure handling

Concurrent requests queue, but only one task starts at a time. A pending batch reuses the browser. Once the queue drains, the owned browser closes before the final caller receives success.

Use `--request-id` to recover the same request without resubmitting. An ID is bound to the prompt, effort, profile, and image contents. Reusing it with different input is rejected. If submission becomes uncertain, Slingring records that uncertainty and does not automatically retry. Inspect `slingring jobs --json` and ChatGPT before issuing a new request.

The bounded private journal stores hashes, statuses, timestamps, and chat URLs, not prompt text, image contents, or assistant answers. Credentials remain in the local private configuration and Cloudflare secrets. See the [documentation](https://slingring.ashray.xyz/docs/) for operational details.

Apache-2.0.
