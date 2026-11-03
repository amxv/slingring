---
title: Troubleshooting
description: Resolve setup problems without duplicate chats or messages.
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
| Follow-up still waiting or chat busy | Readiness waits within the task-start timeout for a visible voice button and no stop control. Let the existing response finish; Slingring never interrupts it. A waiting follow-up can delay later queued tasks. |
| Follow-up URL rejected | Use the exact `https://chatgpt.com/c/ID` URL without query, fragment, or trailing slash. IDs are 1..128 ASCII letters, digits, or hyphens. Work, share, project, and custom GPT routes are unsupported. |
| Follow-up target or voice control unavailable | Verify that the selected account can open the chat and that its composer is ready. Login changes, redirects, and unknown layouts fail closed; do not repeatedly submit. |
| Unrelated draft present | Review the existing composer draft yourself. Slingring will not overwrite an unrelated draft. |
| An uncertain result | Check the target conversation and the job journal before trying another request. A message may already have been sent; never blindly repeat it. |
| Menu bar version looks old | Quit the menu bar and reopen `~/.local/bin/Slingring.app`; keep the CLI beside it. |

The Cloudflare test page is not the product's health check. A challenge there does not establish that ChatGPT submissions are broken. Keep compatibility experiments isolated from the working browser configuration.
