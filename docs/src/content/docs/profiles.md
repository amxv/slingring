---
title: Choose your account
description: Switch Chrome profiles without touching your everyday browser.
order: 3
category: Guides
---

On macOS, Slingring uses the ChatGPT login in the Chrome profile you select. Your Chrome profile name and your ChatGPT account are not necessarily the same: open Chrome in that profile and confirm the ChatGPT account yourself.

## In the macOS menu bar

Open **Configure → Chrome Profile** and click a profile. A checkmark shows the selection. The list is refreshed when you open the submenu; it does not poll in the background.

The change applies to new tasks. Let any active or queued tasks finish before switching accounts.

## Select a macOS profile from the CLI

```bash
slingring profile list
slingring profile use "Profile 6"
```

You can also use a unique display name from the list:

```bash
slingring profile use "Personal"
```

When two profiles share a name, use the `Profile N` directory instead. To inspect the current choice:

```bash
slingring config show --json
```

## Sign in or refresh your login on macOS

Open normal Chrome, switch to your chosen profile, and sign into ChatGPT. Then run a new Slingring task. Slingring uses an isolated copy of that profile; it does not take control of your regular windows or install its extension into your everyday browser.

A website challenge or expired login may require your attention. Slingring does not bypass a login or guarantee that a site will accept browser automation.

## Set up the dedicated Linux profile

On Linux, there is one dedicated Slingring profile per private Slingring state directory. No profile list is read from normal Chrome, and `profile use` cannot choose a regular Chrome profile.

From an interactive Linux desktop terminal, run `slingring setup` and then `slingring profile open`. If the Slingring service is running, wait for jobs to finish and stop it first. Sign in manually in the opened dedicated window. Close Chrome normally or press Enter in the terminal; Slingring verifies the owned process actually exited. Cancel at the terminal with Ctrl-C to close without marking fresh setup complete. Repeat this process when you need to sign in again. No sign-in is automated. If Chrome uses the desktop keyring, use the same regular Linux user session for background jobs so Chrome can access that keyring; Slingring never enables weaker storage to compensate.

Linux launches Chrome with the sandbox enabled and a private browser control pipe. If Slingring cannot verify shutdown, it stops new launches rather than attaching to another Chrome instance or replacing the profile. Do not delete the profile or its ownership files to retry; verify what happened first. Older or incompatible Chrome builds fail closed rather than being launched with weaker settings.
