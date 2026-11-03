---
title: Development and validation
description: Implementation details for maintainers and integrators.
order: 25
category: Reference
---

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cd relay
bun install --frozen-lockfile
bun run check
bun run test
bun run dry-run
```

Rust tests cover validation, idempotency, serialization, queue cancellation, crash recovery, safe CLI inspection, and the text-only MCP boundary. Follow-up cases check exact conversation URLs, message bounds, shared queue behavior, and target/message/operation-bound IDs. DOM fixtures cover Chat/Work state, account reasoning levels, voice/stop readiness, target changes, fresh-turn evidence, repeated historical prompts, and rejection of loading indicators. Relay tests cover authentication, Origin checks, bounded JSON, deadlines, disconnects, and pending-call limits.

For a live browser acceptance test, use a dedicated `SLINGRING_HOME`, the tested browsing fork, and an explicitly authorized signed-in profile. Submit harmless prompts and use a dedicated test conversation for follow-ups. Inspect only UI state, activity booleans, and canonical URLs. Never dump response bodies, browser cookies, credentials, or generated assistant text into test artifacts. Fixture coverage does not establish compatibility with every live account or layout.

Use the same complete runner flags and environment on every command in a named browser session. Changing agent-browser launch configuration during a task can restart its background process and invalidate the native connection. Never close other agents' browser sessions.

The documentation site retains Astro/ZueDocs. Its `docs/**` workflow builds in GitHub Actions and deploys a prebuilt bundle to Vercel. Local docs validation is `bun run test:vercel` followed by `bun run check`; no custom documentation renderer is used.
