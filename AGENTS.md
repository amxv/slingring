# Slingring

Mac-only Rust CLI/service that starts or follows up on ChatGPT Chat conversations and returns their URLs after real assistant activity begins. Do not extract generated answers. Do not use local Codex to delegate this project unless the user explicitly changes that instruction.

## Invariants

- Use the pinned `agent-browser-browse` compatibility baseline. Never silently fall back to upstream, attach to normal Chrome windows, change fingerprint flags, or use headed mode without approval.
- Chat mode must be positively verified. Never submit in Work or an ambiguous mode. Requested effort must be read back; never silently downgrade account-restricted levels.
- Stop buttons and loading indicators are not activity. Watch the visible transcript: the current SPA leaves a hidden pre-submit `main` mounted.
- Do not read, log, return, or store generated answer text. The DOM adapter may classify content presence to detect streaming.
- One serial queue, bounded at 32, for local and remote calls. Prompt/message/image content stays in memory. Journal metadata, including follow-up target URLs, is private, bounded, and never automatically replayed after a crash.
- Image paths are CLI-only for both `new-task` and `send-message`. MCP `new_task` accepts `prompt`, optional `reasoning_effort` and optional `request_id`; MCP `send_message` accepts `chatgpt_url`, `message`, and optional `request_id`; read-only `get_job_status` accepts only `request_id`. Reject all other fields. The local stdio MCP server uses official `rmcp`; keep the existing remote relay compatible. A status query must not launch a browser or submit anything.
- Follow-ups preserve the chat's reasoning effort and require an exact `https://chatgpt.com/c/` URL with a 1..128-character ASCII alphanumeric/hyphen ID, no query or fragment. Recheck the target before sending. Wait for visible voice readiness without a stop control before filling; never interrupt a response. Detect activity for the newly submitted turn, not historical messages.
- Never retry an uncertain submission. Only close the owned named browser session. Reuse it while queued jobs remain; close it on drain or failure.
- Track safe startup stages and elapsed time in bounded private logs. Never log prompts, URLs, tokens, cookies, generated answers, raw browser output or user-controlled error strings. A missing/evicted journal entry is *unknown*, not evidence of no submission.
- No idle browser, polling daemon, Redis, or `cloudflared`. Remote mode has one Rust runtime thread and outbound WSS; local-only mode exits at idle.
- Menu bar commands wrap the CLI. No independent configuration, network service, timer polling, or embedded browser.

## Source map

`src/queue.rs`: durable metadata, recovery classification and serial ownership. `src/browser.rs`: patched-browser launch, safe stage categories, limited CDP extension installation/upload, cleanup. `src/diagnostics.rs`: bounded, payload-free private lifecycle events. `src/native.rs`: short-lived Native Messaging and owned-profile registration. `src/daemon.rs`: private Unix socket and lifecycle. `src/mcp.rs`: remote-compatible new task, follow-up and status request dispatch; `src/mcp_sdk.rs`: official Rust SDK server for stdio. All tools advertise owner-requested readOnlyHint=true/openWorldHint=false; only two tools send messages; hints never bypass authorization. `src/relay.rs`: outbound WSS. `extension/dom.js`: narrow UI predicates. `relay/`: authenticated transport-only Worker/Durable Object. `apps/menubar/`: AppKit CLI wrapper.

The Chrome copy uses its own `NativeMessagingHosts` directory. Derive its browser PID through the already-owned CDP browser connection, then verify the headless process and isolated temporary user-data directory. Never guess another browser's profile. Do not enable CDP Runtime event streaming.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
(cd extension && bun install --frozen-lockfile && bun run check && bun run test)
(cd relay && bun install --frozen-lockfile && bun run check && bun run test)
just docs-test
just docs-check
dist generate --check
dist plan
```

Keep Rust files below 1,000 lines. Use the pinned Rust 1.99.1 toolchain. `unsafe_code` is forbidden. Commit Cargo and Bun lockfiles. Real ChatGPT tests create real chats and must be authorized; fixture tests never need account access.

## Deployment and release

The GitHub repository starts private. Direct installers and upgrade checks must support authenticated `gh` downloads. macOS ARM64 and Intel only; no npm or crates.io publication. Regenerate `.github/workflows/release.yml` with pinned cargo-dist 0.32.0, never manually edit it. Read `.agents/skills/release/SKILL.md` for releases. Keep CLI and optional ad-hoc-signed app adjacent. Never disable Gatekeeper or strip quarantine.

Docs remain Astro/ZueDocs. Vercel root is `docs`; Git automatic deployments stay disabled. The docs-only GitHub Action builds in `docs`, copies the prebuilt bundle to repository-root `.vercel/output`, then deploys from root. Validate with Astro check locally, not redundant production builds.

Credentials belong in local private configuration, Cloudflare secrets, or GitHub secrets. Never print tokens, authentication files, browser cookies, or capability URLs. Keep scratch files in ignored `tmp/gg/`. Do not use a local agent delegation harness.

Profile discovery and selection belong in the Rust CLI; the menu bar only invokes it. The embedded browser installer installs a version-pinned, checksummed private helper. Maintain scripts/test_installers.py coverage without touching a real user installation.
