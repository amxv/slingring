---
title: Get started
description: Install Slingring, choose your account, and start your first chat.
order: 1
category: Start here
---

Slingring lets local and remote agents delegate tasks to your ChatGPT account. It returns a conversation link as soon as ChatGPT starts, and your agent can use that link to send follow-ups. It does not retrieve the answer.

## 1. Install

You need an **Apple Silicon Mac**, Google Chrome, and a ChatGPT account signed into Chrome. Public releases install without GitHub authentication.

```bash
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

One installer sets up the CLI, menu bar app, companion extension, and verified browser helper. There is no separate agent-browser, Node, or Homebrew installation step. Chrome itself and signing into your account remain yours to set up.

The app opens in your menu bar. The `slingring` command lives in `~/.local/bin`. If your shell cannot find it, use `~/.local/bin/slingring` in the commands below, or add that directory to your PATH.

Both **Configure → Open Menu Bar at Login** and **Configure → Start Slingring Service at Login** are enabled on first install. They are independent: you can hide the UI at future logins without disabling the service, or turn off service autostart without stopping a daemon that's already running. The same controls are available through `slingring login menu-bar on|off|status` and `slingring login service on|off|status`.

## 2. Choose your Chrome profile

Open **Slingring → Configure → Chrome Profile**, then select the profile where you are signed into ChatGPT. Profiles appear by name, not just a number. Your everyday Chrome windows stay untouched.

Prefer the terminal?

```bash
slingring profile list
slingring profile use "Profile 6"
```

Replace `Profile 6` with a directory or unique name from your own list. Slingring does not choose among multiple accounts for you.

Under **Configure → Subscription**, select Plus or Pro to match your account. Fresh installs use Plus with High reasoning; saved defaults are preserved. **Configure → Default Reasoning** shows levels for that tier: Plus supports Instant, Medium, and High; Pro also supports Extra High and Pro. Use ChatGPT Setting retains the site's selection.

## 3. Send your first task

```bash
slingring new-task "Help me compare two approaches to this project" --reasoning-effort High
```

Slingring starts a new **Chat**, waits until ChatGPT actually begins thinking or responding, and prints the conversation link. Open it to continue. The temporary browser closes when there are no more tasks to start.

## Use it with an agent

A local agent such as Claude Code can run `slingring new-task` directly. Give it a brief, and ask it to return the link. For agents that connect over MCP, follow [Connect a remote assistant](/docs/remote).

For images and longer briefs, see [Send prompts and images](/docs/local-input).
