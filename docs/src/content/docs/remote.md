---
title: Connect a remote assistant
description: Give ChatGPT or another remote agent the new_task tool.
order: 4
category: Guides
---

Remote access lets an assistant start a ChatGPT chat through your Mac. You still get a conversation link, not the generated answer.

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

The tool accepts only `prompt` and `reasoning_effort`. Leave effort out, or leave it blank, to use the default configured on your Mac. Local file paths and images are not exposed remotely.

## Keep your Mac available

Your Mac must be awake, logged in, online, and signed into ChatGPT in the chosen Chrome profile. Enable **Configure → Start Service at Login** for automatic startup after login. The menu bar app can be closed without disabling the service.

To pause access, choose **Stop Service**. To disable only remote access, use **Configure → Disable Remote Access**.

## First-time remote setup

Remote access requires a relay in your own Cloudflare account; it is not a shared hosted service. Local CLI use does not require one. The one-time deployment and credential steps are in [Relay setup](/docs/reference/relay). After provisioning, normal use is just the menu and the connection URL.
