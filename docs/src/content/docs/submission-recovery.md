---
title: Submission readiness and recovery
description: Distinguish queue receipts, safe preparation failures, and uncertain submissions.
order: 14
category: Reference
---

A queued or starting receipt is not proof that ChatGPT received a message. Before authorizing Send, Slingring persists a submission-intent marker. If that save fails, Send is not authorized. A new record interrupted before that marker is definitely not submitted. An interrupted legacy starting record, or one that crossed the marker without confirmed new activity, remains uncertain. No interrupted request is replayed.

Use `slingring jobs --request-id ID --json` or MCP `get_job_status` to recover the original outcome. Do not repeat a write to check whether it finished. A confirmed submission requires positive assistant activity, not simply a busy button or a queue receipt.

Composer readiness diagnostics distinguish missing, hidden, disabled, ambiguous, and ready known editors without recording their content. New-chat preparation retries a bounded composer hydration wait before filling anything, while retaining positive Chat-mode and effort checks. The `composer_wait` phase distinguishes this readiness wait from mode verification. It never retries Send. Post-send recovery remains one read-only refresh of the exact conversation.

These changes do not change browser-process cleanup, the pinned browser helper, fingerprint flags, normal Chrome, or profile selection. Browser lifecycle work is tracked separately in [issue #4](https://github.com/amxv/slingring/issues/4).

## Fixture verification

`cargo test --locked --test submission_intent` exercises cancellation on both sides of authorization, a failed journal save, conservative legacy recovery, and duplicate request IDs without a browser or real submission. Extension fixtures cover a composer that appears after the first hydration timeout and reject disabled, read-only, hidden, or ambiguous editors.
