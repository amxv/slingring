---
title: Send prompts and images
description: Use a prompt, a saved brief, or local images.
order: 2
category: Guides
---

Use Slingring to delegate a task to ChatGPT from any local agent or terminal. Send a prompt, a prepared brief, or images; get back the conversation link.

## From your terminal or local agent

```bash
slingring new-task "Review this plan and point out its biggest risks"
```

A local agent can run the same command. For longer prompts, ask it to write a brief to a file and send that file:

```bash
slingring new-task --prompt-file ./brief.md --reasoning-effort "Extra High"
```

Piped input works too:

```bash
cat brief.md | slingring new-task --stdin
```

## Attach an image

```bash
slingring new-task "Compare these designs" \
  --image ./first.png \
  --image ./second.png
```

Repeat `--image` for multiple files. Images must be on the Mac running Slingring. Remote MCP callers cannot read or upload local files.

## Get a result your agent can parse

```bash
slingring new-task --prompt-file ./brief.md --json
```

The JSON result includes the conversation URL, job ID, selected reasoning level, and the kind of activity observed. It does **not** include ChatGPT's answer. Open the returned link to read or continue the conversation.

You can send several requests together. They queue up and start one at a time; you do not need to manage browser windows.

## When a task fails

Read the error before trying again. An **uncertain** result means ChatGPT may have received the prompt. Check **Open Latest Chat**, your ChatGPT history, or `slingring jobs --json` first. Do not immediately create a replacement request.

The [command reference](/docs/command-reference) covers limits and safe retries.
