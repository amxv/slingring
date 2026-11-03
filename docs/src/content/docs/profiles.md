---
title: Choose your account
description: Switch Chrome profiles without touching your everyday browser.
order: 3
category: Guides
---

Slingring uses the ChatGPT login in the Chrome profile you select. Your Chrome profile name and your ChatGPT account are not necessarily the same: open Chrome in that profile and confirm the ChatGPT account yourself.

## In the menu bar

Open **Configure → Chrome Profile** and click a profile. A checkmark shows the selection. The list is refreshed when you open the submenu; it does not poll in the background.

The change applies to new tasks. Let any active or queued tasks finish before switching accounts.

## From the CLI

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

## Sign in or refresh your login

Open normal Chrome, switch to your chosen profile, and sign into ChatGPT. Then run a new Slingring task. Slingring uses an isolated copy of that profile; it does not take control of your regular windows or install its extension into your everyday browser.

A website challenge or expired login may require your attention. Slingring does not bypass a login or guarantee that a site will accept browser automation.
