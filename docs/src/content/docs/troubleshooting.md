---
title: Troubleshooting
description: Inspect state before retrying a submission.
order: 7
category: Reference
summary: Inspect state before retrying a submission.
---

## Start with metadata

```bash
slingring status --json
slingring jobs --json
slingring config show --json
```

Do not paste credentials, prompts, browser cookies, or token-bearing relay URLs into reports.

| Symptom | Check |
| --- | --- |
| Browser cannot launch | The tested `agent-browser-browse` binary is installed and `browser_binary` points to it |
| Authentication required | The isolated Chrome profile is authenticated and `chrome_profile` is correct |
| Effort unavailable | The requested mode exists for this account; explicitly choose another if appropriate |
| Remote Mac offline | Service state, deployed relay hostname, device token, outbound network access |
| Timeout or `uncertain` | Inspect the job and ChatGPT before any new submission |
| Queue full | Let existing work drain; avoid retry storms |
| Menu bar cannot find CLI | Put the companion `slingring` executable beside the app bundle |

## Configuration

```bash
slingring config set browser_binary agent-browser-browse
slingring config set chrome_profile "Profile 6"
slingring config set timeout_secs 180
```

For foreground diagnostics, `slingring daemon` runs the daemon directly. It can process real queued work, so use it deliberately. Changing configuration does not justify resubmitting ambiguous jobs.
