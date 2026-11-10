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
slingring login status
```

These commands do not open a browser. Do not include credentials or private MCP URLs in a bug report.

| Problem | What to do |
| --- | --- |
| Command not found | Run `~/.local/bin/slingring`, or add `~/.local/bin` to PATH. |
| No Chrome profiles | Open Google Chrome once, create/select a profile, and sign into ChatGPT. Reopen Configure → Chrome Profile. |
| Wrong account | Use Configure → Chrome Profile or `slingring profile use NAME`. Check the login in normal Chrome. |
| Browser helper missing | Run `slingring browser install`, then `slingring setup --profile NAME`. Explicit `browser-binary` overrides must point to a valid fork. |
| Extra High or Pro unavailable | Check Configure → Subscription matches your plan. Plus allows Instant/Medium/High; Pro also enables Extra High/Pro. The selected account must expose the requested level; new tasks never silently downgrade. |
| Remote offline | Keep the Mac awake and online, start the service, and check the relay credentials. |
| Remote URL not configured | Complete the one-time [relay setup](/docs/reference/relay). Local use still works. |
| Follow-up still waiting or chat busy | Readiness waits within the task-start timeout for a visible voice button and no stop control. Let the existing response finish; Slingring never interrupts it. A waiting follow-up can delay later queued tasks. |
| Follow-up URL rejected | Use the exact `https://chatgpt.com/c/ID` URL without query, fragment, or trailing slash. IDs are 1..128 ASCII letters, digits, or hyphens. Work, share, project, and custom GPT routes are unsupported. |
| Follow-up target or voice control unavailable | Verify that the selected account can open the chat and that its composer is ready. Login changes, redirects, and unknown layouts fail closed; do not repeatedly submit. |
| Unrelated draft present | Review the existing composer draft yourself. Slingring will not overwrite an unrelated draft. |
| `images_not_ready` or `attachments_changed` | Slingring could not prove that the exact requested attachment set was ready and unchanged. Remove-only layouts with no identifiable thumbnail are rejected because sibling previews can change undetected; even a cosmetic change to an already-ready attachment can fail the final check. Inspect the draft and recorded job outcome before deliberately trying again; never automatically resend. |
| An uncertain result | Check the target conversation and the job journal before trying another request. A message may already have been sent; never blindly repeat it. |
| Need to see the automated browser | Choose Configure → Browser Mode → Headed (Visible Browser), or run `slingring config set browser-mode headed`. The setting applies to newly submitted tasks. Restore the default with `slingring config set browser-mode headless`. |
| Menu shows a startup phase but no new chat | A task is still preparing or waiting. Reopen the menu to refresh its phase, or use `slingring jobs --request-id ID --json`. Submission is confirmed only after fresh ChatGPT activity is detected. |
| Last attempt says not submitted | Read the error code in the menu or journal and resolve that failure before deliberately submitting a new request. Historical attempts do not count as active tasks. |
| Service status unavailable | Saved history may be stale while the service is unresponsive. Do not assume the queue is empty or resend a request; query the same request ID when the service answers. |
| Service did not start after upgrade | Run `slingring service status` and `slingring login status`. A recently restarted LaunchAgent may take longer than five seconds to become ready due to macOS throttling. Slingring now waits for that window and reports any remaining failure with launchd status or code-signing hints. If it still fails, run `slingring daemon` in the foreground to see the specific error. |
| “Slingring was blocked from modifying apps” during a task | Upgrade to 0.6.4 or later. Two Chrome startup paths can modify installed apps under Slingring's responsibility: app cloning/hard-linking, disabled in 0.6.2, and a delayed GoogleUpdater wake that can replace installed Chrome, disabled in 0.6.4. These operations are unnecessary for task submission. The isolated browser disables both with process-local flags; normal Chrome updates, updater launchd jobs and signature checks stay unchanged. You do not need to grant App Management permission for these task-browser operations. |
| App Management turns off after a Slingring update | Ad-hoc signatures identify a particular build by its code hash, so an update can invalidate old consent even when its signing identifier stays the same. A reboot alone is not the confirmed cause. For ordinary task submissions, use 0.6.4 or later instead of repeatedly granting this permission. Installing/upgrading Slingring's own app is a separate operation and may require installer approval. |
| Menu bar version looks old | Quit the menu bar and reopen `~/.local/bin/Slingring.app`; keep the CLI beside it. |
| Menu bar does not return after reboot | Run `slingring login menu-bar status` and `slingring login menu-bar on`. If macOS reports `requires_approval`, approve Slingring in System Settings → General → Login Items & Extensions. |
| Service starts even after disabling autostart | `slingring login service off` affects **future logins**, not an already-running daemon. Use `slingring service stop` to stop it now. `slingring login service status` shows the persistent startup override. |

The Cloudflare test page is not the product's health check. A challenge there does not establish that ChatGPT submissions are broken. Keep compatibility experiments isolated from the working browser configuration.
