---
title: Reasoning levels
description: Configure your subscription and choose a reasoning level.
order: 5
category: Guides
---

Select **Configure → Subscription → Plus or Pro** to match your account, or use the CLI:

```bash
slingring config set subscription plus
slingring config set default-effort High
```

Fresh installs use **Plus** and **High**. Plus allows **Instant, Medium, High**. Pro also allows **Extra High and Pro**. Configure → Default Reasoning and live MCP tool schemas show the levels for the configured tier. An explicitly requested level outside that tier is rejected before submission. The setting does not purchase or change your ChatGPT subscription.

Saved defaults are preserved. Older configuration with an Extra High or Pro default is treated as Pro when no subscription has been saved. The private `subscription.json` sidecar keeps the existing configuration compatible with older installed services; an older service does not gain these new controls until upgraded.

For a Pro account:

```bash
slingring config set subscription pro
slingring new-task "Review this architecture" --reasoning-effort "Extra High"
```

The selected ChatGPT account must also expose the requested level. New chats verify the selection before sending and never silently downgrade. Omitted or blank new-task effort uses your saved default. To retain the site's selection:

```bash
slingring config set default-effort site
```

An omitted follow-up effort never reads or changes the model picker. An explicit tier-allowed follow-up effort is best-effort: a missing or rehydrating picker does not block an otherwise ready message. Structured `requested_effort`, `verified_effort`, and `effort_verification` distinguish requested from verified settings. Legacy results without verification metadata remain unverified; a requested value alone is not proof of selection.

The CLI also accepts `med`, `xhigh`, and `extra-high`. These are shortcuts, not separate modes. A reasoning choice does not guarantee speed or answer quality.
