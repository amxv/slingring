---
title: Changelog
description: Release status and user-visible changes.
order: 99
category: Distribution
summary: Release status and user-visible changes.
---

## 0.1.1 · November 3, 2026

Keep the browser's crash backstop longer than the configured task deadline without changing launch options as queued requests age. Retry a model-menu click that races page hydration, while retaining the one-shot submission guard and strict effort verification.

## 0.1.0 · November 3, 2026

Initial private release for macOS. Local CLI and remote Streamable HTTP clients feed one serial task queue. Local callers can attach images. The browser adapter verifies Chat mode, selects and reads back reasoning effort, and returns a canonical conversation URL only after genuine assistant activity starts.

The tested `agent-browser-browse` baseline remains in charge of Chrome launch and profile isolation. The companion extension uses a short-lived native host; the browser closes when the queue drains. Local-only use exits at idle. Remote access uses one Rust service and an outbound connection to a hibernating Cloudflare relay, without Redis or a tunnel process.

Includes duplicate-request protection, bounded metadata-only history, fail-closed UI checks, explicit uncertain outcomes without automatic replay, direct authenticated downloads, and an optional Swift menu bar wrapper with no polling timer.
