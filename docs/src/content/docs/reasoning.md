---
title: Reasoning levels
description: Choose a level for one task or set your default.
order: 5
category: Guides
---

Choose a default in **Configure → Default Reasoning**, or set it from the CLI:

```bash
slingring config set default-effort "Extra High"
```

Override it for an individual task:

```bash
slingring new-task "Review this architecture" --reasoning-effort Pro
```

Slingring recognizes **Instant, Medium, High, Extra High, and Pro**. The selected ChatGPT account must expose the requested level. Extra High and Pro are not available on every plan. Slingring checks the page before sending and never silently switches to a lower level.

Omitted or blank effort uses your Slingring default. To leave the site's current selection unchanged:

```bash
slingring config set default-effort site
```

The CLI also accepts `med`, `xhigh`, and `extra-high`. These are shortcuts for the same choices, not separate modes. A reasoning choice is not a guarantee of speed or answer quality.
