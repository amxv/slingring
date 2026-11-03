---
title: Troubleshooting
description: Resolve setup problems without creating duplicate chats.
order: 26
category: Reference
---

## Check the current state

```bash
slingring status --json
slingring jobs --json
slingring config show --json
```

These commands do not open a browser. Do not include credentials or private MCP URLs in a bug report.

| Problem | What to do |
| --- | --- |
| Command not found | Run `~/.local/bin/slingring`, or add `~/.local/bin` to PATH. |
| No Chrome profiles | Open Google Chrome once, create/select a profile, and sign into ChatGPT. Reopen Configure → Chrome Profile. |
| Wrong account | Use Configure → Chrome Profile or `slingring profile use NAME`. Check the login in normal Chrome. |
| Browser helper missing | Run `slingring browser install`, then `slingring setup --profile NAME`. Explicit `browser-binary` overrides must point to a valid fork. |
| Extra High or Pro unavailable | Choose a level exposed by the selected account. Slingring will not silently downgrade. |
| Remote offline | Keep the Mac awake and online, start the service, and check the relay credentials. |
| Remote URL not configured | Complete the one-time [relay setup](/docs/reference/relay). Local use still works. |
| An uncertain result | Check ChatGPT history and the job journal before trying another request. |
| Menu bar version looks old | Quit the menu bar and reopen `~/.local/bin/Slingring.app`; keep the CLI beside it. |

The Cloudflare test page is not the product's health check. A challenge there does not establish that ChatGPT submissions are broken. Keep compatibility experiments isolated from the working browser configuration.
