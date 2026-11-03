---
title: Get started
description: Install Slingring, choose your account, and start your first chat.
order: 1
category: Start here
---

Slingring lets another agent start a conversation in your ChatGPT account and return the link. You continue in ChatGPT; the agent does not receive the answer.

## 1. Install

You need an **Apple Silicon Mac**, Google Chrome, and a ChatGPT account signed into Chrome. While Slingring is private, you also need an authenticated GitHub CLI (`gh`) with access to the repository.

```bash
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

One installer sets up the CLI, menu bar app, companion extension, and verified browser helper. There is no separate agent-browser, Node, or Homebrew installation step. Chrome itself and signing into your account remain yours to set up.

The app opens in your menu bar. The `slingring` command lives in `~/.local/bin`. If your shell cannot find it, use `~/.local/bin/slingring` in the commands below, or add that directory to your PATH.

## 2. Choose your Chrome profile

Open **Slingring → Configure → Chrome Profile**, then select the profile where you are signed into ChatGPT. Profiles appear by name, not just a number. Your everyday Chrome windows stay untouched.

Prefer the terminal?

```bash
slingring profile list
slingring profile use "Profile 6"
```

Replace `Profile 6` with a directory or unique name from your own list. Slingring does not choose among multiple accounts for you.

Under **Configure → Default Reasoning**, choose a level your account supports. The initial default is Extra High. Choose High for an account without Extra High, or Use ChatGPT Setting to retain the site's selection.

## 3. Send your first task

```bash
slingring new-task "Help me compare two approaches to this project" --reasoning-effort High
```

Slingring starts a new **Chat**, waits until ChatGPT actually begins thinking or responding, and prints the conversation link. Open it to continue. The temporary browser closes when there are no more tasks to start.

## Use it with an agent

A local agent such as Claude Code can run `slingring new-task` directly. Give it a brief, and ask it to return the link. For agents that connect over MCP, follow [Connect a remote assistant](/docs/remote).

For images and longer briefs, see [Send prompts and images](/docs/local-input).
