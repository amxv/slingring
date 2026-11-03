---
title: Connect a remote assistant
description: Let a remote assistant start chats and send follow-up messages.
order: 4
category: Guides
---

Remote access lets an assistant start a ChatGPT chat or follow up on an existing one through your Mac. You get a conversation link when the new turn starts, not the generated answer.

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

`new_task` accepts only `prompt` and optional `reasoning_effort`. Leave effort out, or leave it blank, to use the default configured on your Mac.

To continue that chat, ask the assistant to call `send_message` with `chatgpt_url` and `message`. Use the exact `https://chatgpt.com/c/...` link without a query or fragment. Other routes are unsupported. The follow-up preserves the chat's reasoning effort, waits for the voice button and no stop control, and sends once when ready. It returns the same link after fresh activity starts; it does not wait for or retrieve the completed answer.

Both tools reject extra fields. Messages are limited to 128 KiB. Local file paths and images are not exposed through MCP. Readiness uses the existing startup timeout; a busy chat can delay other queued requests. If a result is uncertain, inspect the conversation and `slingring jobs --json` before sending anything again. See [CLI and MCP reference](/docs/command-reference) for URL and queue limits.

## Keep your Mac available

Your Mac must be awake, logged in, online, and signed into ChatGPT in the chosen Chrome profile. Enable **Configure → Start Service at Login** for automatic startup after login. The menu bar app can be closed without disabling the service.

To pause access, choose **Stop Service**. To disable only remote access, use **Configure → Disable Remote Access**.

## First-time remote setup

Remote access requires a relay in your own Cloudflare account; it is not a shared hosted service. Local CLI use does not require one. The one-time deployment and credential steps are in [Relay setup](/docs/reference/relay). After provisioning, normal use is just the menu and the connection URL.
