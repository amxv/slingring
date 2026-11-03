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

A transient extension page opens ChatGPT and exchanges messages with a short-lived Rust native host. A content script verifies Chat mode, selects and verifies reasoning, fills the prompt, waits for uploads, and submits once. Local image file inputs are set through a narrow CDP DOM operation. No remote caller can supply file paths.

DOM mutations drive readiness and handoff detection. Stop controls, loading dots, canonical URLs, and empty assistant containers are deliberately insufficient. A matching submitted user bubble plus real thinking/working UI or nonempty response content must be observed. Only a small activity enum and conversation URL leave the page.

## Queue and crash behavior

Only one browser start operation runs at a time. If tasks are already queued, the browser is reused. The last task closes the browser before reporting success. Identifiers are bound to request fingerprints, and a bounded journal records completed, failed, and uncertain states without storing prompt text or answers.

An interrupted task is never silently replayed. A local queue wait does not consume its browser startup budget; remote callers also impose an end-to-end deadline. Configuration changes affect new tasks without mutating an in-flight request's chosen effort or attachments.
