---
title: Changelog
description: Release status and user-visible changes.
order: 99
category: Reference
summary: Release status and user-visible changes.
---

## Unreleased

Follow-ups now open the exact existing conversation URL directly rather than navigating through the ChatGPT homepage or switching Chat/Work mode first. Omitted follow-up `reasoning_effort` skips all model-picker reads, changes and blocks. An optional explicit level on CLI and MCP follow-ups attempts to select the desired reasoning effort before filling; an unavailable or rehydrating picker does not prevent submission. Follow-ups retain strict voice/stop readiness, target-URL binding, user-draft protection, one-shot submission and fresh-activity confirmation. Record only allowlisted requested/observed effort metadata when an explicit level was supplied, never model output or picker text. No new-chat selection behavior changes.

## 0.4.0 · 3 November 2026

Add read-only recovery with `request_id` and `get_job_status`, bounded private startup-stage/timing diagnostics, and accepted MCP receipts after a five-second result wait. Accepted jobs continue in memory after caller disconnects. Reserve status capacity when submissions are saturated, classify interrupted jobs conservatively before restart, and preserve confirmed outcomes without waiting for browser cleanup. A timeout or missing record never proves that a message was unsent, and interrupted work is never replayed.

Use the official `rmcp` Rust SDK for local stdio MCP protocol handling and transport, preserving the existing remote relay integration and strict text-only MCP boundary. Add `slingring send-message` with local image attachments, file/stdin message input, and request-ID recovery. Follow-up images use the existing isolated browser upload path, with readiness and unchanged-conversation checks before submission. No generated answers are extracted.

Preserve rich-text line breaks during prompt verification, reject existing or changed drafts, bind uploads to the exact owned browser target, and wait for uploads without confusing their progress with an active response. Require identifiable, stable attachment cards before sending; a layout with only Remove buttons is rejected rather than risking a changed sibling preview. Keep submission evidence separate from cleanup and report missing composers clearly.

Add persistent headless/headed selection through `config set browser-mode` and Configure → Browser Mode. Headless remains the default. Show current preparation phases and queued jobs separately from the last confirmed, failed, or uncertain attempt. Protect read-only crash recovery by holding the daemon and queue ownership locks across its journal snapshot; never claim a queued job was unsent when ownership cannot be verified.

## 0.3.0 · 3 November 2026

Add the MCP-only `send_message(chatgpt_url, message)` tool for continuing an existing ChatGPT Chat conversation using the same isolated browser, Chrome profile, and serial queue as `new_task`. A follow-up waits for the voice control to signal readiness, never interrupts an ongoing response, preserves the current reasoning effort, and returns the same conversation URL once new thinking, working, or response activity begins. Generated answers are not extracted.

Protect follow-ups from invalid or redirected URLs, existing drafts and attachments, Work mode, busy conversations, and misleading historical activity. A failed or uncertain submission is not automatically retried; follow-up messages remain limited to 128 KiB. Add regression coverage for queueing, duplicate protection, readiness, and real start detection.

## 0.2.0 · 3 November 2026

Install the verified browser helper automatically with Slingring. Add named Chrome profile selection in the CLI and menu bar. Move settings into Configure, retaining service control, latest chat, and quit at the top level. Advertise the requested MCP hint values while retaining explicit tool behavior.

Keep the browser's crash backstop longer than the configured task deadline without changing launch options as queued requests age. Retry a model-menu click that races page hydration, while retaining the one-shot submission guard and strict effort verification.

## 0.1.1 · 3 November 2026

Harden browser startup and reasoning-picker hydration, anchor activity detection to the active transcript, and reject loading-only states. Preserve a full per-task startup budget for locally queued requests, close a warm browser when abandoned queued work drains, and use smaller idle WebSocket buffers. Add regression fixtures, private-release installation support, and automatic relay-code deployment.

## 0.1.0 · 3 November 2026

Initial private macOS release. Local CLI and remote Streamable HTTP MCP share one serial queue and a patched-browser runner. The CLI supports local image attachments; the single remote tool accepts only prompt and reasoning effort.

The browser adapter verifies Chat mode, supports all five reasoning levels when the account exposes them, and returns the conversation URL only after real assistant activity. It handles the current retained home view, user-bubble, and assistant Markdown structure. Stop controls, pulsing dots, and screen-reader loading text do not count as a successful handoff.

Remote mode uses a single outbound Rust WebSocket, a hibernating Cloudflare relay, sparse connection heartbeats, and no Redis or idle browser. Local-only mode exits when idle. The optional signed Swift menu bar wraps CLI commands and has no status polling timer.

The private installer supports authenticated GitHub downloads, paired CLI/app archives, checksum verification, and explicit update preferences. Private release artifacts still require repository access; there is no anonymous release download or npm package.
