---
title: Get started
description: Install Slingring, choose your account, and start your first chat.
order: 1
category: Start here
---

Slingring lets local and remote agents delegate tasks to your ChatGPT account. It returns a conversation link as soon as ChatGPT starts, and your agent can use that link to send follow-ups. It does not retrieve the answer.

## 1. Install

You need **Apple Silicon macOS** or **x86_64 Linux with glibc 2.39+**, Google Chrome, and a ChatGPT account signed into Chrome. Public releases install without GitHub authentication. Linux currently requires system-installed Google Chrome Stable at `/opt/google/chrome/chrome` and a regular user session where Chrome's sandbox works. Root, CI and container browser execution is refused rather than disabling the sandbox.

```bash
curl -fsSL https://slingring.ashray.xyz/install.sh | sh
```

The installer sets up the CLI. macOS also gets the menu bar app and verified browser helper; Linux uses the separately installed stock Chrome Stable and has no tray app. Neither platform requires a separate agent-browser, Node, or Homebrew installation. Install Chrome yourself before installing Slingring.

On macOS the app opens in your menu bar. On both platforms the `slingring` command lives in `~/.local/bin`. If your shell cannot find it, use `~/.local/bin/slingring` in the commands below, or add that directory to your PATH.

On macOS, both **Configure → Open Menu Bar at Login** and **Configure → Start Slingring Service at Login** are enabled on first install. They are independent: you can hide the UI at future logins without disabling the service, or turn off service autostart without stopping a daemon that's already running. The same controls are available through `slingring login menu-bar on|off|status` and `slingring login service on|off|status`.

Linux installation does not register or enable a service. Local calls can start a private daemon directly. For an always-on user session, explicitly run `slingring service install`, `slingring login service on`, then `slingring service start`. This requires a working systemd user manager; it does not enable lingering or change system services. Without that manager, use `slingring daemon` in your own supervisor. Linux updates are manual with `slingring upgrade`; no automatic update timer is installed.

## 2. Choose your Chrome profile

On macOS, open **Slingring → Configure → Chrome Profile**, then select the profile where you are signed into ChatGPT. Profiles appear by name, not just a number. Your everyday Chrome windows stay untouched.

Prefer the terminal on macOS?

```bash
slingring profile list
slingring profile use "Profile 6"
```

Replace `Profile 6` with a directory or unique name from your own list. Slingring does not choose among multiple accounts for you.

On a Linux desktop, run `slingring setup` and then `slingring profile open` in an interactive terminal. A dedicated Chrome window opens with its own empty profile. Sign in manually, leave that window open, and press Enter in the terminal so Slingring can verify the browser shuts down cleanly. This prepares the same private profile for future local or remote jobs. It never copies your regular Chrome data. An SSH-only or headless server without a usable desktop session cannot perform this initial interactive setup.

Select Plus or Pro to match your account using **Configure → Subscription** on macOS or `slingring config set subscription plus` (or `pro`) on either platform. Fresh installs use Plus with High reasoning; saved defaults are preserved. Plus supports Instant, Medium, and High; Pro also supports Extra High and Pro. Use ChatGPT Setting retains the site's selection.

## 3. Send your first task

```bash
slingring new-task "Help me compare two approaches to this project" --reasoning-effort High
```

Slingring starts a new **Chat**, waits until ChatGPT actually begins thinking or responding, and prints the conversation link. Open it to continue. The temporary browser closes when there are no more tasks to start.

## Use it with an agent

A local agent such as Claude Code can run `slingring new-task` directly. Give it a brief, and ask it to return the link. For agents that connect over MCP, follow [Connect a remote assistant](/docs/remote).

For images and longer briefs, see [Send prompts and images](/docs/local-input).
