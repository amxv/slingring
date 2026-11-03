# Slingring architecture

## Process ownership

The Rust CLI is the source of truth. It owns configuration, the task journal, the serial queue, browser lifecycle, Native Messaging transport, MCP semantics, service installation, and updates. The Swift menu bar only invokes CLI commands.

Local-only mode has no resident Slingring process between tasks. Remote mode keeps one current-thread Rust service with one outbound TLS WebSocket. There is no idle Chrome, extension worker, native host, JavaScript runtime, cloudflared process, or job-polling timer. The optional menu bar is independent and refreshes only on interaction.

## Browser boundary

The configured `agent-browser-browse` fork remains the browser baseline. Every command uses identical launch configuration. Slingring does not override the fork's fingerprint settings, enable Runtime telemetry, or fall back to upstream automation on failure.

The fork launches a temporary copy of the authorized Chrome profile. Slingring checks that the owned process is headless and its user-data directory is an isolated `agent-browser-profile-*` directory before registering its native host there. The user's ordinary profile and browser windows are not modified or closed.

Browser-level CDP installs the embedded extension and performs narrow file-input operations for local image uploads. The extension performs ChatGPT UI interaction. It communicates over a transient native-host process and a private Unix socket. Prompts, image contents, and authentication cookies never enter browser launch arguments or URLs.

## Submission state machine

`queued → preparing → verified Chat + effort + prompt + uploads → will_submit → submitted → actual activity + canonical URL → started`

The extension sends once. Chat mode is checked before preparing and immediately before submission. Reasoning selection is verified against the account's accessible slider labels and numeric bounds. Unsupported levels fail rather than downgrade.

The start detector anchors to the submitted user's bubble inside the active transcript. It accepts real thinking/working status or nonempty assistant response content. A Stop button, pulsing dot, screen-reader-only loading label, newly assigned URL, or empty assistant shell is insufficient. Generated text is tested only for presence; it is never serialized, logged, stored, or returned.

Only one task starts at a time. Already queued tasks reuse the browser. The last task closes the browser before notifying its caller. A local queue wait does not consume the browser startup budget; remote tasks additionally respect their HTTP request deadline.

## Failure semantics

The private bounded journal stores identifiers, request fingerprints, timestamps, state, and conversation URLs. It does not retain raw prompts or images. A reused identifier must match the original resolved request, profile, and image contents. Completed requests return their prior result; uncertain requests do not run again.

Caller disconnection cancels a queued task before launch. A crash or disconnect after possible submission produces an uncertain result, optionally with a known conversation URL. Nothing is automatically replayed after restart or reconnection.

## Remote boundary

A Cloudflare Worker exposes Streamable HTTP MCP and authenticates requests with a separate client credential. Its hibernating Durable Object holds the authenticated device WebSocket and forwards bounded requests in memory. It does not persist prompts or queue work while the Mac is offline. The Rust service owns all MCP validation and task policy.

The remote tool accepts exactly `prompt` and optional `reasoning_effort`. Filesystem paths and image attachments are CLI-only. The device and client tokens must be distinct. A private URL may carry the client token for assistants without custom header support; it must be treated as a secret.

Transport health uses a sparse 120-second heartbeat and bounded reconnect backoff, not job polling. Mac sleep, logout, network loss, expired ChatGPT login, UI changes, or browser challenges may prevent a task from starting. No universal undetectability or service-terms guarantee is claimed.
