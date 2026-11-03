---
title: Architecture and idle behavior
description: Implementation details for maintainers and integrators.
order: 22
category: Reference
---

## Local-only idle

No Slingring process is required between tasks. A CLI request connects to a private Unix socket or starts the Rust daemon on demand. The daemon owns one bounded FIFO queue. It exits after the queue drains, the browser closes, and a short idle grace period passes.

## Remote idle

One current-thread Tokio runtime keeps an outbound TLS WebSocket. Small socket buffers and sparse connection heartbeats replace periodic queue requests. The browser, extension, native host, and CLI child processes do not remain running. The optional menu bar is a separate, user-chosen process with no status timer.

Cloudflare hosts the public Streamable HTTP endpoint and a hibernating Durable Object. The relay authenticates both sides, forwards JSON in memory, and returns results. There is no database to provision or cloud backlog to accidentally replay after the Mac reconnects.

## During a task

The patched `agent-browser-browse` launches its isolated copy of the selected Chrome profile using its existing defaults. Slingring attaches only to that owned browser endpoint, installs the bundled extension through Chrome's extension API, and registers Native Messaging in the temporary browser data directory. It does not enable CDP Runtime telemetry or take over ordinary Chrome windows.

A transient extension page opens ChatGPT and exchanges messages with a short-lived Rust native host. For a new task, a content script verifies Chat mode, selects and verifies reasoning, fills the prompt, waits for uploads, and submits once. Local image file inputs are set through a narrow CDP DOM operation. No remote caller can supply file paths.

The MCP-only `send_message(chatgpt_url, message)` follows the same browser and native-host lifecycle. It first verifies Chat mode on the home page, then opens an exact canonical `/c/` conversation URL in the same tab. This also supports profiles that initially open in Work. It preserves the current effort and waits for visible voice readiness with no stop control before filling. It rechecks the target and stop state before sending, including after the native authorization acknowledgement. It never interrupts the existing response or submits a new conversation when the target is unavailable.

DOM mutations drive readiness and handoff detection. Stop controls, loading dots, canonical URLs, and empty assistant containers are deliberately insufficient. A newly submitted matching user bubble plus real thinking/working UI or nonempty response content for that turn must be observed. Historical turns, including repeated prompt text, cannot satisfy follow-up start detection. Generated answer text is never serialized, logged, stored, or returned; the result contains a conversation URL and small job/effort/evidence metadata.

## Queue and crash behavior

Both tools share one serial queue with capacity 32. If tasks are already queued, the browser is reused. The last task closes the browser before reporting success. Identifiers are bound to the operation and request fingerprint, including the follow-up URL and message. A bounded journal records target URLs and completed, failed, and uncertain states without storing prompt/message text or answers.

An interrupted task is never silently replayed. A local queue wait does not consume its default 180-second browser startup budget; follow-up readiness does. Remote callers also impose an end-to-end deadline that includes queue waiting. A busy follow-up can delay the queue, and the queue cannot exclude writes from a person or another client. Configuration changes affect new tasks without mutating an in-flight request's chosen effort or attachments.
