# Slingring architecture

## Process ownership

The Rust CLI is the source of truth. It owns configuration, the task journal, the serial queue, browser lifecycle, Native Messaging transport, MCP semantics, service installation, and updates. The Swift menu bar only invokes CLI commands.

Local-only mode has no resident Slingring process between tasks. Remote mode keeps one current-thread Rust service with one outbound TLS WebSocket. There is no idle Chrome, extension worker, native host, JavaScript runtime, cloudflared process, or job-polling timer. The optional menu bar is independent and refreshes only on interaction.

## Browser boundary

The configured `agent-browser-browse` fork remains the browser baseline. Every command uses identical launch configuration. Slingring does not override the fork's fingerprint settings, enable Runtime telemetry, or fall back to upstream automation on failure.

The fork launches a temporary copy of the authorized Chrome profile. Slingring checks that the owned process matches the configured headed/headless mode and its user-data directory is an isolated `agent-browser-profile-*` directory before registering its native host there. The user's ordinary profile and browser windows are not modified or closed.

Browser-level CDP installs the embedded extension and performs narrow file-input operations for local image uploads. The extension performs ChatGPT UI interaction. It communicates over a transient native-host process and a private Unix socket. Prompts, follow-up message text, image contents, and authentication cookies never enter browser launch arguments or URLs.

## Submission state machine

`queued → preparing → verified Chat + effort + prompt + uploads → will_submit → submitted → actual activity + canonical URL → started`

The extension sends once. Chat mode is checked before preparing and immediately before submission. New tasks verify reasoning selection against the account's accessible slider labels and numeric bounds. Unsupported levels fail rather than downgrade.

For `send_message`, preparation opens the exact requested conversation directly, without a homepage visit or Chat/Work switch. Omitted effort never reads, changes, or blocks on the picker. An explicit tier-allowed effort is a best-effort change before filling; a missing or rehydrating picker does not block an otherwise ready follow-up. Only `https://chatgpt.com/c/` URLs with a 1..128-character ASCII alphanumeric/hyphen ID and no query or fragment are accepted. The extension verifies the target, waits for a visible voice control without a stop control, and then fills the message. It rechecks the target and absence of a stop control before sending, including after native authorization. A busy chat or unsupported layout fails without sending if readiness is not reached within the existing startup budget; it never clicks Stop.

The start detector requires a visible cleared composer and anchors to the newly submitted user's bubble inside the active transcript. Follow-ups exclude historical turns, including previous copies of the same message, and require the prior visible user-node sequence to remain identifiable. A layout that replaces or virtualizes that history fails closed. It accepts real thinking/working status or nonempty assistant response content for the new turn. A Stop button, pulsing dot, screen-reader-only loading label, newly assigned URL, or empty assistant shell is insufficient. Generated text is tested only for presence; it is never serialized, logged, stored, or returned.

`pause_task` opens the exact conversation and may click its Stop control, without sending a message. A pause reports `pause_status: stopped` after clicking Stop, or `already_idle` when observing an idle turn. Confirmation requires stable latest-turn completion controls (`evidence: completion_controls`), or its dedicated exact “Stopped thinking” UI status plus adjacent Dictate/Start Voice controls and no Stop/active signals (`evidence: stopped_thinking`). Missing Stop/voice controls alone do not prove completion. Cancellation without either positive indicator remains uncertain, including a thinking cancellation with neither completion footer nor verified stopped-thinking UI. Pause never reads generic body text, takes content snapshots, or extracts generated responses.

All three operations share one serial queue bounded at 32. Already queued tasks reuse the browser. Confirmed results can be returned before final bounded browser cleanup; subsequent tasks still respect serial browser ownership. A local queue wait does not consume the default 180-second browser startup budget; waiting for follow-up readiness does. Remote tasks additionally respect their end-to-end HTTP deadline, including queue wait. A busy follow-up can delay later requests. This queue cannot lock out a person or another client writing to the same chat.

## Subscription and effort

Fresh configuration uses Plus with High reasoning. Plus allows Instant, Medium, and High; Pro also allows Extra High and Pro. Saved defaults are preserved, with legacy Extra High/Pro defaults inferring Pro when no tier is saved. A private `subscription.json` sidecar avoids changing the configuration format read by older installed services. The CLI owns this setting, the menu wraps it, and live MCP effort enums reflect it. Explicit tier-unavailable effort is rejected before submission. `requested_effort`, `verified_effort`, and `effort_verification` distinguish intent from observation; legacy records without verification remain unverified.

## Failure semantics

Request IDs are 1..128 ASCII letters, digits, underscore, hyphen, or colon on every surface. The CLI prints its recovery ID to stderr before waiting. Use `jobs --request-id ID --json` or `get_job_status` to recover, never a repeated write.

The private bounded journal stores identifiers, request fingerprints, timestamps, state, and conversation URLs, including a follow-up's target. It does not retain raw prompts, messages, or images. A reused identifier must match the original operation, resolved request, profile, and image contents or follow-up URL/message. Completed requests return their prior result; uncertain requests do not run again.

An abandoned queued direct CLI task is canceled before launch; accepted MCP jobs continue in memory after caller disconnection. A crash or disconnect after possible submission produces an uncertain result, optionally with a known conversation URL. Nothing is automatically replayed after restart or reconnection.

## Remote boundary

A Cloudflare Worker exposes Streamable HTTP MCP and authenticates requests with a separate client credential. Its hibernating Durable Object holds the authenticated device WebSocket and forwards bounded requests in memory. It does not persist prompts/messages or queue work while the Mac is offline. The Rust service owns all MCP validation and task policy.

MCP exposes `new_task(prompt, reasoning_effort?, request_id?)`, `send_message(chatgpt_url, message, reasoning_effort?, request_id?)`, `pause_task(chatgpt_url)`, and read-only `get_job_status(request_id)`. All reject unknown fields. Pause accepts no caller request ID; its pending receipt provides a generated ID for status recovery. MCP writes return a result or pending receipt within the five-second post-admission window. Follow-ups and pauses are also available through the CLI. Message text is limited to 128 KiB; local image paths are CLI-only for new tasks and follow-ups. Owner-requested `readOnlyHint: true` / `openWorldHint: false` annotations remain hints, not authorization: only status is read-only. The device and client tokens must be distinct. A private URL may carry the client token for assistants without custom header support; it must be treated as a secret.

Transport health uses a sparse 60-second heartbeat and bounded reconnect backoff, not job polling. Mac sleep, logout, network loss, expired ChatGPT login, UI changes, or browser challenges may prevent a task from starting. No universal undetectability or service-terms guarantee is claimed.
