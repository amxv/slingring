# Slingring

Rust CLI/service for Apple Silicon macOS and x86_64 Linux that starts or follows up on ChatGPT Chat conversations and returns their URLs after real assistant activity begins. Do not extract generated answers. Do not use local Codex to delegate this project unless the user explicitly changes that instruction.

## Invariants

- macOS uses the pinned `agent-browser-browse` compatibility baseline. Linux explicitly uses system Google Chrome Stable via its own scoped browser lifecycle and normal private DevTools pipe, with no fork, fallback, fingerprint changes, public debug listener, or sandbox disable. Never attach to normal Chrome windows. Headless is the default; headed mode requires an explicit saved `browser-mode headed` choice. Preserve that choice across local and remote submissions and never change an active task's mode.
- The isolated browser disables unnecessary experimental WebMCP, LAN MediaRouter discovery, default app installation, macOS code-sign app cloning, and its own delayed GoogleUpdater scheduler. App cloning hard-links installed Chrome; the scheduler wakes the global updater, which can modify Chrome and inherit Slingring's responsibility. Task submission must not require that access. Keep these static, audited performance/privacy flags distinct from the fork's fingerprint defaults. Do not disable normal Chrome updates, updater launchd jobs, signature validation or macOS protections.
- New-chat creation must positively verify Chat mode and the selected effort; never submit a new chat in Work or an ambiguous mode. Existing-chat follow-ups use only the exact canonical conversation URL, voice/stop readiness and composer/draft guards, without a homepage visit or Chat/Work switch. On follow-ups, omitted effort must never read, change or block on the model picker. An explicit effort is a best-effort selection before message fill; its unavailable or rehydrating control cannot block an otherwise ready follow-up.
- Subscription is CLI-owned: fresh Plus/High; preserve saved defaults and infer legacy Pro for Extra High/Pro. Store the tier in private `subscription.json` for compatibility with older installed services. Filter live MCP effort enums and menu defaults by tier; reject explicit tier-unavailable effort before submission. Omitted follow-up effort never touches the picker. Record `requested_effort`, `verified_effort`, and `effort_verification` conservatively; legacy records are unverified.
- Request IDs are 1..128 ASCII letters, digits, underscore, hyphen, or colon across all surfaces. Print CLI recovery IDs to stderr early; recover through `jobs --request-id ID --json` or `get_job_status`, never a repeated write.
- Pause uses the same serial queue and exact conversation URL guard. It never sends a message or extracts answers. Confirm `stopped` only after clicking Stop, or `already_idle` when observing an idle turn. Require stable latest-turn completion controls (`evidence: completion_controls`) or its dedicated exact “Stopped thinking” UI status plus adjacent Dictate/Start Voice controls and no Stop/active signals (`evidence: stopped_thinking`). Absence of Stop/voice alone is insufficient; cancellation without either positive indicator remains uncertain. Pending MCP pauses return a generated recovery ID within the five-second receipt window; never add fields to the one-argument tool. Do not use generic body text or content snapshots as pause evidence.
- Stop buttons and loading indicators are not activity. Watch the visible transcript: the current SPA leaves a hidden pre-submit `main` mounted.
- Do not read, log, return, or store generated answer text. The DOM adapter may classify content presence to detect streaming.
- One serial queue, bounded at 32, for local and remote calls. Prompt/message/image content stays in memory. Journal metadata, including follow-up target URLs, is private, bounded, and never automatically replayed after a crash.
- Image paths are CLI-only for both `new-task` and `send-message`. MCP `new_task` accepts `prompt`, optional `reasoning_effort` and optional `request_id`; MCP `send_message` accepts `chatgpt_url`, `message`, optional `reasoning_effort` and optional `request_id`; MCP `pause_task` accepts exactly `chatgpt_url`, never `request_id`; read-only `get_job_status` accepts only `request_id`. Reject all other fields. The local stdio MCP server uses official `rmcp`; keep the existing remote relay compatible. A status query must not launch a browser or submit anything.
- Follow-ups leave reasoning untouched when `reasoning_effort` is omitted, or attempt the explicitly requested value when supplied. They require an exact `https://chatgpt.com/c/` URL with a 1..128-character ASCII alphanumeric/hyphen ID, no query or fragment. Recheck the target before sending. Wait for visible voice readiness without a stop control before filling; never interrupt a response. Detect activity for the newly submitted turn, not historical messages.
- Never retry an uncertain submission. Only close the owned named browser session. Reuse it while queued jobs remain; close it on drain or failure.
- Track safe startup stages and elapsed time in bounded private logs. Never log prompts, URLs, tokens, cookies, generated answers, raw browser output or user-controlled error strings. A missing/evicted journal entry is *unknown*, not evidence of no submission.
- No idle browser, polling daemon, Redis, or `cloudflared`. Remote mode has one Rust runtime thread and outbound WSS; local-only mode exits at idle.
- Menu bar commands wrap the CLI. No independent configuration, network service, timer polling, or embedded browser.
- Linux has no menu/tray app or automatic update scheduler. Optional systemd user service registration is explicit; installer and tests must not change a real user's service settings. Linux requires glibc 2.39 or newer and a separately installed system Google Chrome Stable. Linux's only supported account profile is initialized manually with `profile open` and stays in Slingring's private state; never import normal Chrome profile data. Do not weaken Chrome's sandbox for CI, containers or root execution.

## Source map

`src/queue.rs`: durable metadata, recovery classification and serial ownership. `src/browser.rs`: patched-browser launch, safe stage categories, limited CDP extension installation/upload, cleanup. `src/diagnostics.rs`: bounded, payload-free private lifecycle events. `src/native.rs`: short-lived Native Messaging and owned-profile registration. `src/daemon.rs`: private Unix socket and lifecycle. `src/mcp.rs`: remote-compatible new task, follow-up and status request dispatch; `src/mcp_sdk.rs`: official Rust SDK server for stdio. All tools advertise owner-requested readOnlyHint=true/openWorldHint=false; only two tools send messages; hints never bypass authorization. `src/relay.rs`: outbound WSS. `extension/dom.js`: narrow UI predicates. `relay/`: authenticated transport-only Worker/Durable Object. `apps/menubar/`: AppKit CLI wrapper.

The app-owned Linux profile and isolated macOS Chrome copy each use their own `NativeMessagingHosts` directory. Derive its browser PID through the already-owned CDP browser connection, then verify the configured headed/headless mode and isolated temporary user-data directory. Never guess another browser's profile. Do not enable CDP Runtime event streaming.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
bash apps/menubar/test.sh
# On Linux instead of the macOS-only Swift check:
python3 scripts/test_linux_installers.py
(cd extension && bun install --frozen-lockfile && bun run check && bun run test)
(cd relay && bun install --frozen-lockfile && bun run check && bun run test)
just docs-test
just docs-check
dist generate --check
dist plan
```

Keep Rust files below 1,000 lines. Use the pinned Rust 1.99.1 toolchain. `unsafe_code` is forbidden. Commit Cargo and Bun lockfiles. Real ChatGPT tests create real chats and must be authorized; fixture tests never need account access.

## Deployment and release

Direct installers and upgrade checks must support authenticated `gh` downloads while the repository is private and anonymous public-release downloads when public. Release targets are macOS ARM64 and Linux x86_64 GNU (glibc 2.39+); do not promise Intel macOS, Linux ARM64 or Windows. No npm or crates.io publication. Regenerate `.github/workflows/release.yml` with pinned cargo-dist 0.32.0, never manually edit it. Read `.agents/skills/release/SKILL.md` for releases. Keep the macOS CLI and optional ad-hoc-signed app adjacent. Never disable Gatekeeper or strip quarantine. The public relay Wrangler template must not contain an account ID, zone ID, or maintainer hostname; production deployment requires explicit GitHub Actions secrets for the existing account, zone and hostname, while preserving Worker identity, custom domain and other credentials.

Docs remain Astro/ZueDocs. Vercel root is `docs`; Git automatic deployments stay disabled. The docs-only GitHub Action builds in `docs`, copies the prebuilt bundle to repository-root `.vercel/output`, then deploys from root. Validate with Astro check locally, not redundant production builds.

Credentials belong in local private configuration, Cloudflare secrets, or GitHub secrets. Never print tokens, authentication files, browser cookies, or capability URLs. Keep scratch files in ignored `tmp/gg/`. Do not use a local agent delegation harness.

Profile discovery and selection belong in the Rust CLI; the menu bar only invokes it. The embedded macOS browser installer installs a version-pinned, checksummed private helper. Linux `browser install` checks the separately installed stock Chrome executable but does not install or change it. Maintain scripts/test_installers.py coverage without touching a real user installation.
