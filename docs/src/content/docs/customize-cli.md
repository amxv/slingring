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

The `send_message(chatgpt_url, message, reasoning_effort?)` MCP tool and `slingring send-message` CLI follow the same browser and native-host lifecycle. The CLI can attach local images with repeatable `--image PATH`; MCP remains text-only. Slingring opens the exact canonical `/c/` conversation URL directly and never detours through the homepage or switches Chat/Work mode. Omitted follow-up effort never reads or changes the model picker. An explicit level is a best-effort change before filling; a missing or rehydrating picker does not block the send. The follow-up waits for visible voice readiness with no stop control before filling, then rechecks the exact target and stop state after native authorization. It never interrupts the existing response or submits a new conversation when the target is unavailable.

DOM mutations drive readiness and handoff detection. Stop controls, loading dots, canonical URLs, and empty assistant containers are deliberately insufficient. A newly submitted matching user bubble plus real thinking/working UI or nonempty response content for that turn must be observed. Historical turns, including repeated prompt text, cannot satisfy follow-up start detection. Generated answer text is never serialized, logged, stored, or returned; the result contains a conversation URL and small job/effort/evidence metadata.

## Queue and crash behavior

Both **write tools** share one serial queue with capacity 32. The read-only `get_job_status` bypasses submission waits and reads private metadata. MCP callers receive the final result if it arrives within five seconds after admission; otherwise they receive an accepted `in_progress` receipt and recover using the same request ID. Accepted MCP jobs outlive their caller's connection but keep their payload only in memory. Direct CLI callers still wait for the confirmed result, and abandoned queued CLI tasks are skipped.

If compatible write tasks are already queued, the browser is reused. A confirmed result is journaled and returned before the final bounded browser cleanup; `browser_busy` can therefore remain true briefly with no active tasks. The next task still waits for cleanup. Changing the saved mode, profile or helper does not interrupt the active task; the owned browser is reopened before the next job that needs different settings. Identifiers are bound to the operation and request fingerprint, including the follow-up URL and message. A bounded journal records target URLs, stages and completed, failed, and uncertain states without storing prompt/message text or answers. The browser reports only static lifecycle stages and elapsed times into a private bounded `events.log`; it never records raw browser command output or credentials.

An interrupted task is never silently replayed. A local queue wait does not consume its default 180-second browser startup budget; follow-up readiness does. Remote callers also impose an end-to-end deadline that includes queue waiting. A busy follow-up can delay the queue, and the queue cannot exclude writes from a person or another client. Configuration changes affect new tasks without mutating an in-flight request's chosen effort or attachments.

Ownership locks distinguish a live queue from a journal left behind by a crash. When no owner remains, queued records are reported as definitely not submitted and starting records as uncertain, without launching a daemon or replaying content. A service that holds its lock but fails to answer status is reported as unavailable, not stopped or idle. The relay reserves bounded status/control capacity separately from writes so a saturated submission path does not block recovery.
