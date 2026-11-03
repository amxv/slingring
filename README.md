# Slingring

**Let an agent start a ChatGPT chat for you. Get the link back. Keep working.**

Slingring accepts a prompt, opens your signed-in Chrome profile in an isolated headless session, verifies **Chat** mode, selects the requested reasoning effort, and sends. Once genuine thinking, working, or response activity starts, it returns the conversation URL and closes the browser. It does not retrieve the answer.

```bash
slingring new-task --prompt-file ./brief.txt --reasoning-effort "Extra High"
slingring new-task "Explain this diagram" --image ./diagram.png --json
```

## Two ways in, one queue

Local agents use the CLI. Remote agents use a single Streamable HTTP MCP tool:

```text
new_task(prompt: string, reasoning_effort?: string)
```

Image files are **CLI-only**. The MCP tool rejects file paths and unknown arguments.

Both entry points feed one bounded serial queue on your Mac. Calls can arrive together; browsers never race to submit them. The browser stays warm for queued work and exits when the queue drains. Request IDs prevent duplicate submissions within the retained journal; uncertain jobs are never automatically replayed.

## Small when idle

Local-only use leaves no Slingring service, browser, or agent-browser process running between requests. Remote mode keeps one single-threaded Rust service and one outbound TLS WebSocket to a Cloudflare Worker/Durable Object relay. The relay supports WebSocket hibernation. There is no Redis, `cloudflared`, inbound Mac listener, job polling, or always-open browser.

The optional Swift/AppKit menu bar app wraps CLI commands and refreshes when its menu opens. It has no WebView or polling timer. Leave it closed for the smallest idle footprint.

## Install

Slingring currently supports **macOS**. This repository is private; authenticate `gh` with repository access before downloading releases.

```bash
curl --proto '=https' --tlsv1.2 -fsSL https://slingring.ashray.xyz/install.sh -o /tmp/slingring-install.sh
sh /tmp/slingring-install.sh
rm /tmp/slingring-install.sh
slingring setup --profile "Profile 6" --default-effort "Extra High"
```

The signed CLI and optional menu bar bundle install together. The menu bar does not launch automatically on first installation. Ad-hoc signing is not Developer ID notarization.

Install the tested [agent-browser-browse](https://github.com/amxv/agent-browser-browse) build separately and sign into ChatGPT in your chosen Chrome profile. Slingring deliberately does not substitute upstream `agent-browser`, attach to your everyday window, or silently bypass a browser challenge.

## Reasoning and remote access

```bash
slingring config set default-effort "Extra High"
slingring new-task "Check this reasoning" --reasoning-effort Pro
slingring status --json
slingring jobs --json
slingring remote url   # Prints a credential-bearing connection URL; keep private.
```

Supported efforts are **Instant, Medium, High, Extra High, Pro**. A blank or omitted value uses your configured default. Unsupported account options fail before submission instead of silently downgrading. `default-effort site` preserves the site's current choice.

Deploy `relay/`, configure separate device/client credentials, enable remote mode, and install the login service as described in the [remote setup guide](https://slingring.ashray.xyz/docs/remote). The Mac must be awake, logged in, online, and able to use its authenticated browser profile. Remote callers receive an explicit offline error when it is unavailable.

## Reliability and privacy

Slingring verifies Chat mode both before preparation and immediately before sending. A stop button, spinner, loading dot, or empty assistant placeholder is **not** start confirmation. Only a canonical chat URL and genuine assistant activity qualify.

Prompts and images go to ChatGPT; remote prompts also transit your relay. The local journal stores status, request fingerprints, timestamps, and chat URLs, not prompts, image paths, cookies, or generated answers. Credentials and local state have private permissions. Treat the remote URL as a password.

Browser interfaces and detection systems change. The patched fork is the tested baseline, not a promise of universal undetectability. Login challenges and unknown UI states fail closed. Inspect an `uncertain` job before retrying.

## Development

Created from `amxv/rust-cli-template`; Rust **1.99.1**, direct macOS distribution, no npm publishing.

```bash
just check
(cd extension && bun install --frozen-lockfile && bun run test)
(cd relay && bun install --frozen-lockfile && bun run check && bun run test)
just docs-test
just docs-check
dist generate --check
dist plan
```

`src/` owns the CLI, queue, service, MCP semantics, native host, and browser lifecycle. `extension/` owns the Chat UI adapter. `relay/` is transport only. `apps/menubar/` is the optional native wrapper. `docs/` retains Astro/ZueDocs and deploys only for docs changes.
