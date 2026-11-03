---
title: Reasoning modes
description: Choose an explicit mode or set your default.
order: 3
category: Reference
summary: Choose an explicit mode or set your default.
---

## Five choices

| Value | Use |
| --- | --- |
| `Instant` | Request the account's Instant option |
| `Medium` | Request Medium reasoning |
| `High` | Request High reasoning |
| `Extra High` | Request Extra High reasoning; initial default |
| `Pro` | Request the account's Pro option |

Availability depends on the ChatGPT account and current interface. Slingring must not silently fall back when a requested tier is unavailable. These labels are selections, not latency or quality guarantees.

```bash
slingring config set default_reasoning_effort "Extra High"
slingring new-task "Audit this proposal" --reasoning-effort Pro
```

Omitting the effort, or passing a blank effort, uses the configured default. Aliases include `med`, `xhigh`, and `extra-high`. The optional menu bar exposes all five defaults.
