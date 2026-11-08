---
title: Connect a remote assistant
description: Let a remote assistant start chats and send follow-up messages.
order: 4
category: Guides
---

Remote access lets an always-on assistant or other remote agent delegate a task to ChatGPT through your Mac, then steer that conversation with follow-ups. The MCP tools return a link when the new turn starts, not the generated answer.

## When remote access is already configured

Open **Configure → Enable Remote Access**. The menu should say **Remote: Connected**.

Then choose **Configure → Copy Remote MCP URL**. Add that URL to your assistant as a remote MCP server. For a URL containing your private access key, select **No authentication** in the assistant's connection dialog: the URL itself is the credential.

You can copy it from the terminal on macOS:

```bash
slingring remote url | pbcopy
```

Keep the entire URL private. Anyone who has it can submit prompts through your account.

## What to ask your assistant

Ask it to use `new_task` with your prompt and, optionally, a reasoning level. For example: “Start a ChatGPT chat to review this plan using High reasoning, then give me the link.”

`new_task` accepts `prompt`, optional `reasoning_effort`, and optional `request_id`. Leave effort out, or leave it blank, to use the default configured on your Mac. Choose a fresh unique `request_id` before starting any job that may outlast your MCP client's timeout.

To continue that chat, ask the assistant to call `send_message` with `chatgpt_url`, `message`, optionally `reasoning_effort`, and a fresh `request_id`. Use the exact `https://chatgpt.com/c/...` link without a query or fragment. Other routes are unsupported. The follow-up opens that URL directly, skips the model picker when effort is omitted, waits for the voice button with no stop control, and sends once when ready. Explicit effort changes are best-effort; a missing picker is not a reason to block the message. Its confirmed result contains the same link after fresh activity starts; it does not wait for or retrieve the completed answer.

Both write tools reject extra fields. Messages are limited to 128 KiB. Local file paths and images are not exposed through MCP. Readiness uses the existing startup timeout; a busy chat can delay other queued requests.

### If the task is still starting

The Mac waits up to five seconds after accepting an MCP request for a confirmed result or failure. If it needs longer, the call returns a receipt containing `request_id`, `job_id`, `accepted: true`, `outcome: in_progress`, the current job phase, and `recovery_tool: get_job_status`. The accepted task continues in the Mac's queue. **Acceptance does not mean a message was submitted.**

Ask the assistant to query `get_job_status` with that ID until the outcome is terminal. Open the returned conversation URL after `submitted`. This keeps browser startup and busy conversations from consuming the calling client's entire timeout.

### If your agent times out before getting a receipt

The Mac may **keep working after the calling agent stops waiting**. A caller's timeout does not establish that the message failed or was sent. Instead of resubmitting, use the read-only `get_job_status` tool with the exact `request_id` you chose before the call:

```json
{"name":"get_job_status","arguments":{"request_id":"review-20261103-001"}}
```

The result distinguishes `in_progress`, `submitted`, `not_submitted`, `uncertain`, and `unknown` (missing/evicted record). Only `not_submitted` confirms nothing was sent; a missing record is **not** proof of failure. For a terminal failure, consult the recorded phase/error and the Mac's private `events.log`. Locally, run `slingring jobs --request-id review-20261103-001 --json`. Never repeat a message merely because a call timed out.

The **local CLI** still waits for a confirmed start and has a default 180-second browser-start budget, excluding local queue waiting. MCP uses the five-second receipt window after admission. The relay retains a 240-second HTTP safety limit and the Mac a 230-second remote job deadline, including queue wait, for compatibility with earlier installed versions; the configured browser budget may be shorter. Client and network timeouts remain outside Slingring's control. Accepted MCP tasks are not canceled when a caller disconnects. Their payload stays in memory and is never replayed after a daemon crash. See [CLI and MCP reference](/docs/command-reference) for details.

## Keep your Mac available

Your Mac must be awake, logged in, online, and signed into ChatGPT in the chosen Chrome profile. Enable **Configure → Start Service at Login** for automatic startup after login. The menu bar app can be closed without disabling the service.

To pause access, choose **Stop Service**. To disable only remote access, use **Configure → Disable Remote Access**.

## First-time remote setup

Remote access requires a relay in your own Cloudflare account; it is not a shared hosted service. Local CLI use does not require one. The one-time deployment and credential steps are in [Relay setup](/docs/reference/relay). After provisioning, normal use is just the menu and the connection URL.
