---
title: Changelog
description: Release status and user-visible changes.
order: 99
category: Distribution
summary: Release status and user-visible changes.
---

## 0.1.0 · 3 November 2026

Initial private macOS release. Local CLI and remote Streamable HTTP MCP share one serial queue and a patched-browser runner. The CLI supports local image attachments; the single remote tool accepts only prompt and reasoning effort.

The browser adapter verifies Chat mode, supports all five reasoning levels when the account exposes them, and returns the conversation URL only after real assistant activity. It handles the current retained home view, user-bubble, and assistant Markdown structure. Stop controls, pulsing dots, and screen-reader loading text do not count as a successful handoff.

Remote mode uses a single outbound Rust WebSocket, a hibernating Cloudflare relay, sparse connection heartbeats, and no Redis or idle browser. Local-only mode exits when idle. The optional signed Swift menu bar wraps CLI commands and has no status polling timer.

The private installer supports authenticated GitHub downloads, paired CLI/app archives, checksum verification, and explicit update preferences. Private release artifacts still require repository access; there is no anonymous release download or npm package.
